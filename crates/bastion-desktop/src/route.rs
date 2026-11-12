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
    Workspaces,
    Harness,
    Map,
    Settings,
}

/// The top bar's pages. A job and New request belong to Jobs.
/// The top bar's pages (L2), in the order a person uses them. Insights
/// opens on the Audit Log and holds the other records as tabs; Settings
/// holds the server's health.
pub const PAGES: [(Route, &str, &str); 7] = [
    (Route::Jobs, "Journeys", icons::JOBS),
    (Route::Map, "Map", icons::MAP),
    (Route::Workflows, "Trails", icons::WORKFLOWS),
    (Route::Workspaces, "Regions", icons::FOLDER),
    (Route::Harness, "Guide", icons::HARNESS),
    (Route::Audit, "Logbook", icons::INSIGHTS),
    (Route::Settings, "Settings", icons::SETTINGS),
];

impl Route {
    /// The top bar entry this page sits under.
    pub fn section(&self) -> Route {
        match self {
            Route::NewRequest | Route::Job(_) => Route::Jobs,
            Route::Eval(_)
            | Route::Evals
            | Route::Session(_)
            | Route::Costs
            | Route::Verification => Route::Audit,
            Route::Engine => Route::Settings,
            other => other.clone(),
        }
    }

    /// The top-bar entry whose tabs this page sits among.
    pub fn tab_group(&self) -> Route {
        self.section()
    }

    /// The tab this page lights: its own, or the list a record belongs to.
    pub fn tab(&self) -> Route {
        match self {
            Route::Eval(_) => Route::Evals,
            Route::Session(_) => Route::Costs,
            other => other.clone(),
        }
    }

    /// A page that can show every server at once (D58): the records, the
    /// workspaces, and the rules a workflow applies. A workflow's steps and
    /// one record's page show one server.
    pub fn records(&self) -> bool {
        matches!(
            self,
            Route::Jobs
                | Route::Audit
                | Route::Costs
                | Route::Evals
                | Route::Verification
                | Route::Workspaces
                | Route::Harness
                | Route::Map
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
        assert_eq!(job.title(), "Journeys");
        assert!(!job.records() && job.one_record());
        assert!(Route::Costs.records() && !Route::Settings.records());
        assert_eq!(Route::Eval("e1".into()).title(), "Logbook");
        assert_eq!(Route::Eval("e1".into()).tab(), Route::Evals);
        assert_eq!(Route::Costs.title(), "Logbook");
        assert_eq!(Route::Engine.section(), Route::Settings);
    }
}
