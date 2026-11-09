//! Commands for settings and integrations: the settings with
//! their history, the models to choose from, the owner's accounts, and
//! tracker issues as requests.

use std::path::Path;

use bastion_core::settings::{Forge, Model, Oversight, Settings, Tracker};
use bastion_protocol::{AccountView, IssueView, ModelOption, SettingsView, VersionView};
use serde_json::json;

use super::factory::{FactoryCell, factory, name};
use crate::events::Bus;
use crate::factory::{Submission, now};
use crate::{forge, settings_store};
use bastion_protocol::Change;

/// Tell every page the settings changed, so each section reads them again.
fn changed(events: &Bus) {
    events.publish(Change::Job { id: String::new() });
}

fn word<T: for<'de> serde::Deserialize<'de>>(text: &str, what: &str) -> Result<T, String> {
    serde_json::from_value(json!(text)).map_err(|_| format!("{text:?} is not a {what}"))
}

fn view_of(s: &Settings) -> SettingsView {
    SettingsView {
        plan_provider: s.plan_model.provider.clone(),
        plan_model: s.plan_model.model.clone(),
        build_provider: s.build_model.provider.clone(),
        build_model: s.build_model.model.clone(),
        forge: name(s.forge),
        tracker: name(s.tracker),
    }
}

/// The settings a client sent, with `was`'s dials, which it sets apart.
fn settings_of(v: &SettingsView, was: Settings) -> Result<Settings, String> {
    let model = |provider: &str, model: &str| Model {
        provider: provider.trim().into(),
        model: model.trim().into(),
    };
    Ok(Settings {
        dials: was.dials,
        plan_model: model(&v.plan_provider, &v.plan_model),
        build_model: model(&v.build_provider, &v.build_model),
        forge: word::<Forge>(&v.forge, "forge")?,
        tracker: word::<Tracker>(&v.tracker, "tracker")?,
    })
}

pub async fn settings(cell: &FactoryCell) -> Result<SettingsView, String> {
    Ok(view_of(
        &settings_store::current(&**factory(cell)?.store()).await?,
    ))
}

pub async fn settings_set(
    events: &Bus,
    cell: &FactoryCell,
    settings: SettingsView,
) -> Result<(), String> {
    let store = factory(cell)?.store().clone();
    let next = settings_of(&settings, settings_store::current(&*store).await?)?;
    settings_store::change(&*store, &next, now()).await?;
    changed(events);
    Ok(())
}

/// Set both dials from one oversight preset.
pub async fn settings_preset(
    events: &Bus,
    cell: &FactoryCell,
    preset: String,
) -> Result<(), String> {
    let factory = factory(cell)?;
    let mut next = settings_store::current(&**factory.store()).await?;
    next.dials = word::<Oversight>(&preset, "preset")?.dials();
    settings_store::change(&**factory.store(), &next, now()).await?;
    changed(events);
    Ok(())
}

pub async fn settings_history(cell: &FactoryCell) -> Result<Vec<VersionView>, String> {
    let versions = factory(cell)?.store().versions().await?;
    Ok(versions
        .into_iter()
        .map(|v| VersionView {
            version: v.version,
            at: v.at,
            note: v.note,
        })
        .collect())
}

pub async fn settings_restore(
    events: &Bus,
    cell: &FactoryCell,
    version: u64,
) -> Result<(), String> {
    settings_store::restore(&**factory(cell)?.store(), version, now()).await?;
    changed(events);
    Ok(())
}

/// The chat models with tools on providers that are ready: a key, a
/// sign-in, or a local host that needs neither.
pub async fn models(cell: &FactoryCell) -> Result<Vec<ModelOption>, String> {
    let factory = factory(cell)?;
    let providers = super::providers::providers(&**factory.engine()).await?;
    let keyed: Vec<&str> = providers
        .iter()
        .filter(|p| p.ready)
        .map(|p| p.id.as_str())
        .collect();
    let models = factory
        .engine()
        .call("bastion", "router::models::list", json!({}))
        .await?;
    Ok(models["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| {
            m["supports_tools"] == true && keyed.contains(&m["provider"].as_str().unwrap_or(""))
        })
        .map(|m| ModelOption {
            provider: m["provider"].as_str().unwrap_or_default().into(),
            id: m["id"].as_str().unwrap_or_default().into(),
            name: m["display_name"]
                .as_str()
                .or(m["id"].as_str())
                .unwrap_or_default()
                .into(),
        })
        .collect())
}

/// Each account Bastion uses, and whether it answers. Model providers have
/// their own section (D34).
pub async fn accounts(cell: &FactoryCell) -> Result<Vec<AccountView>, String> {
    let factory = factory(cell)?;
    let github = match forge::gh(&**factory.engine(), &["api", "user", "--jq", ".login"]).await {
        Ok(login) => AccountView {
            name: "GitHub".into(),
            ok: true,
            status: format!("signed in as {}", login.trim()),
        },
        Err(e) => AccountView {
            name: "GitHub".into(),
            ok: false,
            status: e,
        },
    };
    Ok(vec![github])
}

/// The GitHub repository behind a workspace's checkout.
async fn workspace_slug(
    cell: &FactoryCell,
    machine: &dyn crate::machine::Machine,
    workspace: &str,
) -> Result<String, String> {
    let all = factory(cell)?.store().workspaces().await?;
    let ws = all
        .iter()
        .find(|w| w.name == workspace)
        .ok_or(format!("no workspace {workspace}"))?;
    forge::slug(machine, Path::new(&ws.path))
}

pub async fn issues(
    cell: &FactoryCell,
    machine: &std::sync::Arc<dyn crate::machine::Machine>,
    workspace: String,
) -> Result<Vec<IssueView>, String> {
    let slug = workspace_slug(cell, machine.as_ref(), &workspace).await?;
    let found = forge::GitHub {
        engine: &**factory(cell)?.engine(),
        slug,
    }
    .issues()
    .await?;
    Ok(found
        .into_iter()
        .map(|i| IssueView {
            number: i.number,
            title: i.title,
            url: i.url,
        })
        .collect())
}

/// Start a job from a tracker issue: its title and body are the request,
/// and the job links back to it.
/// An issue to start a job from, and who asked.
pub struct FromIssue {
    pub workspace: String,
    pub number: u64,
    pub requester: Option<String>,
}

pub async fn job_from_issue(
    cell: &FactoryCell,
    machine: &std::sync::Arc<dyn crate::machine::Machine>,
    from: FromIssue,
) -> Result<String, String> {
    let FromIssue {
        workspace,
        number,
        requester,
    } = from;
    let slug = workspace_slug(cell, machine.as_ref(), &workspace).await?;
    let factory = factory(cell)?;
    let issue = forge::GitHub {
        engine: &**factory.engine(),
        slug,
    }
    .issue(number)
    .await?;
    let request = format!("{}\n\n{}", issue.title, issue.body.trim());
    factory
        .submit_with(
            &workspace,
            request.trim(),
            Submission {
                issue: Some(issue.url),
                requester,
                ..Submission::default()
            },
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastion_core::{Dial, Stage};

    #[test]
    fn a_settings_view_reads_back_keeps_the_dials_it_hides_and_refuses_a_bad_word() {
        let mut s = Settings {
            forge: Forge::Github,
            tracker: Tracker::GithubIssues,
            ..Settings::default()
        };
        s.dials.set(Stage::Plan, Dial::Dark);
        s.dials.set(Stage::Discover, Dial::Dark);
        assert_eq!(settings_of(&view_of(&s), s.clone()), Ok(s));
        let bad = SettingsView {
            forge: "gitlab".into(),
            ..view_of(&Settings::default())
        };
        assert!(
            settings_of(&bad, Settings::default())
                .unwrap_err()
                .contains("gitlab")
        );
    }
}
