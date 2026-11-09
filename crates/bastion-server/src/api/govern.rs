//! Commands for held calls, the two ladders and the audit.

use std::sync::Arc;

use bastion_core::govern::{Constraint, Source};
use bastion_protocol::{
    AuditEntryView, AuditView, CapabilityView, ConstraintView, FactoryCapabilityView, HeldCallRef,
    WorkspaceCharterView,
};
use serde_json::json;

use super::factory::{FactoryCell, factory, name};
use crate::factory::now;
use crate::machine::Machine;

pub async fn held_allow(cell: &FactoryCell, held: HeldCallRef) -> Result<(), String> {
    factory(cell)?
        .gate()
        .allow_once(&held.job, &held.call)
        .await
}

pub async fn held_refuse(
    cell: &FactoryCell,
    held: HeldCallRef,
    feedback: String,
) -> Result<(), String> {
    if feedback.trim().is_empty() {
        return Err("say what the agent should do instead".into());
    }
    factory(cell)?
        .gate()
        .refuse(&held.job, &held.call, feedback.trim())
        .await
}

pub async fn constraints(cell: &FactoryCell) -> Result<Vec<ConstraintView>, String> {
    let all = factory(cell)?.store().constraints().await?;
    Ok(all
        .into_iter()
        .map(|c| ConstraintView {
            layer: name(bastion_core::govern::layer(&c)),
            id: c.id,
            rule: c.rule,
            function: c.function,
            contains: c.contains,
            rung: name(c.rung),
            source: name(c.source),
        })
        .collect())
}

/// Add an owner's constraint from the form.
pub async fn constraint_add(cell: &FactoryCell, constraint: ConstraintView) -> Result<(), String> {
    let rung = serde_json::from_value(json!(constraint.rung))
        .map_err(|_| format!("{:?} is not told, checked or enforced", constraint.rung))?;
    let c = Constraint {
        id: constraint.id.trim().into(),
        rule: constraint.rule.trim().into(),
        function: constraint.function.trim().into(),
        contains: constraint.contains.trim().into(),
        rung,
        source: Source::Owner,
    };
    factory(cell)?.gate().add_constraint(c).await
}

pub async fn constraint_move(cell: &FactoryCell, id: String, up: bool) -> Result<(), String> {
    factory(cell)?.gate().move_constraint(&id, up).await
}

pub async fn capabilities(cell: &FactoryCell) -> Result<Vec<CapabilityView>, String> {
    let all = factory(cell)?.store().capabilities().await?;
    Ok(all
        .into_iter()
        .map(|g| CapabilityView {
            stage: name(g.stage),
            function: g.function,
            rung: name(g.rung),
        })
        .collect())
}

pub async fn capability_move(
    cell: &FactoryCell,
    stage: String,
    function: String,
    up: bool,
) -> Result<(), String> {
    let stage =
        serde_json::from_value(json!(stage)).map_err(|_| format!("{stage:?} is not a stage"))?;
    factory(cell)?
        .gate()
        .move_capability(stage, &function, up)
        .await
}

/// The whole chain, verified under its key as it is read, and the writer's
/// state, so a stalled audit shows.
/// One page of the audit log, counted back from its newest entry and kept
/// in chain order. The whole chain is still read, as verifying it needs
/// every entry.
pub async fn audit(cell: &FactoryCell, page: super::Page) -> Result<AuditView, String> {
    let factory = factory(cell)?;
    let log = factory.gate().audit()?;
    let read = log.read().await?;
    let (pending, last_error) = log.status();
    let (limit, offset) = page.bounds();
    let newest = read.chain.len().saturating_sub(offset);
    let entries = read.chain[newest.saturating_sub(limit)..newest]
        .iter()
        .cloned()
        .map(|e| AuditEntryView {
            seq: e.seq,
            at: e.at,
            kind: e.record.kind,
            subject: e.record.subject,
            detail: e.record.detail,
            hash: e.hash,
        })
        .collect();
    Ok(AuditView {
        entries,
        broken_at: read.broken_at,
        pending,
        last_error,
    })
}

/// Write the chain as JSON Lines into `dir`, with a last line that says
/// whether it verified. Returns the file's path.
pub async fn audit_export(
    dir: &std::path::Path,
    cell: &FactoryCell,
    machine: &Arc<dyn Machine>,
) -> Result<String, String> {
    let read = factory(cell)?.gate().audit()?.read().await?;
    let mut lines: Vec<String> = read
        .chain
        .iter()
        .map(|e| serde_json::to_string(e).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    lines.push(
        json!({ "verified": read.broken_at.is_none(), "broken_at": read.broken_at }).to_string(),
    );
    machine.make_dir(dir)?;
    let path = dir.join(format!("audit-{}.jsonl", now()));
    machine.write_bytes(&path, (lines.join("\n") + "\n").as_bytes())?;
    Ok(path.display().to_string())
}

/// Each workspace's charter, read from its repository now.
pub async fn charters(
    cell: &FactoryCell,
    machine: &Arc<dyn Machine>,
) -> Result<Vec<WorkspaceCharterView>, String> {
    let workspaces = factory(cell)?.store().workspaces().await?;
    Ok(workspaces
        .into_iter()
        .map(|w| {
            let root = std::path::Path::new(&w.path);
            let c = crate::charter::read(&|name| machine.read(&root.join(name)).ok());
            let before = c
                .before
                .iter()
                .map(|h| format!("before {}: {}", h.functions.join(", "), h.command));
            let stop = c.stop.iter().map(|h| format!("when a build ends: {h}"));
            WorkspaceCharterView {
                workspace: w.name,
                files: c.files.clone(),
                rules: c.constraints.iter().map(|r| r.rule.clone()).collect(),
                hooks: before.chain(stop).collect(),
                allow: c.allow.clone(),
            }
        })
        .collect())
}

/// What the factory itself may do, as the settings set it.
pub async fn factory_capabilities(
    cell: &FactoryCell,
) -> Result<Vec<FactoryCapabilityView>, String> {
    let s = crate::settings_store::current(&**factory(cell)?.store()).await?;
    let row = |name: &str, rung: String, how: &str| FactoryCapabilityView {
        name: name.into(),
        rung,
        how: how.into(),
    };
    let github = s.forge == bastion_core::settings::Forge::Github;
    Ok(vec![
        row(
            "Approve a plan",
            name(s.dials.at(bastion_core::Stage::Plan)),
            "The plan step's dial.",
        ),
        row(
            "Approve a change",
            name(s.dials.at(bastion_core::Stage::Build)),
            "The build step's dial.",
        ),
        if github {
            row(
                "Push the job's branch and open a pull request",
                "allowed".into(),
                "After review, to the workspace's GitHub origin.",
            )
        } else {
            row(
                "Land on the workspace's branch",
                "allowed".into(),
                "After review, merged here.",
            )
        },
        row(
            "Merge a pull request",
            if github {
                "ask".into()
            } else {
                "denied".into()
            },
            "Approve at land, once CI passes.",
        ),
        row(
            "Start a fix turn when CI fails",
            if github {
                "allowed".into()
            } else {
                "denied".into()
            },
            "At most 2 per job, with the failing log.",
        ),
    ])
}
