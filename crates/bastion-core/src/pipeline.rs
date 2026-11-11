//! The pipelines a job can run, and where a failing step sends the work.

use serde::{Deserialize, Serialize};

use crate::Stage;

/// Which run of stages a job goes through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pipeline {
    /// Plan, build, land (D17). Approve and review are plan's and build's
    /// waits (D33).
    #[default]
    Short,
    /// wipp's whole factory, with acceptance tests written and proven first.
    Full,
    /// The full pipeline for a workspace with no checks to run: no acceptance
    /// tests, so no define, your checks, specify, baseline, verify or acceptance.
    FullNoAcceptance,
}

const SHORT: [Stage; 3] = [Stage::Plan, Stage::Build, Stage::Land];

const FULL: [Stage; 21] = [
    Stage::Prepare,
    Stage::Scope,
    Stage::Discover,
    Stage::Plan,
    Stage::Define,
    Stage::Specify,
    Stage::Baseline,
    Stage::Implement,
    Stage::Checks,
    Stage::Verify,
    Stage::Refactor,
    Stage::Acceptance,
    Stage::CodeReview,
    Stage::Ship,
    Stage::Ci,
    Stage::Prove,
    Stage::Comprehend,
    Stage::Uat,
    Stage::Merge,
    Stage::Deploy,
    Stage::Learn,
];

/// Every step in the order work flows through them, short and full
/// pipelines merged. It places a step a workflow left out (`next_listed`).
const ORDER: [Stage; 26] = [
    Stage::Prepare,
    Stage::Scope,
    Stage::Discover,
    Stage::Plan,
    Stage::Define,
    Stage::Specify,
    Stage::Baseline,
    Stage::Build,
    Stage::Implement,
    Stage::Skill,
    Stage::Prompt,
    Stage::Command,
    Stage::Checks,
    Stage::Verify,
    Stage::Refactor,
    Stage::Acceptance,
    Stage::CodeReview,
    Stage::Ship,
    Stage::Ci,
    Stage::Prove,
    Stage::Comprehend,
    Stage::Uat,
    Stage::Land,
    Stage::Merge,
    Stage::Deploy,
    Stage::Learn,
];

fn rank(stage: Stage) -> usize {
    ORDER
        .iter()
        .position(|s| *s == stage)
        .unwrap_or(ORDER.len())
}

/// The step after `at` in `stages`. A stage the list does not have, as a
/// fix loop's return to a step the workflow left out, goes on to the first
/// listed step that comes after it in `ORDER`.
pub fn next_listed(stages: &[Stage], at: Stage) -> Option<Stage> {
    match stages.iter().position(|s| *s == at) {
        Some(i) => stages.get(i + 1).copied(),
        None => stages.iter().copied().find(|s| rank(*s) > rank(at)),
    }
}

/// Whether steps in this order land the change once (land or merge), with
/// only deploy and learn after it.
pub fn check_landing(order: &[Stage]) -> Result<(), String> {
    let lands = order
        .iter()
        .position(|s| matches!(s, Stage::Land | Stage::Merge))
        .ok_or("a workflow needs land or merge, or nothing lands")?;
    match order[lands + 1..]
        .iter()
        .find(|s| !matches!(s, Stage::Deploy | Stage::Learn))
    {
        Some(s) => Err(format!("{s:?} comes after the change lands")),
        None => Ok(()),
    }
}

/// The stages that exist only to write and prove acceptance tests.
const ACCEPTANCE: [Stage; 5] = [
    Stage::Define,
    Stage::Specify,
    Stage::Baseline,
    Stage::Verify,
    Stage::Acceptance,
];

impl Pipeline {
    /// The pipeline's stages, in order. A side stage such as CI fix is not
    /// in it: a failing step enters it, and it hands back (`revise`).
    pub fn stages(self) -> Vec<Stage> {
        match self {
            Pipeline::Short => SHORT.to_vec(),
            Pipeline::Full => FULL.to_vec(),
            Pipeline::FullNoAcceptance => FULL
                .into_iter()
                .filter(|s| !ACCEPTANCE.contains(s))
                .collect(),
        }
    }
}

/// What runs a stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// An agent turn.
    Turn,
    /// A person decided, before D33 folded each checkpoint into the step
    /// it judged (`folded`). No pipeline has one now.
    Checkpoint,
    /// The factory runs it itself: a script, a check, a push, a watch.
    Factory,
}

/// A failing step's way back: the stage that fixes it, the stage the fixed
/// work returns to, and how many times in a row it may fail before the job
/// waits on the owner. The limits are wipp's `REVISIONS`. `None` means the
/// job waits on the owner at once.
pub fn revise(stage: Stage) -> Option<(Stage, Option<Stage>, u32)> {
    Some(match stage {
        Stage::Baseline => (Stage::Specify, None, 2),
        Stage::Checks => (Stage::Implement, Some(Stage::Checks), 2),
        Stage::Verify => (Stage::Implement, Some(Stage::Verify), 2),
        Stage::Acceptance => (Stage::Implement, Some(Stage::Acceptance), 2),
        Stage::Ship => (Stage::Implement, Some(Stage::Acceptance), 3),
        Stage::Ci => (Stage::CiFix, Some(Stage::Ship), 3),
        Stage::Prove => (Stage::Implement, Some(Stage::Checks), 1),
        _ => return None,
    })
}

/// Where a note sends the work, and where the reworked work goes after it.
/// On a `judged` result, the note is what a person sends at that step:
/// answers to discovery's questions go to plan, and picked refactorings,
/// findings or UAT notes go to implement. A note on a step that failed asks
/// for one more try at fixing it, and any other note reruns the step.
pub fn sent_back(stage: Stage, judged: bool) -> (Stage, Option<Stage>) {
    match stage {
        Stage::Discover if judged => (Stage::Plan, None),
        Stage::Refactor if judged => (Stage::Implement, Some(Stage::Acceptance)),
        Stage::CodeReview | Stage::Comprehend if judged => (Stage::Implement, Some(Stage::Checks)),
        other => revise(other).map_or((other, None), |(fix, back, _)| (fix, back)),
    }
}

/// The step an old checkpoint judged, where a stored job waiting at it
/// now waits (D33). Any other stage is itself.
pub fn folded(stage: Stage) -> Stage {
    match stage {
        Stage::Approve => Stage::Plan,
        Stage::Clarify => Stage::Discover,
        Stage::YourChecks => Stage::Define,
        Stage::Review => Stage::Build,
        Stage::PickRefactors => Stage::Refactor,
        Stage::PickFindings => Stage::CodeReview,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_of_every_pipeline_is_in_all_once() {
        let mut all = Stage::ALL.to_vec();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), Stage::ALL.len());
        for stage in FULL.iter().chain(&SHORT).chain([&Stage::CiFix]) {
            assert!(Stage::ALL.contains(stage), "{stage:?}");
        }
    }

    #[test]
    fn the_short_pipeline_is_unchanged() {
        assert_eq!(Pipeline::Short.stages(), SHORT.to_vec());
    }

    #[test]
    fn without_acceptance_tests_the_full_pipeline_goes_from_plan_to_implement() {
        let stages = Pipeline::FullNoAcceptance.stages();
        let plan = stages.iter().position(|s| *s == Stage::Plan).unwrap();
        assert_eq!(stages[plan + 1], Stage::Implement);
        assert!(!stages.contains(&Stage::Verify));
        assert_eq!(stages.len(), FULL.len() - ACCEPTANCE.len());
    }

    #[test]
    fn every_step_has_a_place_in_the_order_and_every_pipeline_lands() {
        for stage in Stage::ALL.into_iter().filter(|s| s.dialed()) {
            assert!(ORDER.contains(&stage), "{stage:?}");
        }
        for p in [Pipeline::Short, Pipeline::Full, Pipeline::FullNoAcceptance] {
            assert_eq!(check_landing(&p.stages()), Ok(()), "{p:?}");
        }
        assert!(check_landing(&[Stage::Plan, Stage::Land, Stage::Build]).is_err());
    }

    #[test]
    fn a_step_the_workflow_left_out_goes_on_to_the_next_listed_one() {
        let steps = [Stage::Plan, Stage::Implement, Stage::Checks, Stage::Land];
        assert_eq!(next_listed(&steps, Stage::Plan), Some(Stage::Implement));
        assert_eq!(next_listed(&steps, Stage::Acceptance), Some(Stage::Land));
        assert_eq!(next_listed(&steps, Stage::Land), None);
    }

    #[test]
    fn no_pipeline_has_a_checkpoint_and_each_one_folds_into_a_step() {
        for stage in FULL.iter().chain(&SHORT) {
            assert!(stage.dialed(), "{stage:?}");
        }
        for stage in Stage::ALL
            .into_iter()
            .filter(|s| s.kind() == Kind::Checkpoint)
        {
            assert!(folded(stage).dialed(), "{stage:?}");
        }
    }
}
