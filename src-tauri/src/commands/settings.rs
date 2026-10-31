//! Commands for settings and integrations (PLAN.md A7): the settings with
//! their history, the models to choose from, the owner's accounts, and
//! tracker issues as requests.

use std::path::Path;

use bastion_core::settings::{Forge, Model, Oversight, Settings, Tracker};
use bastion_core::{Dial, Dials};
use bastion_protocol::{AccountView, IssueView, ModelOption, SettingsView, VersionView};
use serde_json::json;
use tauri::State;

use super::factory::{FactoryCell, factory, name};
use crate::factory::now;
use crate::{forge, settings_store};

fn word<T: for<'de> serde::Deserialize<'de>>(text: &str, what: &str) -> Result<T, String> {
    serde_json::from_value(json!(text)).map_err(|_| format!("{text:?} is not a {what}"))
}

fn view_of(s: &Settings) -> SettingsView {
    SettingsView {
        approve: name(s.dials.approve),
        review: name(s.dials.review),
        plan_provider: s.plan_model.provider.clone(),
        plan_model: s.plan_model.model.clone(),
        build_provider: s.build_model.provider.clone(),
        build_model: s.build_model.model.clone(),
        forge: name(s.forge),
        tracker: name(s.tracker),
    }
}

fn settings_of(v: &SettingsView) -> Result<Settings, String> {
    let model = |provider: &str, model: &str| Model {
        provider: provider.trim().into(),
        model: model.trim().into(),
    };
    Ok(Settings {
        dials: Dials {
            approve: word::<Dial>(&v.approve, "dial")?,
            review: word::<Dial>(&v.review, "dial")?,
            ..Dials::default()
        },
        plan_model: model(&v.plan_provider, &v.plan_model),
        build_model: model(&v.build_provider, &v.build_model),
        forge: word::<Forge>(&v.forge, "forge")?,
        tracker: word::<Tracker>(&v.tracker, "tracker")?,
    })
}

#[tauri::command]
pub async fn settings(cell: State<'_, FactoryCell>) -> Result<SettingsView, String> {
    Ok(view_of(
        &settings_store::current(&**factory(&cell)?.store()).await?,
    ))
}

#[tauri::command]
pub async fn settings_set(
    cell: State<'_, FactoryCell>,
    settings: SettingsView,
) -> Result<(), String> {
    let next = settings_of(&settings)?;
    settings_store::change(&**factory(&cell)?.store(), &next, now())
        .await
        .map(|_| ())
}

/// Set both dials from one oversight preset.
#[tauri::command]
pub async fn settings_preset(cell: State<'_, FactoryCell>, preset: String) -> Result<(), String> {
    let factory = factory(&cell)?;
    let mut next = settings_store::current(&**factory.store()).await?;
    next.dials = word::<Oversight>(&preset, "preset")?.dials();
    settings_store::change(&**factory.store(), &next, now())
        .await
        .map(|_| ())
}

#[tauri::command]
pub async fn settings_history(cell: State<'_, FactoryCell>) -> Result<Vec<VersionView>, String> {
    let versions = factory(&cell)?.store().versions().await?;
    Ok(versions
        .into_iter()
        .map(|v| VersionView {
            version: v.version,
            at: v.at,
            note: v.note,
        })
        .collect())
}

#[tauri::command]
pub async fn settings_restore(cell: State<'_, FactoryCell>, version: u64) -> Result<(), String> {
    settings_store::restore(&**factory(&cell)?.store(), version, now())
        .await
        .map(|_| ())
}

/// The chat models with tools on providers that have a key.
#[tauri::command]
pub async fn models(cell: State<'_, FactoryCell>) -> Result<Vec<ModelOption>, String> {
    let factory = factory(&cell)?;
    let providers = factory
        .engine()
        .call("bastion", "router::provider::list", json!({}))
        .await?;
    let keyed: Vec<&str> = providers["providers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["configured"] == true)
        .filter_map(|p| p["id"].as_str())
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

/// Each account Bastion uses, and whether it answers.
#[tauri::command]
pub async fn accounts(cell: State<'_, FactoryCell>) -> Result<Vec<AccountView>, String> {
    let factory = factory(&cell)?;
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
    let mut all = vec![github];
    let providers = factory
        .engine()
        .call("bastion", "router::provider::list", json!({}))
        .await?;
    for p in providers["providers"].as_array().into_iter().flatten() {
        let ok = p["configured"] == true;
        let status = if ok {
            format!(
                "key from {}",
                p["credential_source"].as_str().unwrap_or("?")
            )
        } else {
            format!(
                "no key: set {}",
                p["credential_env_var"].as_str().unwrap_or("its key")
            )
        };
        all.push(AccountView {
            name: p["display_name"].as_str().unwrap_or_default().into(),
            ok,
            status,
        });
    }
    Ok(all)
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

#[tauri::command]
pub async fn issues(
    cell: State<'_, FactoryCell>,
    machine: State<'_, std::sync::Arc<dyn crate::machine::Machine>>,
    workspace: String,
) -> Result<Vec<IssueView>, String> {
    let slug = workspace_slug(&cell, machine.inner().as_ref(), &workspace).await?;
    let found = forge::GitHub {
        engine: &**factory(&cell)?.engine(),
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
#[tauri::command]
pub async fn job_from_issue(
    cell: State<'_, FactoryCell>,
    machine: State<'_, std::sync::Arc<dyn crate::machine::Machine>>,
    workspace: String,
    number: u64,
) -> Result<String, String> {
    let slug = workspace_slug(&cell, machine.inner().as_ref(), &workspace).await?;
    let factory = factory(&cell)?;
    let issue = forge::GitHub {
        engine: &**factory.engine(),
        slug,
    }
    .issue(number)
    .await?;
    let request = format!("{}\n\n{}", issue.title, issue.body.trim());
    factory
        .submit_with(&workspace, request.trim(), Some(issue.url))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_settings_view_reads_back_and_a_bad_word_is_refused() {
        let s = Settings {
            forge: Forge::Github,
            tracker: Tracker::GithubIssues,
            ..Settings::default()
        };
        assert_eq!(settings_of(&view_of(&s)), Ok(s));
        let bad = SettingsView {
            forge: "gitlab".into(),
            ..view_of(&Settings::default())
        };
        assert!(settings_of(&bad).unwrap_err().contains("gitlab"));
    }
}
