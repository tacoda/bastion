//! Where the window is, and the top bar's pages.

use crate::icons;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Jobs,
    NewRequest,
    Job(String),
    Audit,
    Evals,
    Eval(String),
    Costs,
    Verification,
    Session(String),
    Engine,
    Workflows,
    Settings,
}

/// The top bar's pages. A job and New request belong to Jobs.
pub const PAGES: [(Route, &str, &str); 8] = [
    (Route::Jobs, "Jobs", icons::JOBS),
    (Route::Workflows, "Workflows", icons::WORKFLOWS),
    (Route::Audit, "Audit Log", icons::AUDIT),
    (Route::Evals, "Evals", icons::EVALS),
    (Route::Costs, "Costs", icons::COSTS),
    (Route::Verification, "Verification", icons::VERIFICATION),
    (Route::Engine, "Engine", icons::ENGINE),
    (Route::Settings, "Settings", icons::SETTINGS),
];

impl Route {
    /// The top bar entry this page sits under.
    pub fn section(&self) -> Route {
        match self {
            Route::NewRequest | Route::Job(_) => Route::Jobs,
            Route::Eval(_) => Route::Evals,
            Route::Session(_) => Route::Costs,
            other => other.clone(),
        }
    }

    /// A records page can show every server at once; a rules page and
    /// one record's page show one server (VISION.md).
    pub fn records(&self) -> bool {
        matches!(
            self,
            Route::Jobs | Route::Audit | Route::Costs | Route::Evals | Route::Verification
        )
    }

    /// One record's page, which belongs to the server it was opened on.
    pub fn one_record(&self) -> bool {
        matches!(self, Route::Job(_) | Route::Eval(_) | Route::Session(_))
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
    fn a_job_sits_under_jobs_and_only_record_lists_span_servers() {
        let job = Route::Job("j1".into());
        assert_eq!(job.section(), Route::Jobs);
        assert_eq!(job.title(), "Jobs");
        assert!(!job.records() && job.one_record());
        assert!(Route::Costs.records() && !Route::Settings.records());
        assert_eq!(Route::Eval("e1".into()).title(), "Evals");
    }
}
