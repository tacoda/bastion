//! What a learn turn proposes (F6): each proposal a constraint for the gate
//! or a gate for the workspace, with why and the evidence from the job. A
//! person accepts or declines each one.

use serde::{Deserialize, Serialize};

use crate::spec::fenced;

/// The reply that proposes nothing.
pub const NOTHING: &str = "NOTHING TO LEARN";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    /// A rule the gate holds every call to.
    Constraint {
        rule: String,
        function: String,
        contains: String,
    },
    /// A check over the changed files before a land, `name: command`.
    Gate { name: String, command: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    #[serde(flatten)]
    pub change: Change,
    pub why: String,
    /// What in the job shows the need: a call, an error, a line of output.
    pub evidence: String,
}

#[derive(Deserialize)]
struct Reply {
    proposals: Vec<Proposal>,
}

/// Read a learn turn's reply: NOTHING TO LEARN, or one JSON object of
/// proposals, fenced or bare, each with its evidence.
pub fn read(report: &str) -> Result<Vec<Proposal>, String> {
    if report.trim() == NOTHING {
        return Ok(Vec::new());
    }
    let body = fenced(report).unwrap_or(report);
    let reply: Reply = serde_json::from_str(body.trim())
        .map_err(|e| format!("The reply was not the JSON asked for: {e}."))?;
    for (n, p) in reply.proposals.iter().enumerate() {
        problem(p).map_or(Ok(()), |why| Err(format!("Proposal {}: {why}", n + 1)))?;
    }
    Ok(reply.proposals)
}

/// What makes a proposal one a person cannot accept as it is.
fn problem(p: &Proposal) -> Option<&'static str> {
    let blank = |s: &str| s.trim().is_empty();
    if blank(&p.evidence) {
        return Some("quote the evidence from the job.");
    }
    match &p.change {
        Change::Constraint { rule, .. } if blank(rule) => Some("a constraint needs its rule."),
        Change::Gate { name, command } if blank(name) || blank(command) => {
            Some("a gate needs a name and a command.")
        }
        Change::Gate { name, .. } if name.contains(':') => Some("a gate's name has no colon."),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_proposes_constraints_and_gates_with_evidence() {
        let reply = r#"Here:
```json
{"proposals": [
  {"kind": "constraint", "rule": "Do not force-push", "function": "sandbox::exec",
   "contains": "git push --force", "why": "it lost work", "evidence": "git push --force origin"},
  {"kind": "gate", "name": "lint", "command": "ruff {files}", "why": "style slipped",
   "evidence": "E501 line too long"}
]}
```"#;
        let got = read(reply).unwrap();
        assert_eq!(got.len(), 2);
        assert!(matches!(&got[1].change, Change::Gate { name, .. } if name == "lint"));
        assert_eq!(read("NOTHING TO LEARN\n").unwrap(), []);
    }

    #[test]
    fn a_proposal_without_evidence_or_its_parts_is_refused() {
        let one = |p: &str| read(&format!(r#"{{"proposals": [{p}]}}"#));
        let bare =
            r#"{"kind": "gate", "name": "lint", "command": "ruff", "why": "w", "evidence": " "}"#;
        assert!(one(bare).unwrap_err().contains("evidence"));
        let no_rule = r#"{"kind": "constraint", "rule": "", "function": "", "contains": "", "why": "w", "evidence": "e"}"#;
        assert!(one(no_rule).unwrap_err().contains("rule"));
        let colon =
            r#"{"kind": "gate", "name": "a:b", "command": "x", "why": "w", "evidence": "e"}"#;
        assert!(one(colon).unwrap_err().contains("colon"));
        assert!(read("I learned a lot").is_err());
    }
}
