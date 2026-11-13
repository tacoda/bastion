//! Commands for held calls, the two ladders and the audit.

use std::sync::Arc;

use bastion_core::govern::{Capability, Constraint, Enforcement, Source};
use bastion_protocol::{
    AuditEntryView, AuditView, CapabilityView, ConstraintView, FactoryCapabilityView, HeldCallRef,
    NodeView, WorkflowView, WorkspaceCharterView,
};
use serde_json::json;

use super::factory::{FactoryCell, factory, name};
use crate::factory::now;
use crate::gate::{Form, Scoped};
use crate::machine::Machine;
use bastion_protocol::api::{ConstraintForm, ScopeSet};

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
            hook: c.hook,
            reviewer: c.reviewer,
            capability: c.capability,
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
    let rung = mode(&constraint.rung)?;
    let c = Constraint {
        id: constraint.id.trim().into(),
        rule: constraint.rule.trim().into(),
        function: constraint.function.trim().into(),
        contains: constraint.contains.trim().into(),
        rung,
        source: Source::Owner,
        steps: steps_of(&constraint.steps),
        hook: constraint.hook.trim().into(),
        reviewer: constraint.reviewer.trim().into(),
        capability: constraint.capability,
    };
    factory(cell)?.gate().add_constraint(c).await
}

pub async fn constraint_remove(cell: &FactoryCell, id: String) -> Result<(), String> {
    factory(cell)?.gate().remove_constraint(&id).await
}

/// Give a constraint its deterministic form, and move it to a rung (D55).
pub async fn constraint_form(
    cell: &FactoryCell,
    id: String,
    f: ConstraintForm,
) -> Result<(), String> {
    let rung = f.rung.as_deref().map(mode).transpose()?;
    let form = Form {
        function: f.function,
        contains: f.contains,
        hook: f.hook,
    };
    factory(cell)?.gate().set_form(&id, form, rung).await
}

/// Set a constraint's rung: told, checked or enforced.
pub async fn constraint_rung(cell: &FactoryCell, id: String, rung: String) -> Result<(), String> {
    factory(cell)?
        .gate()
        .set_constraint_rung(&id, mode(&rung)?)
        .await
}

/// A constraint's mode (D56): feedforward, stored as told, or feedback,
/// stored as enforced. Checked gave way and is refused.
fn mode(word: &str) -> Result<Enforcement, String> {
    match word {
        "told" | "feedforward" => Ok(Enforcement::Told),
        "enforced" | "feedback" => Ok(Enforcement::Enforced),
        other => Err(format!("{other:?} is not feedforward or feedback")),
    }
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
            scopes: g.scopes,
        })
        .collect())
}

/// A grant from its view, its words read as a stage and a rung.
fn grant_of(v: &CapabilityView) -> Result<Capability, String> {
    let stage = serde_json::from_value(json!(v.stage))
        .map_err(|_| format!("{:?} is not a kind of step", v.stage))?;
    let rung = serde_json::from_value(json!(v.rung))
        .map_err(|_| format!("{:?} is not ask or allowed", v.rung))?;
    Ok(Capability {
        stage,
        step: String::new(),
        function: v.function.trim().into(),
        rung,
        scopes: v.scopes.clone(),
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

/// Grant a function to a kind, on at the scope the view's step names: a
/// workflow, `workflow/step`, or every workflow when empty (D54).
pub async fn capability_add(cell: &FactoryCell, grant: CapabilityView) -> Result<(), String> {
    let scope = grant.step.trim().to_string();
    factory(cell)?.gate().grant(grant_of(&grant)?, &scope).await
}

/// Set a constraint or a grant on, off, or inheriting at a scope (D54).
pub async fn scope_set(cell: &FactoryCell, set: ScopeSet) -> Result<(), String> {
    let on = match set.setting.as_str() {
        "on" => Some(true),
        "off" => Some(false),
        "inherit" => None,
        other => return Err(format!("{other:?} is not on, off or inherit")),
    };
    let gate = factory(cell)?.gate().clone();
    let stage;
    let target = match set.constraint.is_empty() {
        false => Scoped::Constraint(&set.constraint),
        true => {
            stage = serde_json::from_value(json!(set.stage))
                .map_err(|_| format!("{:?} is not a kind of step", set.stage))?;
            Scoped::Grant(stage, &set.function)
        }
    };
    gate.set_scope(target, set.scope.trim(), on).await
}

/// Take back a step's own grant.
pub async fn capability_remove(cell: &FactoryCell, grant: CapabilityView) -> Result<(), String> {
    factory(cell)?.gate().ungrant(&grant_of(&grant)?).await
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
                skills: machine.list(&root.join(".agents/skills")),
                agents: machine
                    .list(&root.join(".agents/agents"))
                    .into_iter()
                    .filter_map(|n| n.strip_suffix(".md").map(String::from))
                    .collect(),
                texts: super::charters::texts(machine.as_ref(), root),
            }
        })
        .collect())
}

/// What the factory itself may do (D39 to D42), from where each is set
/// (D58): how a journey lands and how often work is sent back, per region;
/// the network, per step.
pub async fn factory_capabilities(
    cell: &FactoryCell,
) -> Result<Vec<FactoryCapabilityView>, String> {
    let workspaces = factory(cell)?.store().workspaces().await?;
    let trails = super::workflows::workflows(cell).await?;
    let mut lines: Vec<FactoryCapabilityView> = workspaces
        .iter()
        .flat_map(|w| region_lines(w, &trails))
        .collect();
    lines.push(network_line(&trails));
    Ok(lines)
}

/// Each step that sets something itself, as `trail › step (what)`.
fn steps_setting(
    trails: &[WorkflowView],
    what: impl Fn(&NodeView) -> Option<String>,
) -> Vec<String> {
    let each = trails.iter().flat_map(|t| {
        let set = t
            .nodes
            .iter()
            .filter_map(|n| Some((n.id.clone(), what(n)?)));
        set.map(|(step, v)| format!("{} › {step}{v}", t.name))
            .collect::<Vec<_>>()
    });
    each.collect()
}

/// How journeys land in region `w`, and how often their work is sent back.
fn region_lines(
    w: &crate::store::Workspace,
    trails: &[WorkflowView],
) -> [FactoryCapabilityView; 2] {
    let land = match w.merges() {
        true => format!("Merges a finished journey's branch into {}.", w.branch),
        false => format!(
            "Leaves a finished journey on its branch, {}<id>, for its pull request.",
            w.branch_prefix
        ),
    };
    let budget = match w.revision_budget {
        Some(n) => format!("at most {n} times a journey"),
        None => "with no budget for the journey".into(),
    };
    let own = steps_setting(trails, |n| n.options.revisions.map(|r| format!(" ({r})")));
    let steps = match own.is_empty() {
        true => String::new(),
        false => format!(" Steps that set their own: {}.", own.join(", ")),
    };
    let back = format!(
        "{} times a step by default, {budget}.{steps}",
        bastion_core::MAX_REVISIONS
    );
    let line = |name: &str, how: String| FactoryCapabilityView {
        name: name.into(),
        rung: "allowed".into(),
        how,
        workspace: w.name.clone(),
    };
    [
        line("Land a finished journey", land),
        line("Send failing work back to be fixed", back),
    ]
}

/// Which steps reach the network: only those their trail marks.
fn network_line(trails: &[WorkflowView]) -> FactoryCapabilityView {
    let marked = steps_setting(trails, |n| n.reach.network.then(String::new));
    let how = match marked.is_empty() {
        true => "No step is marked for it.".to_string(),
        false => format!(
            "Only the steps their trail marks, each in a VM of its own: {}.",
            marked.join(", ")
        ),
    };
    FactoryCapabilityView {
        name: "Give a shell step the network".into(),
        rung: "ask".into(),
        how,
        workspace: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastion_protocol::{OptionsView, ReachView};

    fn trail() -> WorkflowView {
        let step = |id: &str| NodeView {
            id: id.into(),
            ..NodeView::default()
        };
        let build = NodeView {
            options: OptionsView {
                revisions: Some(4),
                ..OptionsView::default()
            },
            ..step("build")
        };
        let pr = NodeView {
            reach: ReachView {
                network: true,
                ..ReachView::default()
            },
            ..step("pr")
        };
        WorkflowView {
            id: "pull-request".into(),
            name: "Pull request".into(),
            nodes: vec![step("plan"), build, pr],
            ..WorkflowView::default()
        }
    }

    #[test]
    fn a_regions_factory_lines_come_from_its_settings_and_its_steps() {
        let mut w = crate::store::Workspace {
            name: "demo".into(),
            branch: "main".into(),
            branch_prefix: "ian/".into(),
            finish: "merge".into(),
            revision_budget: Some(6),
            ..crate::store::Workspace::default()
        };
        let [land, back] = region_lines(&w, &[trail()]);
        assert_eq!(land.workspace, "demo");
        assert_eq!(land.how, "Merges a finished journey's branch into main.");
        assert_eq!(
            back.how,
            "2 times a step by default, at most 6 times a journey. Steps that set their own: Pull request › build (4)."
        );
        w.finish = "leave".into();
        let [land, _] = region_lines(&w, &[]);
        assert!(land.how.contains("on its branch, ian/<id>"));
        let net = network_line(&[trail()]);
        assert_eq!(net.workspace, "");
        assert!(net.how.ends_with(": Pull request › pr."), "{}", net.how);
        assert_eq!(network_line(&[]).how, "No step is marked for it.");
    }
}
