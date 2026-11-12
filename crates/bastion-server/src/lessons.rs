//! What a field note becomes (D61): a lesson (a rule file, or a hook or a
//! permission in the settings) or a fact (a corpus entry), as files under
//! `.agents/`. The API signs notes; the factory writes the ones signed
//! while a journey was under way onto its branch when it lands.

use bastion_core::audit::Entry;
use bastion_core::corpus;

use crate::charter::{Codified, SETTINGS, with_lesson};

/// The audit kind a field note is kept under.
pub const NOTE: &str = "field-note";

/// The branch a note names while it waits for its journey to land: it is
/// then written onto the journey's own branch.
pub const ON_LANDING: &str = "on landing";

/// The longest a note may be: a sentence or two, not an essay.
const LONGEST: usize = 600;

/// What a note may be: a lesson or a fact for the region, or about Bastion.
pub const ABOUT: [&str; 3] = ["lesson", "fact", "bastion"];

/// What a person wrote and what kind of note it is, for one journey.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Written {
    pub about: String,
    pub text: String,
    pub by: String,
    /// A lesson's shape: `rule`, `hook`, `allow`, `ask` or `deny`.
    pub shape: String,
    /// A hook's command, or a permission's rule.
    pub detail: String,
}

/// A signed note as the audit keeps it: what was written, and the branch
/// its charter change is on, or [`ON_LANDING`], or nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signed {
    pub job: String,
    pub at: u64,
    pub written: Written,
    pub branch: String,
}

impl Written {
    /// The note as kept: its words on one line, or why it may not be kept.
    pub fn kept(&self) -> Result<Written, String> {
        let text = self.text.split_whitespace().collect::<Vec<_>>().join(" ");
        match text.chars().count() {
            0 => return Err("write what the journey taught".into()),
            n if n > LONGEST => {
                return Err(format!("a field note is at most {LONGEST} characters"));
            }
            _ => {}
        }
        if !ABOUT.contains(&self.about.as_str()) {
            return Err(format!(
                "{:?} is not a lesson, a fact or bastion",
                self.about
            ));
        }
        if self.codified().is_some() && self.detail.trim().is_empty() {
            return Err("say the hook's command or the permission's rule".into());
        }
        Ok(Written {
            text,
            detail: self.detail.trim().into(),
            ..self.clone()
        })
    }

    /// The lesson codified in the settings, when it is not a rule file.
    pub fn codified(&self) -> Option<Codified> {
        let detail = self.detail.trim().to_string();
        match (self.about.as_str(), self.shape.as_str()) {
            ("lesson", "hook") => Some(Codified::Hook(detail)),
            ("lesson", kind @ ("allow" | "ask" | "deny")) => Some(Codified::Permission {
                kind: kind.into(),
                rule: detail,
            }),
            _ => None,
        }
    }

    /// The audit entry's detail: `<by>\t<about>\t<text>\t<branch>\t<shape>\t<detail>`.
    pub fn detail_on(&self, branch: &str) -> String {
        let w = self;
        format!(
            "{}\t{}\t{}\t{branch}\t{}\t{}",
            w.by, w.about, w.text, w.shape, w.detail
        )
    }
}

/// A note from its audit entry.
pub fn signed(e: &Entry) -> Signed {
    let mut parts = e.record.detail.splitn(6, '\t').map(String::from);
    let mut next = || parts.next().unwrap_or_default();
    let (by, about, text, branch) = (next(), next(), next(), next());
    Signed {
        job: e.record.subject.clone(),
        at: e.at,
        written: Written {
            about,
            text,
            by,
            shape: next(),
            detail: next(),
        },
        branch,
    }
}

/// The file a rule or a fact becomes: a rule file, or a corpus entry whose
/// paths come from the journey's change and the note itself.
pub fn charter_file(w: &Written, job: &str, diff: &str) -> (String, String) {
    let signed = format!(
        "{}\n\nLearned on journey {job}, signed by {}.\n",
        w.text, w.by
    );
    let fact = corpus::Fact {
        note: &w.text,
        diff,
    };
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

/// What a journey and its notes need to become files.
pub struct Landing<'a> {
    pub job: &'a str,
    /// The journey's change, as a unified diff.
    pub diff: &'a str,
    /// The region's settings as they stand, if it has any.
    pub settings: Option<String>,
}

/// The files the notes become, each a path and its text. Lessons for the
/// settings fold into one settings file, in the order they were signed.
pub fn files(notes: &[Written], at: Landing) -> Result<Vec<(String, String)>, String> {
    let mut settings = at.settings;
    let mut out = Vec::new();
    for w in notes.iter().filter(|w| w.about != "bastion") {
        match w.codified() {
            Some(lesson) => settings = Some(with_lesson(settings.as_deref(), &lesson)?),
            None => out.push(charter_file(w, at.job, at.diff)),
        }
    }
    let touched = notes.iter().any(|w| w.codified().is_some());
    out.extend(
        settings
            .filter(|_| touched)
            .map(|s| (SETTINGS.to_string(), s)),
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastion_core::audit::{Record, append};

    fn written(about: &str, shape: &str, detail: &str) -> Written {
        Written {
            about: about.into(),
            text: "Keep calc small.".into(),
            by: "Ian".into(),
            shape: shape.into(),
            detail: detail.into(),
        }
    }

    #[test]
    fn a_note_reads_back_from_its_entry_and_keeps_only_what_it_may() {
        let w = written("lesson", "deny", "Bash(rm:*)");
        let record = Record {
            kind: NOTE.into(),
            subject: "j1".into(),
            detail: w.detail_on(ON_LANDING),
        };
        let s = signed(&append(None, 7, record, b"k"));
        assert_eq!(
            (s.job.as_str(), s.at, s.branch.as_str()),
            ("j1", 7, ON_LANDING)
        );
        assert_eq!(s.written, w);
        let spaced = Written {
            text: "  the test\n named the bug ".into(),
            ..written("fact", "", "")
        };
        assert_eq!(spaced.kept().unwrap().text, "the test named the bug");
        assert!(
            written("lesson", "hook", " ").kept().is_err(),
            "a hook names its command"
        );
        assert!(written("maybe", "", "").kept().is_err());
    }

    #[test]
    fn notes_become_a_rule_a_fact_and_one_settings_file() {
        let notes = [
            written("lesson", "rule", ""),
            written("fact", "", ""),
            written("lesson", "deny", "Bash(rm:*)"),
            written("lesson", "hook", "./lint.sh"),
            written("bastion", "", ""),
        ];
        let at = Landing {
            job: "j1",
            diff: "diff --git a/src/calc.py b/src/calc.py\n",
            settings: Some(r#"{"model":"x"}"#.into()),
        };
        let out = files(&notes, at).unwrap();
        let paths: Vec<&str> = out.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            paths,
            [
                ".agents/rules/keep-calc-small.md",
                ".agents/corpus/keep-calc-small.md",
                SETTINGS
            ]
        );
        let settings = &out[2].1;
        assert!(
            settings.contains("\"model\"")
                && settings.contains("Bash(rm:*)")
                && settings.contains("./lint.sh")
        );
        assert!(out[1].1.starts_with("---\npaths: src/**\n---\n"));
    }
}
