//! What the tray's menu says, from a snapshot of the server: a pure model
//! the tray turns into a native menu, so it can be tested without one.

use bastion_protocol::JobSummary;

/// The most waiting jobs the menu lists; the rest are counted.
const LISTED: usize = 8;

/// What the server was doing when last read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Whether the server answered.
    pub up: bool,
    /// Jobs that wait on a person, newest first.
    pub waiting: Vec<JobSummary>,
    pub running: usize,
    pub intake_paused: bool,
}

/// What a menu entry does when picked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Approve(String),
    Intake { paused: bool },
    OpenApp,
    Quit,
}

/// One line of the menu: its text, and what it does, if anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub text: String,
    pub action: Option<Action>,
}

impl Entry {
    fn note(text: impl Into<String>) -> Self {
        Entry {
            text: text.into(),
            action: None,
        }
    }

    fn doing(text: impl Into<String>, action: Action) -> Self {
        Entry {
            text: text.into(),
            action: Some(action),
        }
    }
}

/// The count shown beside the icon: the jobs waiting, when any.
pub fn badge(s: &Snapshot) -> Option<String> {
    (!s.waiting.is_empty()).then(|| s.waiting.len().to_string())
}

/// The menu, top to bottom; `None` stands for a separator.
pub fn entries(s: &Snapshot) -> Vec<Option<Entry>> {
    let mut all = vec![Some(Entry::note(summary(s))), None];
    for job in s.waiting.iter().take(LISTED) {
        let label = format!(
            "Approve {} at {}: {}",
            job.id,
            job.stage,
            first_line(&job.request)
        );
        all.push(Some(Entry::doing(label, Action::Approve(job.id.clone()))));
    }
    if s.waiting.len() > LISTED {
        let more = s.waiting.len() - LISTED;
        all.push(Some(Entry::note(format!("{more} more in Bastion"))));
    }
    if !s.waiting.is_empty() {
        all.push(None);
    }
    if s.up {
        let (text, paused) = match s.intake_paused {
            true => ("Open intake", false),
            false => ("Pause intake", true),
        };
        all.push(Some(Entry::doing(text, Action::Intake { paused })));
    }
    all.push(Some(Entry::doing("Open Bastion", Action::OpenApp)));
    all.push(None);
    all.push(Some(Entry::doing(
        "Quit the tray (the server keeps running)",
        Action::Quit,
    )));
    all
}

fn summary(s: &Snapshot) -> String {
    if !s.up {
        return "The local Bastion server does not answer".into();
    }
    let intake = if s.intake_paused {
        " · intake paused"
    } else {
        ""
    };
    format!(
        "{} waiting · {} running{intake}",
        s.waiting.len(),
        s.running
    )
}

/// A request's first line, short enough for a menu.
fn first_line(request: &str) -> String {
    let line = request.lines().next().unwrap_or_default();
    match line.char_indices().nth(48) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waiting(id: &str) -> JobSummary {
        JobSummary {
            id: id.into(),
            stage: "plan".into(),
            state: "needs_you".into(),
            request: "Add a health check\nwith a test".into(),
            ..JobSummary::default()
        }
    }

    fn texts(s: &Snapshot) -> Vec<String> {
        let line = |e: Option<Entry>| e.map_or("-".into(), |e| e.text);
        entries(s).into_iter().map(line).collect()
    }

    #[test]
    fn the_menu_lists_each_waiting_job_to_approve_and_the_intake() {
        let s = Snapshot {
            up: true,
            waiting: vec![waiting("j1")],
            running: 2,
            intake_paused: false,
        };
        assert_eq!(
            texts(&s),
            [
                "1 waiting · 2 running",
                "-",
                "Approve j1 at plan: Add a health check",
                "-",
                "Pause intake",
                "Open Bastion",
                "-",
                "Quit the tray (the server keeps running)",
            ]
        );
        assert_eq!(
            entries(&s)[2].clone().unwrap().action,
            Some(Action::Approve("j1".into()))
        );
        assert_eq!(badge(&s).as_deref(), Some("1"));
    }

    #[test]
    fn a_paused_or_silent_server_says_so_and_many_jobs_are_counted() {
        let paused = Snapshot {
            up: true,
            intake_paused: true,
            ..Snapshot::default()
        };
        assert_eq!(texts(&paused)[0], "0 waiting · 0 running · intake paused");
        assert!(texts(&paused).contains(&"Open intake".to_string()));
        assert_eq!(badge(&paused), None);
        let down = texts(&Snapshot::default());
        assert_eq!(down[0], "The local Bastion server does not answer");
        assert!(!down.iter().any(|t| t.contains("intake")));
        let many = Snapshot {
            up: true,
            waiting: (0..10).map(|n| waiting(&format!("j{n}"))).collect(),
            ..Snapshot::default()
        };
        assert!(texts(&many).contains(&"2 more in Bastion".to_string()));
    }

    #[test]
    fn a_long_request_is_cut_for_the_menu() {
        let long = "x".repeat(60);
        assert_eq!(first_line(&long).chars().count(), 49);
    }
}
