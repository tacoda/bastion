//! Field notes and the atlas (D59): what a job taught, one sentence signed
//! by the person who wrote it. A note is an audit entry, so the chain signs
//! it and nothing rewrites it. All of a server's notes are its atlas, where
//! the next request starts.

use bastion_core::State;
use bastion_core::audit::{Entry, Record};
use bastion_protocol::FieldNoteView;

use super::factory::{FactoryCell, factory};

/// The audit kind a field note is kept under.
pub const NOTE: &str = "field-note";

/// The longest a note may be: a sentence or two, not an essay.
const LONGEST: usize = 600;

/// What a note may be about: the job's workspace, or Bastion itself.
const ABOUT: [&str; 2] = ["workspace", "bastion"];

/// A note entry's detail is `<by>\t<about>\t<text>`.
fn view(e: &Entry) -> FieldNoteView {
    let mut parts = e.record.detail.splitn(3, '\t');
    let (by, about, text) = (parts.next(), parts.next(), parts.next());
    FieldNoteView {
        job: e.record.subject.clone(),
        by: by.unwrap_or_default().into(),
        about: about.unwrap_or_default().into(),
        text: text.unwrap_or_default().into(),
        at: e.at,
        ..FieldNoteView::default()
    }
}

/// What a person wrote, as a note may keep it, or why it may not.
fn kept(text: &str) -> Result<String, String> {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match one_line.chars().count() {
        0 => Err("write what the job taught".into()),
        n if n > LONGEST => Err(format!("a field note is at most {LONGEST} characters")),
        _ => Ok(one_line),
    }
}

/// What a person wrote and whom it is for, for one job.
pub struct Written {
    pub about: String,
    pub text: String,
    pub by: String,
}

/// Sign a field note on job `id`.
pub async fn note(cell: &FactoryCell, id: String, w: Written) -> Result<(), String> {
    let factory = factory(cell)?;
    let text = kept(&w.text)?;
    if !ABOUT.contains(&w.about.as_str()) {
        return Err(format!("{:?} is not workspace or bastion", w.about));
    }
    factory.store().load(&id).await?;
    factory.gate().audit()?.note(Record {
        kind: NOTE.into(),
        subject: id,
        detail: format!("{}\t{}\t{text}", w.by, w.about),
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
            detail: "Ian Johnson\tworkspace\tthe flaky test was the clock".into(),
        };
        let e = bastion_core::audit::append(None, 7, record, b"k");
        let v = view(&e);
        let read = (v.by.as_str(), v.about.as_str(), v.text.as_str(), v.at);
        assert_eq!(
            read,
            (
                "Ian Johnson",
                "workspace",
                "the flaky test was the clock",
                7
            )
        );
    }
}
