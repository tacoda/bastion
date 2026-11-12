//! Field notes (D61): what a journey taught, one sentence signed by the
//! person who wrote it. A lesson or a fact about the region becomes a
//! charter change under `.agents/`, committed on a branch of its own for a
//! person to merge, so the repository stays the one source of truth. A
//! note about Bastion stays here, for the learning loop on the Guide.
//! Every note is also an audit entry, so the chain signs it.

use std::sync::Arc;

use bastion_core::State;
use bastion_core::audit::{Entry, Record};
use bastion_core::corpus;
use bastion_protocol::{CharterChange, FieldNoteView};

use super::factory::{FactoryCell, factory};
use crate::machine::Machine;

/// The audit kind a field note is kept under.
pub const NOTE: &str = "field-note";

/// The longest a note may be: a sentence or two, not an essay.
const LONGEST: usize = 600;

/// What a note may be: a lesson or a fact for the region, or about Bastion.
const ABOUT: [&str; 3] = ["lesson", "fact", "bastion"];

/// A note entry's detail is `<by>\t<about>\t<text>`, then `\t<branch>`
/// when it became a charter change.
fn view(e: &Entry) -> FieldNoteView {
    let mut parts = e.record.detail.splitn(4, '\t').map(String::from);
    let mut next = || parts.next().unwrap_or_default();
    FieldNoteView {
        job: e.record.subject.clone(),
        by: next(),
        about: next(),
        text: next(),
        branch: next(),
        at: e.at,
        ..FieldNoteView::default()
    }
}

/// What a person wrote, as a note may keep it, or why it may not.
fn kept(text: &str) -> Result<String, String> {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match one_line.chars().count() {
        0 => Err("write what the journey taught".into()),
        n if n > LONGEST => Err(format!("a field note is at most {LONGEST} characters")),
        _ => Ok(one_line),
    }
}

/// What a person wrote and what kind of note it is, for one journey.
pub struct Written {
    pub about: String,
    pub text: String,
    pub by: String,
}

/// The charter file a lesson or a fact becomes: a rule, or a corpus entry
/// whose paths come from the journey's change and the note itself.
fn charter_file(w: &Written, text: &str, job: &str, diff: &str) -> (String, String) {
    let signed = format!("{text}\n\nLearned on journey {job}, signed by {}.\n", w.by);
    let fact = corpus::Fact { note: text, diff };
    let name = fact.slug();
    match w.about.as_str() {
        "lesson" => (format!(".agents/rules/{name}.md"), signed),
        _ => {
            let paths = fact.paths().join(", ");
            let file = format!("---\npaths: {paths}\n---\n{signed}");
            (format!("{}/{name}.md", corpus::DIR), file)
        }
    }
}

/// Sign a field note on journey `id`. A lesson or a fact is committed to
/// the region's charter on a branch of its own.
pub async fn note(
    cell: &FactoryCell,
    machine: Arc<dyn Machine>,
    id: String,
    w: Written,
) -> Result<(), String> {
    let factory = factory(cell)?;
    let text = kept(&w.text)?;
    if !ABOUT.contains(&w.about.as_str()) {
        return Err(format!("{:?} is not a lesson, a fact or bastion", w.about));
    }
    let record = factory.store().load(&id).await?;
    let mut branch = String::new();
    if w.about != "bastion" {
        let artifacts = factory.store().artifacts(&id).await?;
        let change = artifacts
            .iter()
            .rev()
            .find(|a| a.body.starts_with("diff --git"));
        let diff = change.map_or("", |a| a.body.as_str());
        let (path, file) = charter_file(&w, &text, &id, diff);
        let change = CharterChange {
            workspace: record.job.workspace.clone(),
            path,
            text: file,
        };
        branch = super::charters::edit(cell, machine.as_ref(), change).await?;
    }
    factory.gate().audit()?.note(Record {
        kind: NOTE.into(),
        subject: id,
        detail: format!("{}\t{}\t{text}\t{branch}", w.by, w.about),
    });
    Ok(())
}

/// The notes on job `id`, oldest first.
pub async fn notes_of(cell: &FactoryCell, id: &str) -> Result<Vec<FieldNoteView>, String> {
    let entries = factory(cell)?.store().entries_of(NOTE).await?;
    Ok(entries
        .iter()
        .filter(|e| e.record.subject == id)
        .map(view)
        .collect())
}

/// Every note on the server, newest first, each with its job's request.
pub async fn atlas(cell: &FactoryCell) -> Result<Vec<FieldNoteView>, String> {
    let factory = factory(cell)?;
    let mut notes: Vec<FieldNoteView> = factory
        .store()
        .entries_of(NOTE)
        .await?
        .iter()
        .map(view)
        .collect();
    for n in &mut notes {
        if let Ok(r) = factory.store().load(&n.job).await {
            n.request = r.job.request;
            n.workspace = r.job.workspace;
            n.landed = r.job.state == State::Landed;
            n.workflow = r.runtime.workflow.unwrap_or_default();
        }
    }
    notes.sort_by_key(|n| std::cmp::Reverse(n.at));
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_is_one_line_and_short_and_reads_back_with_its_author() {
        assert_eq!(
            kept("  the test\n named the bug  ").unwrap(),
            "the test named the bug"
        );
        assert!(kept("   ").is_err());
        assert!(kept(&"x".repeat(LONGEST + 1)).is_err());
        let record = Record {
            kind: NOTE.into(),
            subject: "j1".into(),
            detail: "Ian Johnson\tfact\tthe clock is UTC\tian/charter-7".into(),
        };
        let v = view(&bastion_core::audit::append(None, 7, record, b"k"));
        let read = (
            v.by.as_str(),
            v.about.as_str(),
            v.text.as_str(),
            v.branch.as_str(),
        );
        assert_eq!(
            read,
            ("Ian Johnson", "fact", "the clock is UTC", "ian/charter-7")
        );
    }

    #[test]
    fn a_lesson_is_a_rule_and_a_fact_a_corpus_entry_with_inferred_paths() {
        let w = |about: &str| Written {
            about: about.into(),
            text: String::new(),
            by: "Ian".into(),
        };
        let diff = "diff --git a/src/calc.py b/src/calc.py\n";
        let (path, file) = charter_file(&w("lesson"), "Keep calc small.", "j1", diff);
        assert_eq!(path, ".agents/rules/keep-calc-small.md");
        assert!(file.starts_with("Keep calc small.") && file.contains("journey j1, signed by Ian"));
        let (path, file) = charter_file(&w("fact"), "Rounding is half to even.", "j1", diff);
        assert_eq!(path, ".agents/corpus/rounding-is-half-to-even.md");
        assert!(file.starts_with("---\npaths: src/**\n---\n"), "{file}");
    }
}
