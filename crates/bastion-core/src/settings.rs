//! The owner's settings (PLAN.md A7, D23): the checkpoint dials, the model
//! each agent stage runs, and which forge and tracker the factory uses. They
//! are kept as versions of key/value pairs, so any version can be restored.

use serde::{Deserialize, Serialize};

use crate::{Dial, Dials, Stage};

/// One model on one provider, as `llm-router` names them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub provider: String,
    pub model: String,
}

impl Default for Model {
    fn default() -> Self {
        Model {
            provider: "openai".into(),
            model: "gpt-5-mini".into(),
        }
    }
}

/// Where a reviewed change goes: merged into the workspace's branch here,
/// or pushed and opened as a pull request on GitHub.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Forge {
    #[default]
    Local,
    Github,
}

/// Where requests can come from besides the owner's own words.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tracker {
    #[default]
    None,
    GithubIssues,
}

/// How much the owner decides, as one choice that sets both dials.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Oversight {
    /// The owner approves every plan and every change.
    High,
    /// Plans and changes approve themselves after a hold unless the owner acts.
    Medium,
    /// Plans approve at once; changes still hold for the owner.
    Low,
    /// As little as Bastion allows: review never goes below hold (D15).
    Off,
}

impl Oversight {
    pub fn dials(self) -> Dials {
        let (approve, review) = match self {
            Oversight::High => (Dial::Ask, Dial::Ask),
            Oversight::Medium => (Dial::Hold, Dial::Hold),
            Oversight::Low | Oversight::Off => (Dial::Auto, Dial::Hold),
        };
        Dials {
            approve,
            review,
            ..Dials::default()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub dials: Dials,
    pub plan_model: Model,
    pub build_model: Model,
    pub forge: Forge,
    pub tracker: Tracker,
}

/// Each setting's words, as a version keeps them.
const DIALS: [(Dial, &str); 3] = [
    (Dial::Ask, "ask"),
    (Dial::Hold, "hold"),
    (Dial::Auto, "auto"),
];
const FORGES: [(Forge, &str); 2] = [(Forge::Local, "local"), (Forge::Github, "github")];
const TRACKERS: [(Tracker, &str); 2] = [
    (Tracker::None, "none"),
    (Tracker::GithubIssues, "github_issues"),
];

fn word<T: PartialEq + Copy>(table: &[(T, &'static str)], value: T) -> String {
    table
        .iter()
        .find(|(v, _)| *v == value)
        .map_or("", |(_, w)| w)
        .to_string()
}

fn from_word<T: Copy>(table: &[(T, &'static str)], text: &str) -> Option<T> {
    table.iter().find(|(_, w)| *w == text).map(|(v, _)| *v)
}

impl Settings {
    /// The model an agent stage runs on.
    pub fn model(&self, stage: Stage) -> &Model {
        if stage == Stage::Plan {
            &self.plan_model
        } else {
            &self.build_model
        }
    }

    /// The settings as the key/value pairs a version keeps, in a fixed order.
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        [
            ("dial.approve", word(&DIALS, self.dials.approve)),
            ("dial.review", word(&DIALS, self.dials.review)),
            ("model.plan.provider", self.plan_model.provider.clone()),
            ("model.plan.model", self.plan_model.model.clone()),
            ("model.build.provider", self.build_model.provider.clone()),
            ("model.build.model", self.build_model.model.clone()),
            ("forge", word(&FORGES, self.forge)),
            ("tracker", word(&TRACKERS, self.tracker)),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
    }

    /// Settings from a version's pairs. A key that is missing or does not
    /// read keeps its default, so an older version still restores.
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
        if let Some(d) = get("dial.approve").and_then(|w| from_word(&DIALS, w)) {
            s.dials.approve = d;
        }
        if let Some(d) = get("dial.review").and_then(|w| from_word(&DIALS, w)) {
            s.dials.review = d;
        }
        s.forge = get("forge")
            .and_then(|w| from_word(&FORGES, w))
            .unwrap_or_default();
        s.tracker = get("tracker")
            .and_then(|w| from_word(&TRACKERS, w))
            .unwrap_or_default();
        s
    }

    /// The keys whose values differ from `before`, as "key: old -> new".
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
    fn settings_round_trip_through_their_pairs() {
        let s = Settings {
            dials: Oversight::Low.dials(),
            plan_model: Model {
                provider: "anthropic".into(),
                model: "claude-sonnet-5-5".into(),
            },
            build_model: Model::default(),
            forge: Forge::Github,
            tracker: Tracker::GithubIssues,
        };
        assert_eq!(Settings::from_pairs(&s.to_pairs()), s);
    }

    #[test]
    fn a_missing_or_unreadable_key_keeps_its_default() {
        let pairs = vec![
            ("forge".to_string(), "gitlab".to_string()),
            ("dial.approve".into(), "auto".into()),
        ];
        let s = Settings::from_pairs(&pairs);
        assert_eq!(
            (s.forge, s.dials.approve, s.build_model.clone()),
            (Forge::Local, Dial::Auto, Model::default())
        );
    }

    #[test]
    fn a_preset_sets_both_dials_and_review_never_drops_below_hold() {
        assert_eq!(Oversight::High.dials().at(Stage::Review), Dial::Ask);
        assert_eq!(Oversight::Off.dials().at(Stage::Approve), Dial::Auto);
        assert_eq!(Oversight::Off.dials().at(Stage::Review), Dial::Hold);
    }

    #[test]
    fn changes_name_each_key_that_moved() {
        let before = Settings::default();
        let after = Settings {
            forge: Forge::Github,
            ..Settings::default()
        };
        assert_eq!(after.changes(&before), ["forge: local -> github"]);
        assert_eq!(after.model(Stage::Build), &Model::default());
    }
}
