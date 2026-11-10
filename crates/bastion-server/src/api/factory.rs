//! Commands for the floor, the inbox and the job view.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use bastion_core::{Dial, Dials, Event, Pipeline, Stage, actions, steps};
use bastion_protocol::{
    ArtifactView, CharterView, DialsView, HeldCallView, JobDetail, JobSummary, PipelineDials,
    StepDial, StepView, TranscriptLine, Workspace,
};
use serde::{Deserialize, Serialize};
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
        parent: j.parent.clone(),
        part: j.part.clone(),
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
            max_jobs: w.max_jobs,
            revision_budget: w.revision_budget,
            branch_prefix: w.branch_prefix,
            setup: w.setup,
            cleanup: w.cleanup,
            env: w.env,
        })
        .collect())
}

/// Register a repository. The path must be a git repository with the branch.
pub async fn workspace_add(
    cell: &FactoryCell,
    machine: &Arc<dyn Machine>,
    w: Workspace,
) -> Result<(), String> {
    let (name, path) = (w.name.trim(), w.path.trim());
    if name.is_empty() || path.is_empty() {
        return Err("a workspace needs a name, a path and a branch".into());
    }
    let mut row = store::Workspace {
        name: name.into(),
        path: path.into(),
        ..store::Workspace::default()
    };
    let settings = WorkspaceSettings {
        branch: w.branch,
        checks: w.checks,
        max_jobs: w.max_jobs,
        revision_budget: w.revision_budget,
        branch_prefix: w.branch_prefix,
        setup: w.setup,
        cleanup: w.cleanup,
        env: w.env,
    };
    settle(machine, &mut row, settings)?;
    factory(cell)?.store().add_workspace(&row).await
}

/// What a person may change in a workspace.
#[derive(Deserialize)]
pub struct WorkspaceSettings {
    pub branch: String,
    pub checks: String,
    pub max_jobs: Option<u32>,
    pub revision_budget: Option<u32>,
    pub branch_prefix: String,
    pub setup: String,
    pub cleanup: String,
    pub env: String,
}

/// Change a workspace's settings. Its name and its path stay.
pub async fn workspace_edit(
    cell: &FactoryCell,
    machine: &Arc<dyn Machine>,
    name: &str,
    edit: WorkspaceSettings,
) -> Result<(), String> {
    let factory = factory(cell)?;
    let all = factory.store().workspaces().await?;
    let mut row = all
        .into_iter()
        .find(|w| w.name == name)
        .ok_or_else(|| format!("no workspace named {name}"))?;
    settle(machine, &mut row, edit)?;
    factory.store().edit_workspace(&row).await
}

/// Put `s` in `row`, trimmed, once each setting is one a job can use: the
/// branch is in the repository, git takes the prefix, and the environment
/// is `K=V` pairs.
fn settle(
    machine: &Arc<dyn Machine>,
    row: &mut store::Workspace,
    s: WorkspaceSettings,
) -> Result<(), String> {
    let branch = s.branch.trim();
    if branch.is_empty() {
        return Err("a workspace needs a branch".into());
    }
    has_branch(machine, &row.path, branch)?;
    row.branch = branch.into();
    row.branch_prefix = prefix(machine, &s.branch_prefix)?;
    row.env = s.env.trim().into();
    row.env_vars()?;
    row.checks = s.checks.trim().into();
    row.max_jobs = s.max_jobs.filter(|n| *n > 0);
    row.revision_budget = s.revision_budget.filter(|n| *n > 0);
    (row.setup, row.cleanup) = (s.setup.trim().into(), s.cleanup.trim().into());
    Ok(())
}

/// A branch prefix, trimmed, or why git would refuse a branch named with it.
fn prefix(machine: &Arc<dyn Machine>, typed: &str) -> Result<String, String> {
    let prefix = typed.trim();
    let sample = format!("refs/heads/{prefix}j1");
    let out = machine.run("git", &["check-ref-format", &sample], None)?;
    match out.success {
        true => Ok(prefix.into()),
        false => Err(format!("{prefix} cannot start a git branch name")),
    }
}

/// Refuse a path that is not a git repository with `branch`.
fn has_branch(machine: &Arc<dyn Machine>, path: &str, branch: &str) -> Result<(), String> {
    let verify = format!("refs/heads/{branch}");
    let out = machine.run(
        "git",
        &["rev-parse", "--verify", "--quiet", &verify],
        Some(Path::new(path)),
    )?;
    match out.success {
        true => Ok(()),
        false => Err(format!(
            "{path} is not a git repository with a branch {branch}"
        )),
    }
}

pub async fn jobs(cell: &FactoryCell, page: super::Page) -> Result<Vec<JobSummary>, String> {
    let factory = factory(cell)?;
    let (limit, offset) = page.bounds();
    let some = factory.store().jobs(limit, offset).await?;
    Ok(some.iter().map(summary).collect())
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
    let steps = record
        .job
        .step_ids()
        .into_iter()
        .zip(steps(&record.job))
        .map(|(id, (stage, state))| StepView {
            id,
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
        workflow: super::workflows::of_job(&**factory.store(), &record).await?,
        results: record.job.results.clone(),
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

/// Run a failed job, or its stuck step, again.
pub async fn job_retry(cell: &FactoryCell, id: String, by: String) -> Result<(), String> {
    factory(cell)?.decide(&id, Event::Retried, &by).await
}

/// Give the job's running turn its full cap again, from now.
pub async fn job_extend(cell: &FactoryCell, id: String, by: String) -> Result<(), String> {
    factory(cell)?.decide(&id, Event::Extended, &by).await
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
        level: name(d.level(&pipeline.stages())),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::ThisMachine;

    #[test]
    fn a_branch_prefix_is_one_git_takes() {
        let git: Arc<dyn Machine> = Arc::new(ThisMachine);
        assert_eq!(prefix(&git, " ian/ "), Ok("ian/".into()));
        assert_eq!(prefix(&git, ""), Ok("".into()));
        assert!(prefix(&git, "a b/").is_err());
        assert!(prefix(&git, "x..").is_err());
    }
}
