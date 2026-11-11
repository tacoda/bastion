//! The owner's settings (DECISIONS.md D23): each step's dial, the model
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

impl Model {
    /// A model named `provider/model`, as a request may name one.
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

/// The oversight presets, one per workflow level (D33).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Oversight {
    /// Every step waits for a person.
    LightsOn,
    /// Only the steps that act outside the job (ship, land, merge, deploy)
    /// wait for a person, so a person gates the merge.
    Dim,
    /// No step waits.
    Dark,
}

impl Oversight {
    pub fn dials(self) -> Dials {
        match self {
            Oversight::LightsOn => Dials::all(Dial::LightsOn),
            Oversight::Dim => {
                let mut d = Dials::all(Dial::Dark);
                for stage in Stage::ALL.into_iter().filter(|s| s.acts()) {
                    d.set(stage, Dial::LightsOn);
                }
                d
            }
            Oversight::Dark => Dials::all(Dial::Dark),
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
    /// No queued job is admitted while intake is paused (F8). Requests
    /// still queue, and jobs already admitted go on.
    pub intake_paused: bool,
}

/// How a version says whether intake is paused.
fn intake(paused: bool) -> &'static str {
    match paused {
        true => "paused",
        false => "open",
    }
}

/// The key a version keeps a step's dial by, such as `step.code_review`:
/// the stage's name in snake case, as serde writes it.
fn step_key(stage: Stage) -> String {
    let mut key = String::from("step.");
    for (i, c) in format!("{stage:?}").chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            key.push('_');
        }
        key.push(c.to_ascii_lowercase());
    }
    key
}

/// The checkpoint dials a version kept before D33, and the steps each
/// judged. Review and merge never went below hold, which reads as
/// lights-on, the default, so they are not read.
const OLD_DIALS: [(&str, &[Stage]); 5] = [
    ("dial.approve", &[Stage::Plan]),
    ("dial.questions", &[Stage::Discover]),
    ("dial.checks", &[Stage::Define]),
    ("dial.refactor", &[Stage::Refactor]),
    ("dial.uat", &[Stage::Comprehend]),
];

/// Each step's dial from a version: the old checkpoint dials first, then
/// each step's own.
fn dials_from(read: &dyn Fn(&str) -> Option<Dial>) -> Dials {
    let mut d = Dials::default();
    for (key, stages) in OLD_DIALS {
        let dial = read(key);
        stages
            .iter()
            .for_each(|s| dial.into_iter().for_each(|v| d.set(*s, v)));
    }
    for stage in Stage::ALL.into_iter().filter(|s| s.dialed()) {
        read(&step_key(stage))
            .into_iter()
            .for_each(|v| d.set(stage, v));
    }
    d
}

/// Each setting's words, as a version keeps them. The first word is the
/// one written; the others are read from versions kept before D33.
const DIALS: [(Dial, &str); 5] = [
    (Dial::LightsOn, "lights_on"),
    (Dial::Dark, "dark"),
    (Dial::LightsOn, "ask"),
    (Dial::LightsOn, "hold"),
    (Dial::Dark, "auto"),
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
    /// The model a turn at `stage` runs on: the job's own when it named one
    /// (F8), else the plan model for plan and the build model for the rest.
    pub fn model_for(&self, stage: Stage, job_model: Option<&str>) -> Model {
        job_model
            .and_then(Model::named)
            .unwrap_or_else(|| self.model(stage).clone())
    }

    pub fn model(&self, stage: Stage) -> &Model {
        if stage == Stage::Plan {
            &self.plan_model
        } else {
            &self.build_model
        }
    }

    /// The settings as the key/value pairs a version keeps, in a fixed order.
    pub fn to_pairs(&self) -> Vec<(String, String)> {
        let dialed = Stage::ALL.into_iter().filter(|s| s.dialed());
        dialed
            .map(|s| (step_key(s), word(&DIALS, self.dials.at(s))))
            .chain(
                [
                    ("model.plan.provider", self.plan_model.provider.clone()),
                    ("model.plan.model", self.plan_model.model.clone()),
                    ("model.build.provider", self.build_model.provider.clone()),
                    ("model.build.model", self.build_model.model.clone()),
                    ("forge", word(&FORGES, self.forge)),
                    ("tracker", word(&TRACKERS, self.tracker)),
                    ("intake", intake(self.intake_paused).into()),
                ]
                .map(|(k, v)| (k.to_string(), v)),
            )
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
        s.dials = dials_from(&|key| get(key).and_then(|w| from_word(&DIALS, w)));
        s.forge = get("forge")
            .and_then(|w| from_word(&FORGES, w))
            .unwrap_or_default();
        s.tracker = get("tracker")
            .and_then(|w| from_word(&TRACKERS, w))
            .unwrap_or_default();
        s.intake_paused = get("intake") == Some(intake(true));
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
    fn a_job_that_names_a_model_runs_its_turns_on_it() {
        let s = Settings::default();
        let own = s.model_for(Stage::Build, Some("deepseek/deepseek-flash"));
        assert_eq!(
            (own.provider.as_str(), own.model.as_str()),
            ("deepseek", "deepseek-flash")
        );
        assert_eq!(s.model_for(Stage::Plan, None), s.plan_model);
        assert_eq!(Model::named("no-slash"), None);
        assert_eq!(Model::named("openai/ "), None);
    }

    #[test]
    fn settings_round_trip_through_their_pairs() {
        let s = Settings {
            dials: Oversight::Dim.dials(),
            plan_model: Model {
                provider: "anthropic".into(),
                model: "claude-sonnet-5-5".into(),
            },
            build_model: Model::default(),
            forge: Forge::Github,
            tracker: Tracker::GithubIssues,
            intake_paused: true,
        };
        assert_eq!(Settings::from_pairs(&s.to_pairs()), s);
        let open = Settings::default().to_pairs();
        assert!(open.contains(&("intake".into(), "open".into())));
    }

    #[test]
    fn a_steps_key_is_its_name_in_snake_case() {
        assert_eq!(step_key(Stage::CodeReview), "step.code_review");
        assert_eq!(step_key(Stage::Plan), "step.plan");
    }

    #[test]
    fn a_missing_or_unreadable_key_keeps_its_default() {
        let pairs = vec![
            ("forge".to_string(), "gitlab".to_string()),
            ("step.plan".into(), "dark".into()),
            ("step.build".into(), "loud".into()),
        ];
        let s = Settings::from_pairs(&pairs);
        assert_eq!(
            (s.forge, s.dials.at(Stage::Plan), s.dials.at(Stage::Build)),
            (Forge::Local, Dial::Dark, Dial::LightsOn)
        );
    }

    #[test]
    fn each_preset_makes_its_level_on_both_pipelines() {
        use crate::{Level, Pipeline};
        for pipeline in [Pipeline::Short, Pipeline::Full] {
            for (preset, level) in [
                (Oversight::LightsOn, Level::LightsOn),
                (Oversight::Dim, Level::Dim),
                (Oversight::Dark, Level::Dark),
            ] {
                assert_eq!(
                    preset.dials().level(&pipeline.stages()),
                    level,
                    "{preset:?} {pipeline:?}"
                );
            }
        }
    }

    #[test]
    fn a_gate_only_after_the_merge_leaves_the_workflow_dark() {
        use crate::{Level, Pipeline};
        let mut d = Dials::all(Dial::Dark);
        d.set(Stage::Learn, Dial::LightsOn);
        assert_eq!(d.level(&Pipeline::Full.stages()), Level::Dark);
        d.set(Stage::Plan, Dial::LightsOn);
        assert_eq!(d.level(&Pipeline::Full.stages()), Level::Dim);
    }

    #[test]
    fn a_version_kept_before_d33_maps_each_old_dial_to_the_steps_it_judged() {
        let pairs = [
            ("dial.approve", "auto"),
            ("dial.review", "auto"),
            ("dial.questions", "hold"),
            ("dial.uat", "ask"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()));
        let d = Settings::from_pairs(&pairs).dials;
        assert_eq!(d.at(Stage::Plan), Dial::Dark);
        assert_eq!(
            d.at(Stage::Build),
            Dial::LightsOn,
            "review never went below hold"
        );
        assert_eq!(d.at(Stage::CodeReview), Dial::LightsOn);
        assert_eq!(d.at(Stage::Discover), Dial::LightsOn, "hold is lights-on");
        assert_eq!(d.at(Stage::Comprehend), Dial::LightsOn);
        assert_eq!(d.at(Stage::Checks), Dial::LightsOn, "a step no dial judged");
        let medium = Settings {
            dials: Oversight::Dim.dials(),
            ..Settings::default()
        };
        assert_eq!(Settings::from_pairs(&medium.to_pairs()), medium);
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
