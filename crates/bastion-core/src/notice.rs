//! Notices: a job that comes to wait on a person, fails, or lands is
//! worth telling someone about. Each other change only refreshes a page.

use crate::{Job, State};

/// The state a job just entered, when it is one a person hears about.
pub fn entered(before: &Job, after: &Job) -> Option<State> {
    let worth = matches!(after.state, State::NeedsYou | State::Failed | State::Landed);
    (worth && (before.state, before.stage) != (after.state, after.stage)).then_some(after.state)
}

/// One line about the job as it now stands, for a notification or a chat.
pub fn text(job: &Job) -> String {
    let request = job.request.lines().next().unwrap_or_default();
    let what: String = request.chars().take(80).collect();
    let stage = format!("{:?}", job.stage).to_lowercase();
    match job.state {
        State::NeedsYou => format!("{} waits for you at {stage}: {what}", job.id),
        State::Failed => {
            let why = job
                .error
                .as_deref()
                .and_then(|e| e.lines().next())
                .unwrap_or("no reason given");
            format!("{} failed at {stage}: {why}", job.id)
        }
        State::Landed => format!("{} landed: {what}", job.id),
        _ => format!("{} is {:?} at {stage}", job.id, job.state),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Stage;

    fn at(stage: Stage, state: State) -> Job {
        let mut j = Job::new(
            "j1".into(),
            "w".into(),
            "Add a health check\nwith a test".into(),
            1,
        );
        (j.stage, j.state) = (stage, state);
        j
    }

    #[test]
    fn a_job_that_comes_to_wait_fails_or_lands_is_a_notice_once() {
        let running = at(Stage::Plan, State::Running);
        let waits = at(Stage::Plan, State::NeedsYou);
        assert_eq!(entered(&running, &waits), Some(State::NeedsYou));
        assert_eq!(entered(&waits, &waits), None, "still waiting is not news");
        let next_wait = at(Stage::Build, State::NeedsYou);
        assert_eq!(
            entered(&waits, &next_wait),
            Some(State::NeedsYou),
            "a new step's wait is"
        );
        assert_eq!(entered(&waits, &at(Stage::Build, State::Running)), None);
        assert_eq!(
            entered(&running, &at(Stage::Land, State::Landed)),
            Some(State::Landed)
        );
    }

    #[test]
    fn a_notice_says_what_happened_in_one_line() {
        assert_eq!(
            text(&at(Stage::Plan, State::NeedsYou)),
            "j1 waits for you at plan: Add a health check"
        );
        let mut failed = at(Stage::Build, State::Failed);
        failed.error = Some("the turn ran past 30 minutes\nmore".into());
        assert_eq!(
            text(&failed),
            "j1 failed at build: the turn ran past 30 minutes"
        );
        assert_eq!(
            text(&at(Stage::Land, State::Landed)),
            "j1 landed: Add a health check"
        );
    }
}
