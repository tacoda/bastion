//! The full pipeline's transitions (A8 step 1), as plain function calls.

use super::*;

const AT: u64 = 1_000;

fn job(pipeline: Pipeline) -> Job {
    Job::new(
        "j1".into(),
        "bastion".into(),
        "Add a health check".into(),
        AT,
    )
    .on(pipeline)
}

/// A full-pipeline job in `state` at `stage`.
fn at(stage: Stage, state: State) -> Job {
    let mut j = job(Pipeline::Full);
    j.stage = stage;
    j.state = state;
    j
}

fn done() -> Event {
    Event::TurnFinished {
        outcome: Outcome::Done {
            report: "done".into(),
        },
    }
}

fn failed(detail: &str) -> Event {
    Event::StepFailed {
        detail: detail.into(),
    }
}

fn go(j: &Job, e: &Event) -> (Job, Vec<Effect>) {
    step(j, e, AT + 1, &Dials::default())
}

fn starts_turn(effects: &[Effect], stage: Stage) -> bool {
    effects
        .iter()
        .any(|e| matches!(e, Effect::StartTurn { stage: s, .. } if *s == stage))
}

#[test]
fn a_full_job_prepares_first_then_scopes_read_only() {
    let j = job(Pipeline::Full);
    assert_eq!(j.stage, Stage::Prepare);
    let (j, _) = go(&j, &Event::Admitted);
    let (j, effects) = go(&j, &Event::Provisioned);
    assert_eq!(
        effects,
        vec![Effect::RunStep {
            stage: Stage::Prepare
        }]
    );
    let (j, effects) = go(&j, &Event::StepPassed);
    assert_eq!(j.stage, Stage::Scope);
    assert!(matches!(
        effects.as_slice(),
        [Effect::StartTurn {
            stage: Stage::Scope,
            read_only: true,
            ..
        }]
    ));
}

#[test]
fn without_acceptance_tests_an_approved_plan_goes_to_implement() {
    let mut j = job(Pipeline::FullNoAcceptance);
    j.stage = Stage::Approve;
    j.state = State::NeedsYou;
    let (j, effects) = go(&j, &Event::Approved);
    assert_eq!(j.stage, Stage::Implement);
    assert!(starts_turn(&effects, Stage::Implement));
}

#[test]
fn each_checkpoint_obeys_its_own_dial_and_merge_never_goes_below_hold() {
    let dials = Dials {
        questions: Dial::Auto,
        merge: Dial::Auto,
        ..Dials::default()
    };
    let (j, effects) = step(&at(Stage::Discover, State::Running), &done(), AT, &dials);
    assert_eq!(j.stage, Stage::Plan, "clarify on auto passes through");
    assert!(starts_turn(&effects, Stage::Plan));

    let (j, effects) = step(&at(Stage::Comprehend, State::Running), &done(), AT, &dials);
    assert_eq!((j.stage, j.state), (Stage::Uat, State::NeedsYou));
    assert!(!effects.iter().any(|e| matches!(e, Effect::Hold { .. })));
    let (j, effects) = step(&j, &Event::Approved, AT, &dials);
    assert_eq!((j.stage, j.state), (Stage::Merge, State::NeedsYou));
    assert!(effects.contains(&Effect::Hold {
        stage: Stage::Merge,
        secs: HOLD_SECS
    }));
}

#[test]
fn failing_checks_send_the_work_to_implement_and_back_to_the_checks() {
    let (j, effects) = go(&at(Stage::Checks, State::Running), &failed("1 failed"));
    assert_eq!(
        (j.stage, j.back_to),
        (Stage::Implement, Some(Stage::Checks))
    );
    let brief = match effects.as_slice() {
        [Effect::StartTurn { brief, .. }] => brief.clone(),
        other => panic!("{other:?}"),
    };
    assert!(brief.contains("1 failed"));
    let (j, effects) = go(&j, &done());
    assert_eq!((j.stage, j.back_to), (Stage::Checks, None));
    assert!(effects.contains(&Effect::RunStep {
        stage: Stage::Checks
    }));
}

#[test]
fn the_same_failure_twice_waits_on_the_owner_and_approve_goes_on_without_it() {
    let (j, _) = go(&at(Stage::Checks, State::Running), &failed("1 failed"));
    let (j, _) = go(&j, &done());
    let (j, effects) = go(&j, &failed("1 failed"));
    assert_eq!((j.stage, j.state), (Stage::Checks, State::NeedsYou));
    assert_eq!(j.error.as_deref(), Some("1 failed"));
    assert!(effects.is_empty());
    let (j, _) = go(&j, &Event::Approved);
    assert_eq!((j.stage, j.error), (Stage::Verify, None));
}

#[test]
fn a_step_waits_on_the_owner_once_its_limit_is_spent() {
    let mut j = at(Stage::Ship, State::Running);
    for i in 0..3 {
        (j, _) = go(&j, &failed(&format!("refusal {i}")));
        assert_eq!(j.stage, Stage::Implement);
        (j, _) = go(&j, &done());
        assert_eq!(j.stage, Stage::Acceptance);
        j.stage = Stage::Ship;
    }
    let (j, _) = go(&j, &failed("refusal 3"));
    assert_eq!((j.stage, j.state), (Stage::Ship, State::NeedsYou));
}

#[test]
fn implement_runs_one_turn_per_task_then_the_checks() {
    let mut j = at(Stage::Implement, State::Running);
    j.tasks = vec!["Add the route".into(), "Add the test".into()];
    let (j, effects) = go(&j, &done());
    assert_eq!((j.stage, j.done_tasks), (Stage::Implement, 1));
    let brief = match effects.last() {
        Some(Effect::StartTurn { brief, .. }) => brief.clone(),
        other => panic!("{other:?}"),
    };
    assert!(brief.contains("Add the test") && !brief.contains("Add the route"));
    let (j, effects) = go(&j, &done());
    assert_eq!(j.stage, Stage::Checks);
    assert!(effects.contains(&Effect::RunStep {
        stage: Stage::Checks
    }));
}

#[test]
fn ship_opens_the_pull_request_and_a_ci_failure_goes_through_ci_fix_and_ship_again() {
    let (j, effects) = go(
        &at(Stage::Ship, State::Running),
        &Event::PrOpened { url: "pr/2".into() },
    );
    assert_eq!((j.stage, j.pr.as_deref()), (Stage::Ci, Some("pr/2")));
    assert_eq!(effects, vec![Effect::WatchCi]);
    let (j, effects) = go(&j, &Event::CiFailed { log: "red".into() });
    assert_eq!((j.stage, j.back_to), (Stage::CiFix, Some(Stage::Ship)));
    assert!(starts_turn(&effects, Stage::CiFix));
    let (j, effects) = go(&j, &done());
    assert_eq!(j.stage, Stage::Ship);
    assert!(effects.contains(&Effect::RunStep { stage: Stage::Ship }));
    let (j, _) = go(&j, &Event::PrOpened { url: "pr/2".into() });
    let (j, _) = go(&j, &Event::CiPassed);
    assert_eq!(j.stage, Stage::Prove);
}

#[test]
fn the_merge_then_deploy_then_learn_land_the_job() {
    let (j, effects) = go(&at(Stage::Merge, State::NeedsYou), &Event::Approved);
    assert_eq!((j.state, effects), (State::Running, vec![Effect::Merge]));
    let (j, effects) = go(&j, &Event::Landed);
    assert_eq!(j.stage, Stage::Deploy);
    assert_eq!(
        effects,
        vec![Effect::RunStep {
            stage: Stage::Deploy
        }]
    );
    let (j, _) = go(&j, &Event::StepPassed);
    assert_eq!(j.stage, Stage::Learn);
    let (j, effects) = go(&j, &done());
    assert_eq!(j.state, State::Landed);
    assert!(effects.contains(&Effect::Teardown));
}

#[test]
fn a_failed_deploy_waits_on_the_owner_at_once() {
    let (j, _) = go(
        &at(Stage::Deploy, State::Running),
        &failed("health check red"),
    );
    assert_eq!((j.stage, j.state), (Stage::Deploy, State::NeedsYou));
}

#[test]
fn comments_at_the_picks_and_uat_send_the_work_to_implement() {
    let comment = Event::Commented {
        text: "Extract the parser".into(),
    };
    let (j, _) = go(&at(Stage::PickRefactors, State::NeedsYou), &comment);
    assert_eq!(
        (j.stage, j.back_to),
        (Stage::Implement, Some(Stage::Acceptance))
    );

    let mut uat = at(Stage::Uat, State::NeedsYou);
    uat.revisions.insert(Stage::Checks, 2);
    let (j, _) = go(&uat, &comment);
    assert_eq!(
        (j.stage, j.back_to),
        (Stage::Implement, Some(Stage::Checks))
    );
    assert!(
        j.revisions.is_empty(),
        "a failed UAT starts the limits again"
    );
}

#[test]
fn the_merge_offers_only_approve_and_stop() {
    let j = at(Stage::Merge, State::NeedsYou);
    assert_eq!(actions(&j), vec![Action::Approve, Action::Stop]);
}

#[test]
fn steps_show_the_whole_full_pipeline() {
    let j = at(Stage::Scope, State::Running);
    let steps = steps(&j);
    assert_eq!(steps.len(), Pipeline::Full.stages().len());
    assert_eq!(steps[0], (Stage::Prepare, StepState::Done));
    assert_eq!(steps[1], (Stage::Scope, StepState::Running));
    assert_eq!(steps[2], (Stage::Discover, StepState::Pending));
}

const DISCOVERY: &str =
    "TASKS:\n1. Add the /health route\n2) Add its test\nQUESTIONS:\n- Which port?\n";

#[test]
fn discovery_reads_tasks_and_questions() {
    assert_eq!(
        discovery(DISCOVERY),
        Discovery {
            tasks: vec!["Add the /health route".into(), "Add its test".into()],
            questions: vec!["Which port?".into()],
        }
    );
    assert_eq!(discovery("I looked around."), Discovery::default());
}

#[test]
fn discovery_with_questions_waits_at_clarify_and_the_answers_reach_the_plan() {
    let (j, effects) = go(
        &at(Stage::Discover, State::Running),
        &Event::TurnFinished {
            outcome: Outcome::Done {
                report: DISCOVERY.into(),
            },
        },
    );
    assert_eq!((j.stage, j.state), (Stage::Clarify, State::NeedsYou));
    assert_eq!(j.tasks.len(), 2);
    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::RecordArtifact {
            stage: Stage::Discover,
            ..
        }
    )));
    let (j, effects) = go(
        &j,
        &Event::Commented {
            text: "8080".into(),
        },
    );
    assert_eq!(j.stage, Stage::Plan);
    let brief = match effects.as_slice() {
        [Effect::StartTurn { brief, .. }] => brief.clone(),
        other => panic!("{other:?}"),
    };
    assert!(brief.contains("8080") && brief.contains("- Add its test"));
}

#[test]
fn discovery_without_questions_goes_straight_to_the_plan() {
    let (j, _) = go(
        &at(Stage::Discover, State::Running),
        &Event::TurnFinished {
            outcome: Outcome::Done {
                report: "TASKS:\n1. Add the route\nQUESTIONS:\n".into(),
            },
        },
    );
    assert_eq!((j.stage, j.state), (Stage::Plan, State::Running));
}

fn reply(report: &str) -> Event {
    Event::TurnFinished {
        outcome: Outcome::Done {
            report: report.into(),
        },
    }
}

const SPEC: &str =
    r#"{"checks": [{"criterion": "C1", "run": "pytest -k c1", "files": ["tests/test_c1.py"]}]}"#;

#[test]
fn defines_criteria_reach_specify_and_the_owner_may_redefine_three_times() {
    let (j, _) = go(&at(Stage::Define, State::Running), &reply("C1: it answers"));
    assert_eq!(
        (j.stage, j.criteria.as_deref()),
        (Stage::YourChecks, Some("C1: it answers"))
    );
    let mut j = j;
    for _ in 0..MAX_REDEFINES {
        assert!(actions(&j).contains(&Action::Comment));
        (j, _) = go(
            &j,
            &Event::Commented {
                text: "fewer".into(),
            },
        );
        assert_eq!(j.stage, Stage::Define);
        (j, _) = go(&j, &reply("C1: it answers"));
    }
    assert_eq!(actions(&j), vec![Action::Approve, Action::Stop]);
    let (_, effects) = go(&j, &Event::Approved);
    let brief = match effects.as_slice() {
        [
            Effect::StartTurn {
                stage: Stage::Specify,
                brief,
                ..
            },
        ] => brief.clone(),
        other => panic!("{other:?}"),
    };
    assert!(brief.contains("C1: it answers"));
}

#[test]
fn a_spec_that_does_not_read_gets_one_retry_then_waits_on_the_owner() {
    let unusable = |report: &str| Event::TurnFinished {
        outcome: Outcome::Unusable {
            report: report.into(),
            problem: "not JSON".into(),
        },
    };
    let (j, effects) = go(&at(Stage::Specify, State::Running), &unusable("no json"));
    assert_eq!((j.stage, j.state), (Stage::Specify, State::Running));
    assert!(starts_turn(&effects, Stage::Specify));
    let (j, _) = go(&j, &unusable("still no json"));
    assert_eq!((j.stage, j.state), (Stage::Specify, State::NeedsYou));

    let (j, effects) = go(&at(Stage::Specify, State::Running), &reply(SPEC));
    assert_eq!(j.stage, Stage::Baseline);
    assert!(effects.contains(&Effect::RunStep {
        stage: Stage::Baseline
    }));
}

#[test]
fn green_checks_at_the_baseline_go_to_falsify_once_per_spec() {
    let (j, effects) = go(&at(Stage::Baseline, State::Running), &Event::ChecksGreen);
    assert_eq!(
        (j.stage, j.back_to, j.falsify_tried),
        (Stage::Falsify, Some(Stage::Baseline), true)
    );
    assert!(starts_turn(&effects, Stage::Falsify));
    let (j, effects) = go(&j, &done());
    assert_eq!(j.stage, Stage::Baseline);
    assert!(effects.contains(&Effect::RunStep {
        stage: Stage::Baseline
    }));
    let (j, _) = go(&j, &Event::ChecksGreen);
    assert_eq!(
        j.stage,
        Stage::Specify,
        "a second green baseline is the spec's problem"
    );
    let (j, _) = go(&j, &reply(SPEC));
    assert!(!j.falsify_tried, "a new spec may be falsified again");
}

#[test]
fn a_changed_test_at_verify_waits_on_the_owner_without_a_fix_turn() {
    let (j, effects) = go(
        &at(Stage::Verify, State::Running),
        &Event::StepStuck {
            detail: "changed".into(),
        },
    );
    assert_eq!(
        (j.stage, j.state, j.error.as_deref()),
        (Stage::Verify, State::NeedsYou, Some("changed"))
    );
    assert!(effects.is_empty());
}

#[test]
fn a_refactor_with_nothing_proposed_goes_to_acceptance_and_one_with_proposals_waits_for_a_pick() {
    let (j, effects) = go(&at(Stage::Refactor, State::Running), &reply("REFACTORS:\n"));
    assert_eq!(j.stage, Stage::Acceptance);
    assert!(effects.contains(&Effect::RunStep {
        stage: Stage::Acceptance
    }));
    let (j, _) = go(
        &at(Stage::Refactor, State::Running),
        &reply("REFACTORS:\n1. Extract the parser"),
    );
    assert_eq!((j.stage, j.state), (Stage::PickRefactors, State::NeedsYou));
}

#[test]
fn a_review_captures_the_diff_and_waits_for_a_pick_only_when_it_found_something() {
    let (j, effects) = go(
        &at(Stage::CodeReview, State::Running),
        &reply("FINDINGS:\n"),
    );
    assert_eq!(j.stage, Stage::Ship);
    assert!(effects.contains(&Effect::CaptureDiff));
    let (j, _) = go(
        &at(Stage::CodeReview, State::Running),
        &reply("FINDINGS:\n1. calc.py:3 divides by zero"),
    );
    assert_eq!((j.stage, j.state), (Stage::PickFindings, State::NeedsYou));
}

#[test]
fn a_list_reads_only_under_its_header() {
    let report = "TASKS:\n1. a\nREFACTORS:\n- Extract it\n2) Rename it\nNOTES:\n- not a refactor\n";
    assert_eq!(listed(report, "REFACTORS:"), ["Extract it", "Rename it"]);
    assert!(listed("nothing", "FINDINGS:").is_empty());
    let mut j = at(Stage::CodeReview, State::Running);
    j.request = "r".into();
    assert!(brief(&j).contains(&format!("git diff {BASE_TAG}")));
}
