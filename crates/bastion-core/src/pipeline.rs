//! The pipelines a job can run, and where a failing step sends the work
//! (PLAN.md A8, after wipp's stage graph in `stages.py` at `f7c7e02`).

use serde::{Deserialize, Serialize};

use crate::Stage;

/// Which run of stages a job goes through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pipeline {
    /// Plan, approve, build, review, land (D17).
    #[default]
    Short,
    /// wipp's whole factory, with acceptance tests written and proven first.
    Full,
    /// The full pipeline for a workspace with no checks to run: no acceptance
    /// tests, so no define, your checks, specify, baseline, verify or acceptance.
    FullNoAcceptance,
}

const SHORT: [Stage; 5] = [
    Stage::Plan,
    Stage::Approve,
    Stage::Build,
    Stage::Review,
    Stage::Land,
];

const FULL: [Stage; 26] = [
    Stage::Prepare,
    Stage::Scope,
    Stage::Discover,
    Stage::Clarify,
    Stage::Plan,
    Stage::Approve,
    Stage::Define,
    Stage::YourChecks,
    Stage::Specify,
    Stage::Baseline,
    Stage::Implement,
    Stage::Checks,
    Stage::Verify,
    Stage::Refactor,
    Stage::PickRefactors,
    Stage::Acceptance,
    Stage::CodeReview,
    Stage::PickFindings,
    Stage::Ship,
    Stage::Ci,
    Stage::Prove,
    Stage::Comprehend,
    Stage::Uat,
    Stage::Merge,
    Stage::Deploy,
    Stage::Learn,
];

/// The stages that exist only to write and prove acceptance tests.
const ACCEPTANCE: [Stage; 6] = [
    Stage::Define,
    Stage::YourChecks,
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
    /// The owner decides, under the checkpoint's dial.
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
        _ => return None,
    })
}

/// Where a comment at a checkpoint sends the work, and where the reworked
/// work goes after it. A comment on a step that waits on the owner asks for
/// one more try at fixing it.
pub fn sent_back(stage: Stage) -> (Stage, Option<Stage>) {
    match stage {
        Stage::Approve | Stage::Clarify => (Stage::Plan, None),
        Stage::Review => (Stage::Build, None),
        Stage::YourChecks => (Stage::Define, None),
        Stage::PickRefactors => (Stage::Implement, Some(Stage::Acceptance)),
        Stage::PickFindings | Stage::Uat => (Stage::Implement, Some(Stage::Checks)),
        other => revise(other).map_or((other, None), |(fix, back, _)| (fix, back)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_short_pipeline_is_unchanged() {
        assert_eq!(Pipeline::Short.stages(), SHORT.to_vec());
    }

    #[test]
    fn without_acceptance_tests_the_full_pipeline_goes_from_approve_to_implement() {
        let stages = Pipeline::FullNoAcceptance.stages();
        let approve = stages.iter().position(|s| *s == Stage::Approve).unwrap();
        assert_eq!(stages[approve + 1], Stage::Implement);
        assert!(!stages.contains(&Stage::Verify));
        assert_eq!(stages.len(), FULL.len() - ACCEPTANCE.len());
    }

    #[test]
    fn every_full_stage_has_one_kind_and_only_checkpoints_wait_on_a_dial() {
        let checkpoints: Vec<Stage> = Pipeline::Full
            .stages()
            .into_iter()
            .filter(|s| s.kind() == Kind::Checkpoint)
            .collect();
        assert_eq!(
            checkpoints,
            [
                Stage::Clarify,
                Stage::Approve,
                Stage::YourChecks,
                Stage::PickRefactors,
                Stage::PickFindings,
                Stage::Uat,
                Stage::Merge,
            ]
        );
    }
}
