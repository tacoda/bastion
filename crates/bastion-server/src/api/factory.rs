//! Commands for the floor, the inbox and the job view (PLAN.md A3).

use std::path::Path;
use std::sync::{Arc, OnceLock};

use bastion_core::{Dial, Dials, Event, Pipeline, Stage, actions, steps};
use bastion_protocol::{
    ArtifactView, CharterView, DialsView, HeldCallView, JobDetail, JobSummary, PipelineDials,
    StepDial, StepView, TranscriptLine, Workspace,
};
use serde::Serialize;
use serde_json::json;

use crate::factory::{Factory, Submission};
use crate::machine::Machine;
use crate::store::{self, Record};

/// Set once the engine is up and the store has its tables.
pub type FactoryCell = Arc<OnceLock<Arc<Factory>>>;

/// The answer while the factory waits for the engine.
pub const NOT_STARTED: &str = "the factory starts once the engine is up; start the stack";

pub(super) fn factory(cell: &FactoryCell) -> Result<Arc<Factory>, String> {
    cell.get().cloned().ok_or_else(|| NOT_STARTED.into())
}

/// An enum as its snake_case name.
pub(super) fn name<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

fn summary(r: &Record) -> JobSummary {
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
        actions: actions(j).into_iter().map(name).collect(),
        pr: j.pr.clone(),
        issue: r.runtime.issue.clone(),
        requester: r.runtime.requester.clone(),
    }
}

pub async fn workspaces(cell: &FactoryCell) -> Result<Vec<Workspace>, String> {
    let all = factory(cell)?.store().workspaces().await?;
    Ok(all
        .into_iter()
        .map(|w| Workspace {
            name: w.name,
            path: w.path,
            branch: w.branch,
            checks: w.checks,
        })
        .collect())
}

/// Register a repository. The path must be a git repository with the branch.
pub async fn workspace_add(
    cell: &FactoryCell,
    machine: &Arc<dyn Machine>,
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
        checks: workspace.checks.trim().into(),
    };
    factory(cell)?.store().add_workspace(&row).await
}

pub async fn jobs(cell: &FactoryCell) -> Result<Vec<JobSummary>, String> {
    let factory = factory(cell)?;
    Ok(factory.store().jobs().await?.iter().map(summary).collect())
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

pub async fn job(cell: &FactoryCell, id: String) -> Result<JobDetail, String> {
    let factory = factory(cell)?;
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
        job: summary(&record),
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

/// Submit a request. `how` names its pipeline and who asked.
pub async fn job_submit(
    cell: &FactoryCell,
    workspace: String,
    request: String,
    how: Submission,
) -> Result<String, String> {
    if request.trim().is_empty() {
        return Err("a request needs text".into());
    }
    factory(cell)?
        .submit_with(&workspace, request.trim(), how)
        .await
}

pub async fn job_approve(cell: &FactoryCell, id: String, by: String) -> Result<(), String> {
    factory(cell)?.decide(&id, Event::Approved, &by).await
}

/// A note from `by`: an answer to the job's question, or the step sent back.
pub async fn job_comment(
    cell: &FactoryCell,
    id: String,
    text: String,
    by: String,
) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("a comment needs text".into());
    }
    let event = Event::Commented {
        text: text.trim().into(),
    };
    factory(cell)?.decide(&id, event, &by).await
}

pub async fn job_stop(cell: &FactoryCell, id: String) -> Result<(), String> {
    factory(cell)?.apply(&id, Event::Stopped).await
}

/// The dials of `pipeline`'s steps, in order, and the level they make.
fn pipeline_dials(d: &Dials, pipeline: Pipeline) -> PipelineDials {
    PipelineDials {
        level: name(d.level(pipeline)),
        steps: pipeline
            .stages()
            .into_iter()
            .map(|s| StepDial {
                stage: name(s),
                dial: name(d.at(s)),
            })
            .collect(),
    }
}

pub async fn dials(cell: &FactoryCell) -> Result<DialsView, String> {
    let d = crate::settings_store::current(&**factory(cell)?.store())
        .await?
        .dials;
    Ok(DialsView {
        short: pipeline_dials(&d, Pipeline::Short),
        full: pipeline_dials(&d, Pipeline::Full),
    })
}

/// Set one step's dial, as a new settings version.
/// A stage no pipeline runs, such as an old checkpoint, is not a step.
pub async fn dials_set(cell: &FactoryCell, stage: Stage, dial: Dial) -> Result<(), String> {
    if !stage.dialed() {
        return Err(format!("{} is not a step", name(stage)));
    }
    let factory = factory(cell)?;
    let mut next = crate::settings_store::current(&**factory.store()).await?;
    next.dials.set(stage, dial);
    crate::settings_store::change(&**factory.store(), &next, crate::factory::now())
        .await
        .map(|_| ())
}
