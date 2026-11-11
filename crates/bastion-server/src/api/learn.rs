//! What learn turns proposed, and a person's decision on each (F6). An
//! accepted constraint joins the gate's rules as told; an accepted gate
//! joins its workspace's gates. Each decision is an audit entry.

use std::collections::HashMap;

use bastion_core::audit::Record;
use bastion_core::govern::{Constraint, Enforcement, Source};
use bastion_protocol::{ProposalDecision, ProposalView};

use super::factory::{FactoryCell, factory};
use crate::factory::Factory;
use crate::learn::{self, Change, Proposal};

/// The audit kind a decision is kept under.
const KIND: &str = "learn";

// ponytail: every learn reply and decision is read per call; a table of
// proposals when a server keeps thousands of jobs.
pub async fn proposals(cell: &FactoryCell) -> Result<Vec<ProposalView>, String> {
    let factory = factory(cell)?;
    let decided = decisions(&factory).await?;
    let mut all = Vec::new();
    for (job, workspace, body) in factory.store().learned().await? {
        // A reply the turn could not read was sent back, so skip it here.
        let Ok(proposals) = learn::read(&body) else {
            continue;
        };
        for (i, p) in proposals.into_iter().enumerate() {
            let n = i as u32 + 1;
            all.push(ProposalView {
                decision: decided.get(&(job.clone(), n)).cloned().unwrap_or_default(),
                job: job.clone(),
                workspace: workspace.clone(),
                n,
                ..view(p)
            });
        }
    }
    Ok(all)
}

/// The decision on each `(job, n)`: the newest audit entry for it.
async fn decisions(factory: &Factory) -> Result<HashMap<(String, u32), String>, String> {
    let entries = factory.store().entries_of(KIND).await?;
    Ok(entries
        .into_iter()
        .filter_map(|e| {
            let (decision, n) = e.record.detail.split_once(' ')?;
            Some(((e.record.subject, n.parse().ok()?), decision.to_string()))
        })
        .collect())
}

/// A proposal as the page reads it; the caller says whose and which.
fn view(p: Proposal) -> ProposalView {
    let (kind, change, binds) = match p.change {
        Change::Constraint {
            rule,
            function,
            contains,
        } => ("constraint", rule, format!("{function} {contains}")),
        Change::Gate { name, command } => ("gate", format!("{name}: {command}"), String::new()),
    };
    ProposalView {
        kind: kind.into(),
        change,
        binds: binds.trim().into(),
        why: p.why,
        evidence: p.evidence,
        ..ProposalView::default()
    }
}

/// Accept or decline one proposal. Accepting twice changes nothing more:
/// the constraint's id is the proposal's, and a gate already listed stays once.
pub async fn decide(cell: &FactoryCell, d: ProposalDecision) -> Result<(), String> {
    let factory = factory(cell)?;
    let found = factory.store().learned().await?;
    let (_, workspace, body) = found
        .into_iter()
        .find(|(job, ..)| *job == d.job)
        .ok_or_else(|| format!("job {} proposed nothing", d.job))?;
    let proposal = learn::read(&body)?
        .into_iter()
        .nth((d.n as usize).wrapping_sub(1))
        .ok_or_else(|| format!("job {} has no proposal {}", d.job, d.n))?;
    if d.accept {
        accept(&factory, &d, &workspace, proposal.change).await?;
    }
    let decision = if d.accept { "accepted" } else { "declined" };
    factory.gate().audit()?.note(Record {
        kind: KIND.into(),
        subject: d.job,
        detail: format!("{decision} {}", d.n),
    });
    Ok(())
}

async fn accept(
    factory: &Factory,
    d: &ProposalDecision,
    workspace: &str,
    change: Change,
) -> Result<(), String> {
    match change {
        Change::Constraint {
            rule,
            function,
            contains,
        } => {
            let c = Constraint {
                id: format!("learn-{}-{}", d.job, d.n),
                rule,
                function,
                contains,
                rung: Enforcement::Told,
                source: Source::Owner,
            };
            factory.gate().add_constraint(c).await
        }
        Change::Gate { name, command } => {
            add_gate(factory, workspace, &format!("{name}: {command}")).await
        }
    }
}

async fn add_gate(factory: &Factory, workspace: &str, line: &str) -> Result<(), String> {
    let all = factory.store().workspaces().await?;
    let mut row = all
        .into_iter()
        .find(|w| w.name == workspace)
        .ok_or_else(|| format!("no workspace named {workspace}"))?;
    if row.gates.lines().any(|l| l.trim() == line) {
        return Ok(());
    }
    row.gates = with_line(&row.gates, line);
    factory.store().edit_workspace(&row).await
}

fn with_line(lines: &str, line: &str) -> String {
    match lines.trim_end() {
        "" => line.into(),
        kept => format!("{kept}\n{line}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gate_is_added_as_one_more_line() {
        assert_eq!(with_line("", "lint: ruff"), "lint: ruff");
        assert_eq!(with_line("a: b\n", "lint: ruff"), "a: b\nlint: ruff");
    }

    #[test]
    fn a_proposal_reads_as_its_change_and_what_it_binds() {
        let p = Proposal {
            change: Change::Constraint {
                rule: "No force-push".into(),
                function: "sandbox::exec".into(),
                contains: "push --force".into(),
            },
            why: "w".into(),
            evidence: "e".into(),
        };
        let v = view(p);
        assert_eq!(
            (v.kind.as_str(), v.binds.as_str()),
            ("constraint", "sandbox::exec push --force")
        );
    }
}
