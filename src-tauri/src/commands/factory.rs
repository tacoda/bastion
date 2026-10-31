//! Commands for the floor, the inbox and the job view (PLAN.md A3).

use std::path::Path;
use std::sync::{Arc, OnceLock};

use bastion_core::{Dials, Event, actions, hold_until, steps};
use bastion_protocol::{
    ArtifactView, CharterView, DialsView, HeldCallView, JobDetail, JobSummary, StepView,
    TranscriptLine, Workspace,
};
use serde::Serialize;
use serde_json::json;
use tauri::State;

use crate::factory::Factory;
use crate::machine::Machine;
use crate::store::{self, Record};

/// Set once the engine is up and the store has its tables.
pub type FactoryCell = Arc<OnceLock<Arc<Factory>>>;

pub(super) fn factory(cell: &FactoryCell) -> Result<Arc<Factory>, String> {
    cell.get()
        .cloned()
        .ok_or_else(|| "the factory starts once the engine is up; start the stack".into())
}

/// An enum as its snake_case name.
pub(super) fn name<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

fn summary(r: &Record, dials: &Dials) -> JobSummary {
    let j = &r.job;
    JobSummary {
        id: j.id.clone(),
        workspace: j.workspace.clone(),
        request: j.request.clone(),
        stage: name(j.stage),
        state: name(j.state),
        question: j.question.clone(),
        error: j.error.clone(),
        created_at: r.created_at,
        updated_at: j.updated_at,
        hold_until: hold_until(j, dials),
        actions: actions(j).into_iter().map(name).collect(),
    }
}

#[tauri::command]
pub async fn workspaces(cell: State<'_, FactoryCell>) -> Result<Vec<Workspace>, String> {
    let all = factory(&cell)?.store().workspaces().await?;
    Ok(all
        .into_iter()
        .map(|w| Workspace {
            name: w.name,
            path: w.path,
            branch: w.branch,
        })
        .collect())
}

/// Register a repository. The path must be a git repository with the branch.
#[tauri::command]
pub async fn workspace_add(
    cell: State<'_, FactoryCell>,
    machine: State<'_, Arc<dyn Machine>>,
    workspace: Workspace,
) -> Result<(), String> {
    let (name, path, branch) = (
        workspace.name.trim(),
        workspace.path.trim(),
        workspace.branch.trim(),
    );
    if name.is_empty() || path.is_empty() || branch.is_empty() {
        return Err("a workspace needs a name, a path and a branch".into());
    }
    let verify = format!("refs/heads/{branch}");
    let out = machine.run(
        "git",
        &["rev-parse", "--verify", "--quiet", &verify],
        Some(Path::new(path)),
    )?;
    if !out.success {
        return Err(format!(
            "{path} is not a git repository with a branch {branch}"
        ));
    }
    let row = store::Workspace {
        name: name.into(),
        path: path.into(),
        branch: branch.into(),
    };
    factory(&cell)?.store().add_workspace(&row).await
}

#[tauri::command]
pub async fn jobs(cell: State<'_, FactoryCell>) -> Result<Vec<JobSummary>, String> {
    let factory = factory(&cell)?;
    let dials = factory.store().dials().await?;
    Ok(factory
        .store()
        .jobs()
        .await?
        .iter()
        .map(|r| summary(r, &dials))
        .collect())
}

/// The latest turn's messages, or none when the job has no turn or the
/// session cannot be read.
async fn transcript(factory: &Factory, session: Option<&str>) -> Vec<TranscriptLine> {
    let Some(session) = session else {
        return Vec::new();
    };
    let Ok(out) = factory
        .engine()
        .call(
            "bastion",
            "session::messages",
            json!({ "session_id": session }),
        )
        .await
    else {
        return Vec::new();
    };
    out["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| TranscriptLine {
            role: m["message"]["role"].as_str().unwrap_or_default().into(),
            text: m["message"]["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        })
        .collect()
}

#[tauri::command]
pub async fn job(cell: State<'_, FactoryCell>, id: String) -> Result<JobDetail, String> {
    let factory = factory(&cell)?;
    let record = factory.store().load(&id).await?;
    let artifacts = factory
        .store()
        .artifacts(&id)
        .await?
        .into_iter()
        .map(|a| ArtifactView {
            stage: name(a.stage),
            body: a.body,
            created_at: a.created_at,
        })
        .collect();
    let transcript = transcript(&factory, record.runtime.session.as_deref()).await;
    let steps = steps(&record.job)
        .into_iter()
        .map(|(stage, state)| StepView {
            stage: name(stage),
            state: name(state),
        })
        .collect();
    let held = factory
        .store()
        .held(&id)
        .await?
        .into_iter()
        .map(|h| HeldCallView {
            id: h.id,
            function: h.call.function,
            text: h.call.text,
            at: h.at,
        })
        .collect();
    Ok(JobDetail {
        job: summary(&record, &factory.store().dials().await?),
        steps,
        artifacts,
        transcript,
        held,
        charter: factory.gate().charter(&id).map(|c| CharterView {
            files: c.files.clone(),
            rules: c.constraints.iter().map(|r| r.rule.clone()).collect(),
            hook_runs: factory.gate().hook_runs(&id),
            findings: factory.gate().findings(&id),
        }),
    })
}

#[tauri::command]
pub async fn job_submit(
    cell: State<'_, FactoryCell>,
    workspace: String,
    request: String,
) -> Result<String, String> {
    if request.trim().is_empty() {
        return Err("a request needs text".into());
    }
    factory(&cell)?.submit(&workspace, request.trim()).await
}

#[tauri::command]
pub async fn job_approve(cell: State<'_, FactoryCell>, id: String) -> Result<(), String> {
    factory(&cell)?.apply(&id, Event::Approved).await
}

#[tauri::command]
pub async fn job_comment(
    cell: State<'_, FactoryCell>,
    id: String,
    text: String,
) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("a comment needs text".into());
    }
    factory(&cell)?
        .apply(
            &id,
            Event::Commented {
                text: text.trim().into(),
            },
        )
        .await
}

#[tauri::command]
pub async fn job_stop(cell: State<'_, FactoryCell>, id: String) -> Result<(), String> {
    factory(&cell)?.apply(&id, Event::Stopped).await
}

#[tauri::command]
pub async fn dials(cell: State<'_, FactoryCell>) -> Result<DialsView, String> {
    let d = factory(&cell)?.store().dials().await?;
    Ok(DialsView {
        approve: name(d.approve),
        review: name(d.review),
    })
}

#[tauri::command]
pub async fn dials_set(cell: State<'_, FactoryCell>, dials: DialsView) -> Result<(), String> {
    let parse = |text: &str| {
        serde_json::from_value(json!(text))
            .map_err(|_| format!("{text:?} is not ask, hold or auto"))
    };
    let dials = Dials {
        approve: parse(&dials.approve)?,
        review: parse(&dials.review)?,
    };
    factory(&cell)?.store().set_dials(&dials).await
}
