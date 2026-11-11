//! Commands for held calls, the two ladders and the audit.

use std::sync::Arc;

use bastion_core::govern::{Capability, Constraint, Source};
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
            steps: c.steps,
        })
        .collect())
}

/// The steps a form names, trimmed, without the empty ones.
fn steps_of(typed: &[String]) -> Vec<String> {
    let kept = typed.iter().map(|s| s.trim().to_string());
    kept.filter(|s| !s.is_empty()).collect()
}

/// Set the workflow steps a constraint applies at (D44).
pub async fn constraint_steps(
    cell: &FactoryCell,
    id: String,
    steps: Vec<String>,
) -> Result<(), String> {
    factory(cell)?
        .gate()
        .scope_constraint(&id, steps_of(&steps))
        .await
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
        steps: steps_of(&constraint.steps),
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
            step: g.step,
            function: g.function,
            rung: name(g.rung),
        })
        .collect())
}

/// A grant from its view, its words read as a stage and a rung.
fn grant_of(v: &CapabilityView) -> Result<Capability, String> {
    let stage = serde_json::from_value(json!(v.stage))
        .map_err(|_| format!("{:?} is not a kind of step", v.stage))?;
    let rung = serde_json::from_value(json!(v.rung))
        .map_err(|_| format!("{:?} is not denied, ask or allowed", v.rung))?;
    Ok(Capability {
        stage,
        step: v.step.trim().into(),
        function: v.function.trim().into(),
        rung,
    })
}

/// Move the grant `g` names, its kind's or a step's own, up or down its
/// ladder; its rung is not read.
pub async fn capability_move(
    cell: &FactoryCell,
    g: CapabilityView,
    up: bool,
) -> Result<(), String> {
    let stage = serde_json::from_value(json!(g.stage))
        .map_err(|_| format!("{:?} is not a stage", g.stage))?;
    let at = bastion_core::govern::At {
        stage,
        step: &g.step,
    };
    factory(cell)?
        .gate()
        .move_capability(at, &g.function, up)
        .await
}

/// Grant a function at one workflow step (D44).
pub async fn capability_add(cell: &FactoryCell, grant: CapabilityView) -> Result<(), String> {
    factory(cell)?.gate().grant_at_step(grant_of(&grant)?).await
}

/// Take back a step's own grant.
pub async fn capability_remove(cell: &FactoryCell, grant: CapabilityView) -> Result<(), String> {
    factory(cell)?
        .gate()
        .ungrant_at_step(&grant_of(&grant)?)
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

/// What the factory itself may do (D39 to D42). Each is the workflow's or
/// the workspace's to say, so none is a setting here.
pub async fn factory_capabilities(
    cell: &FactoryCell,
) -> Result<Vec<FactoryCapabilityView>, String> {
    factory(cell)?;
    let row = |name: &str, rung: &str, how: &str| FactoryCapabilityView {
        name: name.into(),
        rung: rung.into(),
        how: how.into(),
    };
    Ok(vec![
        row(
            "Merge a finished job's branch",
            "allowed",
            "As its workspace says: merge into the base, or leave the branch.",
        ),
        row(
            "Give a shell step the network",
            "ask",
            "Only a step its workflow marks, in a VM of its own, with the secrets it names.",
        ),
        row(
            "Send failing work back to be fixed",
            "allowed",
            "As often as each step allows, 2 times by default, within the workspace's revision budget.",
        ),
    ])
}
