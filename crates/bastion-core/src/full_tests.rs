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
