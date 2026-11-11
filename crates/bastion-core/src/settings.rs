//! The owner's settings (DECISIONS.md D23): the models turns run on, the
//! tracker requests may come from, and whether intake is open. They are kept
//! as versions of key/value pairs, so any version can be restored. A step's
//! dial is its workflow's, not a setting (D39).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub provider: String,
    pub model: String,
}

impl Model {
    /// A model written as `provider/model`.
    pub fn named(text: &str) -> Option<Model> {
        let (provider, model) = text.trim().split_once('/')?;
        let (provider, model) = (provider.trim(), model.trim());
        (!provider.is_empty() && !model.is_empty()).then(|| Model {
            provider: provider.into(),
            model: model.into(),
        })
    }
}

impl Default for Model {
    fn default() -> Self {
        Model {
            provider: "openai".into(),
            model: "gpt-5-mini".into(),
        }
    }
}

/// Where requests may come from besides the app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tracker {
    #[default]
    None,
    GithubIssues,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// The model a step that only reads runs on, such as a plan.
    pub plan_model: Model,
    /// The model every other agent's step runs on.
    pub build_model: Model,
    pub tracker: Tracker,
    pub intake_paused: bool,
}

fn intake(paused: bool) -> &'static str {
    match paused {
        true => "paused",
        false => "open",
    }
}

const TRACKERS: [(Tracker, &str); 2] = [
    (Tracker::None, "none"),
    (Tracker::GithubIssues, "github_issues"),
];

impl Settings {
    /// The model a turn runs on: the job's own when it named one, else the
    /// plan model for a step that only reads and the build model for the rest.
    pub fn model_for(&self, reads_only: bool, job_model: Option<&str>) -> Model {
        let kept = match reads_only {
            true => &self.plan_model,
            false => &self.build_model,
        };
        job_model
            .and_then(Model::named)
            .unwrap_or_else(|| kept.clone())
    }

    /// The settings as the key/value pairs a version keeps, in a fixed order.
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        let tracker = TRACKERS.iter().find(|(t, _)| *t == self.tracker);
        [
            ("model.plan.provider", self.plan_model.provider.clone()),
            ("model.plan.model", self.plan_model.model.clone()),
            ("model.build.provider", self.build_model.provider.clone()),
            ("model.build.model", self.build_model.model.clone()),
            ("tracker", tracker.map_or("", |(_, w)| w).to_string()),
            ("intake", intake(self.intake_paused).into()),
        ]
        .map(|(k, v)| (k.to_string(), v))
        .into()
    }

    pub fn from_pairs(pairs: &[(String, String)]) -> Settings {
        let get = |key: &str| {
            pairs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        let mut s = Settings::default();
        let text = |key: &str, into: &mut String| {
            if let Some(v) = get(key).filter(|v| !v.trim().is_empty()) {
                *into = v.trim().into();
            }
        };
        text("model.plan.provider", &mut s.plan_model.provider);
        text("model.plan.model", &mut s.plan_model.model);
        text("model.build.provider", &mut s.build_model.provider);
        text("model.build.model", &mut s.build_model.model);
        s.tracker = get("tracker")
            .and_then(|w| TRACKERS.iter().find(|(_, t)| *t == w).map(|(v, _)| *v))
            .unwrap_or_default();
        s.intake_paused = get("intake") == Some(intake(true));
        s
    }

    pub fn changes(&self, before: &Settings) -> Vec<String> {
        let (now, was) = (self.to_pairs(), before.to_pairs());
        now.iter()
            .zip(&was)
            .filter(|(n, w)| n.1 != w.1)
            .map(|(n, w)| format!("{}: {} -> {}", n.0, w.1, n.1))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_runs_on_the_jobs_model_or_the_one_for_what_it_does() {
        let s = Settings {
            plan_model: Model::named("anthropic/claude-sonnet-5-5").unwrap(),
            ..Settings::default()
        };
        let own = s.model_for(false, Some("deepseek/deepseek-flash"));
        assert_eq!(
            (own.provider.as_str(), own.model.as_str()),
            ("deepseek", "deepseek-flash")
        );
        assert_eq!(s.model_for(true, None), s.plan_model);
        assert_eq!(s.model_for(false, None), s.build_model);
        assert_eq!(Model::named("no-slash"), None);
        assert_eq!(Model::named("openai/ "), None);
    }

    #[test]
    fn settings_round_trip_through_their_pairs_and_name_what_moved() {
        let s = Settings {
            plan_model: Model::named("anthropic/claude-sonnet-5-5").unwrap(),
            build_model: Model::default(),
            tracker: Tracker::GithubIssues,
            intake_paused: true,
        };
        assert_eq!(Settings::from_pairs(&s.to_pairs()), s);
        let open = Settings::default();
        assert!(open.to_pairs().contains(&("intake".into(), "open".into())));
        assert_eq!(
            s.changes(&open)[0],
            "model.plan.provider: openai -> anthropic"
        );
        let old = [
            ("step.plan", "dark"),
            ("forge", "github"),
            ("tracker", "jira"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()));
        assert_eq!(
            Settings::from_pairs(&old),
            Settings::default(),
            "old keys are ignored"
        );
    }
}
