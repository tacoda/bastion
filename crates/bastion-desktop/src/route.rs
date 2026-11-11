//! Where the window is, and the sidebar's pages.

use crate::icons;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Jobs,
    NewRequest,
    Job(String),
    Constraints,
    Capabilities,
    Audit,
    Evals,
    Eval(String),
    Costs,
    Learn,
    Verification,
    Session(String),
    Engine,
    Workflows,
    Settings,
}

/// The sidebar's pages. A job and New request belong to Jobs.
pub const PAGES: [(Route, &str, &str); 11] = [
    (Route::Jobs, "Jobs", icons::JOBS),
    (Route::Workflows, "Workflows", icons::WORKFLOWS),
    (Route::Constraints, "Constraints", icons::CONSTRAINTS),
    (Route::Capabilities, "Capabilities", icons::CAPABILITIES),
    (Route::Audit, "Audit Log", icons::AUDIT),
    (Route::Evals, "Evals", icons::EVALS),
    (Route::Costs, "Costs", icons::COSTS),
    (Route::Learn, "Learning", icons::LEARN),
    (Route::Verification, "Verification", icons::VERIFICATION),
    (Route::Engine, "Engine", icons::ENGINE),
    (Route::Settings, "Settings", icons::SETTINGS),
];

impl Route {
    /// The sidebar entry this page sits under.
    pub fn section(&self) -> Route {
        match self {
            Route::NewRequest | Route::Job(_) => Route::Jobs,
            Route::Eval(_) => Route::Evals,
            Route::Session(_) => Route::Costs,
            other => other.clone(),
        }
    }

    /// A page about one Bastion server shows the server picker (D30).
    pub fn about_one_server(&self) -> bool {
        !matches!(self, Route::Jobs | Route::NewRequest | Route::Job(_))
    }

    pub fn title(&self) -> &'static str {
        let section = self.section();
        PAGES
            .iter()
            .find(|(r, _, _)| *r == section)
            .map_or("", |(_, title, _)| title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_sits_under_jobs_and_shows_no_server_picker() {
        let job = Route::Job("j1".into());
        assert_eq!(job.section(), Route::Jobs);
        assert_eq!(job.title(), "Jobs");
        assert!(!job.about_one_server());
        assert!(Route::Settings.about_one_server());
        assert_eq!(Route::Eval("e1".into()).title(), "Evals");
    }
}
