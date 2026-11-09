//! Every transition of the short pipeline, as plain function calls. Every
//! dial is lights-on unless a test sets it (D33).

use super::*;

const AT: u64 = 1_000;

fn job() -> Job {
    Job::new(
        "j1".into(),
        "bastion".into(),
        "Add a health check".into(),
        AT,
    )
}

fn done(report: &str) -> Event {
    Event::TurnFinished {
        outcome: Outcome::Done {
            report: report.into(),
        },
    }
}

fn failed() -> Event {
    Event::TurnFinished {
        outcome: Outcome::Failed {
            error: "timeout".into(),
        },
    }
}

/// Run events in order, and return the job and the last step's effects.
fn run(events: &[Event]) -> (Job, Vec<Effect>) {
    let mut current = (job(), Vec::new());
    for (i, e) in events.iter().enumerate() {
        current = step(&current.0, e, AT + 1 + i as u64, &Dials::default());
    }
    current
}

/// A step's result, kept as its artifact.
fn result(stage: Stage, body: &str) -> Effect {
    Effect::RecordArtifact {
        stage,
        body: body.into(),
    }
}

fn planned() -> Vec<Event> {
    vec![Event::Admitted, Event::Provisioned, done("1. Add /health")]
}

#[test]
fn a_new_job_waits_in_the_queue_at_plan() {
    let j = job();
    assert_eq!((j.state, j.stage), (State::Queued, Stage::Plan));
}

#[test]
fn admission_provisions_and_provisioning_starts_a_read_only_plan_turn() {
    let (j, effects) = run(&[Event::Admitted]);
    assert_eq!(
        (j.state, effects),
        (State::Provisioning, vec![Effect::Provision])
    );
    let (j, effects) = run(&[Event::Admitted, Event::Provisioned]);
    assert_eq!(j.state, State::Running);
    assert!(matches!(
        &effects[..],
        [Effect::StartTurn {
            stage: Stage::Plan,
            read_only: true,
            cap_secs: TURN_CAP_SECS,
            ..
        }]
    ));
}

#[test]
fn a_finished_plan_is_recorded_and_waits_for_approval() {
    let (j, effects) = run(&planned());
    assert_eq!((j.stage, j.state), (Stage::Plan, State::NeedsYou));
    assert_eq!(j.plan.as_deref(), Some("1. Add /health"));
    assert_eq!(
        effects,
        vec![Effect::RecordArtifact {
            stage: Stage::Plan,
            body: "1. Add /health".into()
        }]
    );
}

#[test]
fn approval_starts_a_build_that_may_write_and_sees_the_plan() {
    let mut events = planned();
    events.push(Event::Approved);
    let (j, effects) = run(&events);
    assert_eq!((j.stage, j.state), (Stage::Build, State::Running));
    match &effects[..] {
        [
            Effect::StartTurn {
                stage: Stage::Build,
                read_only: false,
                brief,
                ..
            },
        ] => {
            assert!(brief.contains("1. Add /health"), "{brief}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_comment_at_approval_replans_with_the_comment_verbatim() {
    let mut events = planned();
    events.push(Event::Commented {
        text: "Use the existing router.".into(),
    });
    let (j, effects) = run(&events);
    assert_eq!((j.stage, j.state), (Stage::Plan, State::Running));
    match &effects[..] {
        [Effect::StartTurn { brief, .. }] => {
            assert!(brief.contains("Use the existing router."), "{brief}")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_comment_at_review_rebuilds() {
    let mut events = planned();
    events.extend([
        Event::Approved,
        done("Added /health"),
        Event::Commented {
            text: "Add a test.".into(),
        },
    ]);
    let (j, _) = run(&events);
    assert_eq!((j.stage, j.state), (Stage::Build, State::Running));
}

#[test]
fn the_land_waits_before_it_acts_then_lands_and_tears_down() {
    let mut events = planned();
    events.extend([Event::Approved, done("Added /health"), Event::Approved]);
    let (j, effects) = run(&events);
    assert_eq!(
        (j.stage, j.state, effects),
        (Stage::Land, State::NeedsYou, vec![])
    );
    events.push(Event::Approved);
    let (j, effects) = run(&events);
    assert_eq!(
        (j.stage, j.state, effects),
        (Stage::Land, State::Running, vec![Effect::Land])
    );
    events.push(Event::Landed);
    let (j, effects) = run(&events);
    assert_eq!(
        (j.state, effects),
        (
            State::Landed,
            vec![result(Stage::Land, "Landed."), Effect::Teardown]
        )
    );
}

#[test]
fn a_question_waits_on_the_owner_and_the_answer_goes_back_to_the_same_stage() {
    let mut events = vec![
        Event::Admitted,
        Event::Provisioned,
        Event::TurnFinished {
            outcome: Outcome::Asked {
                question: "Which port?".into(),
            },
        },
    ];
    let (j, _) = run(&events);
    assert_eq!(
        (j.stage, j.state, j.question.as_deref()),
        (Stage::Plan, State::NeedsYou, Some("Which port?"))
    );
    let (_, effects) = step(&j, &Event::Approved, AT + 9, &Dials::default());
    assert!(
        effects.is_empty(),
        "a question needs an answer, not an approval"
    );
    events.push(Event::Commented {
        text: "8080".into(),
    });
    let (j, effects) = run(&events);
    assert_eq!(
        (j.stage, j.state, j.question),
        (Stage::Plan, State::Running, None)
    );
    match &effects[..] {
        [Effect::StartTurn { brief, .. }] => {
            assert!(
                brief.contains("You asked: Which port?\nThe answer: 8080"),
                "{brief}"
            );
            assert!(brief.contains("8080"), "{brief}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_failed_turn_is_retried_once_then_fails_the_job() {
    let (j, effects) = run(&[Event::Admitted, Event::Provisioned, failed()]);
    assert_eq!((j.state, j.failures), (State::Running, 1));
    assert!(matches!(&effects[..], [Effect::StartTurn { .. }]));
    let (j, effects) = run(&[Event::Admitted, Event::Provisioned, failed(), failed()]);
    assert_eq!(
        (j.state, j.error.as_deref()),
        (State::Failed, Some("timeout"))
    );
    assert_eq!(effects, vec![Effect::Teardown]);
}

#[test]
fn stop_tears_down_from_any_live_state_and_a_finished_job_ignores_everything() {
    let (j, effects) = run(&[Event::Admitted, Event::Stopped]);
    assert_eq!((j.state, effects), (State::Stopped, vec![Effect::Teardown]));
    let (after, effects) = step(&j, &Event::Admitted, AT + 9, &Dials::default());
    assert_eq!((after, effects), (j, Vec::new()));
}

#[test]
fn an_event_that_does_not_fit_changes_nothing() {
    let j = job();
    assert_eq!(
        step(&j, &Event::Approved, AT + 1, &Dials::default()),
        (j.clone(), Vec::new())
    );
    assert_eq!(
        step(&j, &done("x"), AT + 1, &Dials::default()),
        (j, Vec::new())
    );
}

#[test]
fn admission_fills_free_slots_oldest_first() {
    let mut a = Job::new("a".into(), "w".into(), "r".into(), 30);
    let b = Job::new("b".into(), "w".into(), "r".into(), 10);
    let c = Job::new("c".into(), "w".into(), "r".into(), 20);
    assert_eq!(admit(&[a.clone(), b.clone(), c.clone()], 2), vec!["b", "c"]);
    a.state = State::Running;
    assert_eq!(admit(&[a, b, c], 2), vec!["b"]);
}

#[test]
fn a_report_that_starts_with_the_marker_is_a_question() {
    assert_eq!(
        outcome("  INTERRUPT: Which database? \n"),
        Outcome::Asked {
            question: "Which database?".into()
        }
    );
    assert_eq!(
        outcome("All done.\n"),
        Outcome::Done {
            report: "All done.".into()
        }
    );
}

#[test]
fn the_request_is_quoted_as_data_in_every_brief() {
    let b = brief(&job());
    assert!(
        b.contains("<request>\nAdd a health check\n</request>"),
        "{b}"
    );
    assert!(b.contains(QUESTION_MARKER), "{b}");
}

#[test]
fn a_checkpoint_allows_approve_comment_and_stop_and_a_question_allows_an_answer() {
    let (j, _) = run(&planned());
    assert_eq!(
        actions(&j),
        vec![Action::Approve, Action::Comment, Action::Stop]
    );
    let (j, _) = run(&[
        Event::Admitted,
        Event::Provisioned,
        Event::TurnFinished {
            outcome: Outcome::Asked {
                question: "?".into(),
            },
        },
    ]);
    assert_eq!(actions(&j), vec![Action::Answer, Action::Stop]);
    let (j, _) = run(&[Event::Stopped]);
    assert!(actions(&j).is_empty());
}

#[test]
fn steps_show_done_before_the_current_stage_and_pending_after_it() {
    let mut events = planned();
    events.extend([Event::Approved]);
    let (j, _) = run(&events);
    let states: Vec<StepState> = steps(&j).into_iter().map(|(_, s)| s).collect();
    assert_eq!(
        states,
        [StepState::Done, StepState::Running, StepState::Pending]
    );
    let (j, _) = run(&planned());
    assert_eq!(steps(&j)[0], (Stage::Plan, StepState::Waiting));
    let (j, _) = run(&[
        Event::Admitted,
        Event::ProvisionFailed { error: "x".into() },
    ]);
    assert_eq!(steps(&j)[0], (Stage::Plan, StepState::Failed));
}

#[test]
fn a_landed_job_shows_every_step_done() {
    let mut events = planned();
    events.extend([
        Event::Approved,
        done("built"),
        Event::Approved,
        Event::Approved,
        Event::Landed,
    ]);
    let (j, _) = run(&events);
    assert!(steps(&j).iter().all(|(_, s)| *s == StepState::Done));
}

#[test]
fn a_finished_build_records_its_report_and_captures_the_diff_for_review() {
    let mut events = planned();
    events.extend([Event::Approved, done("Added /health")]);
    let (j, effects) = run(&events);
    assert_eq!((j.stage, j.state), (Stage::Build, State::NeedsYou));
    assert_eq!(
        effects,
        vec![
            Effect::RecordArtifact {
                stage: Stage::Build,
                body: "Added /health".into()
            },
            Effect::CaptureDiff,
        ]
    );
}

fn run_with(dials: Dials, events: &[Event]) -> (Job, Vec<Effect>) {
    let mut current = (job(), Vec::new());
    for (i, e) in events.iter().enumerate() {
        current = step(&current.0, e, AT + 1 + i as u64, &dials);
    }
    current
}

/// Every step lights-on but `stage`, which is at `dial`.
fn one(stage: Stage, dial: Dial) -> Dials {
    let mut d = Dials::default();
    d.set(stage, dial);
    d
}

#[test]
fn a_dark_plan_goes_straight_to_build() {
    let (j, effects) = run_with(one(Stage::Plan, Dial::Dark), &planned());
    assert_eq!((j.stage, j.state), (Stage::Build, State::Running));
    assert!(matches!(
        &effects[..],
        [
            Effect::RecordArtifact { .. },
            Effect::DarkDecided { stage: Stage::Plan },
            Effect::StartTurn {
                stage: Stage::Build,
                ..
            }
        ]
    ));
}

#[test]
fn with_every_dial_dark_a_request_lands_alone() {
    let events = [
        Event::Admitted,
        Event::Provisioned,
        done("plan"),
        done("built"),
    ];
    let (j, effects) = run_with(Dials::all(Dial::Dark), &events);
    assert_eq!(
        (j.stage, j.state, effects.last()),
        (Stage::Land, State::Running, Some(&Effect::Land))
    );
    let (j, _) = step(&j, &Event::Landed, AT + 9, &Dials::all(Dial::Dark));
    assert_eq!(j.state, State::Landed);
}

#[test]
fn a_failed_land_waits_on_the_owner_with_its_vm_and_approve_retries_it() {
    let mut events = planned();
    events.extend([
        Event::Approved,
        done("built"),
        Event::Approved,
        Event::Approved,
        Event::LandFailed {
            error: "no ref".into(),
        },
    ]);
    let (j, effects) = run(&events);
    assert_eq!(
        (j.stage, j.state, j.error.as_deref()),
        (Stage::Land, State::NeedsYou, Some("no ref"))
    );
    assert_eq!(
        effects,
        vec![result(Stage::Land, "no ref")],
        "the VM stays: no teardown"
    );
    assert_eq!(actions(&j), vec![Action::Approve, Action::Stop]);
    events.push(Event::Approved);
    let (j, effects) = run(&events);
    assert_eq!(
        (j.state, j.error, effects),
        (State::Running, None, vec![Effect::Land])
    );
}

#[test]
fn a_trial_approves_its_plan_and_stops_at_review_with_its_diff() {
    let trial = Dials {
        trial: true,
        ..Dials::default()
    };
    let mut j = job();
    for (i, e) in [Event::Admitted, Event::Provisioned, done("1. plan")]
        .iter()
        .enumerate()
    {
        j = step(&j, e, AT + i as u64, &trial).0;
    }
    assert_eq!(
        (j.stage, j.state),
        (Stage::Build, State::Running),
        "the plan approved itself"
    );
    let (j, effects) = step(&j, &done("built"), AT + 9, &trial);
    assert_eq!((j.stage, j.state), (Stage::Review, State::Stopped));
    let diff = effects
        .iter()
        .position(|e| *e == Effect::CaptureDiff)
        .unwrap();
    let teardown = effects.iter().position(|e| *e == Effect::Teardown).unwrap();
    assert!(diff < teardown, "the diff is taken before the VM goes");
}

/// A job at land, as the adapter leaves it after pushing its pull request.
fn opened() -> Job {
    let mut j = run(&[
        Event::Admitted,
        Event::Provisioned,
        done("plan"),
        Event::Approved,
        done("built"),
        Event::Approved,
        Event::Approved,
    ])
    .0;
    assert_eq!((j.stage, j.state), (Stage::Land, State::Running));
    let (next, effects) = step(
        &j,
        &Event::PrOpened {
            url: "https://github.com/o/r/pull/1".into(),
        },
        AT + 20,
        &Dials::default(),
    );
    assert_eq!(effects.last(), Some(&Effect::WatchCi));
    j = next;
    j
}

#[test]
fn a_passing_pull_request_waits_for_the_owner_and_approve_merges() {
    let j = opened();
    let (j, effects) = step(&j, &Event::CiPassed, AT + 21, &Dials::default());
    assert_eq!(
        (j.state, effects),
        (State::NeedsYou, vec![result(Stage::Land, "Passed.")])
    );
    assert!(actions(&j).contains(&Action::Approve));
    let (j, effects) = step(&j, &Event::Approved, AT + 22, &Dials::default());
    assert_eq!((j.state, effects), (State::Running, vec![Effect::Merge]));
    let (j, effects) = step(&j, &Event::Landed, AT + 23, &Dials::default());
    assert_eq!(
        (j.state, effects),
        (
            State::Landed,
            vec![result(Stage::Land, "Landed."), Effect::Teardown]
        )
    );
}

#[test]
fn a_failing_ci_run_sends_a_bounded_fix_turn_with_its_log() {
    let mut j = opened();
    for fix in 1..=MAX_CI_FIXES {
        let (next, effects) = step(
            &j,
            &Event::CiFailed {
                log: "CHANGELOG.md has no entry".into(),
            },
            AT + 30,
            &Dials::default(),
        );
        assert_eq!(
            (next.stage, next.state, next.ci_fixes),
            (Stage::Build, State::Running, fix)
        );
        assert!(
            next.feedback
                .as_deref()
                .unwrap()
                .contains("CHANGELOG.md has no entry")
        );
        let Some(Effect::StartTurn { brief, .. }) = effects.last() else {
            panic!("a fix turn starts")
        };
        assert!(brief.contains("CHANGELOG.md has no entry"), "{brief}");
        // The fix waits on the owner again, then the land pushes to the same
        // pull request.
        let mut back = next;
        for e in [done("fixed"), Event::Approved, Event::Approved] {
            back = step(&back, &e, AT + 31, &Dials::default()).0;
        }
        assert_eq!((back.stage, back.pr.is_some()), (Stage::Land, true));
        j = step(
            &back,
            &Event::PrOpened {
                url: "https://github.com/o/r/pull/1".into(),
            },
            AT + 32,
            &Dials::default(),
        )
        .0;
    }
    let (j, _) = step(
        &j,
        &Event::CiFailed {
            log: "still".into(),
        },
        AT + 40,
        &Dials::default(),
    );
    assert_eq!(j.state, State::Failed);
}

#[test]
fn a_turns_result_at_a_step_that_runs_no_turn_is_ignored() {
    let (mut verifying, _) = run(&[Event::Admitted, Event::Provisioned]);
    verifying.stage = Stage::Verify;
    let (after, effects) = step(
        &verifying,
        &done("an old build's reply"),
        AT,
        &Dials::default(),
    );
    assert_eq!(after, verifying);
    assert!(effects.is_empty());
}

#[test]
fn a_running_step_starts_again_without_moving() {
    let (planning, _) = run(&[Event::Admitted, Event::Provisioned]);
    let again = restart(&planning, &Dials::default());
    assert!(matches!(
        &again[..],
        [Effect::StartTurn {
            stage: Stage::Plan,
            ..
        }]
    ));
    let mut landing = planning.clone();
    landing.stage = Stage::Land;
    assert_eq!(restart(&landing, &Dials::default()), vec![Effect::Land]);
    let (waiting, _) = run(&[Event::Admitted, Event::Provisioned, done("plan")]);
    assert!(restart(&waiting, &Dials::default()).is_empty());
}

#[test]
fn a_part_whose_earlier_part_failed_is_abandoned() {
    let mut first = job();
    first.state = State::Failed;
    let mut part = job();
    (part.id, part.after) = ("j1-2".into(), Some("j1".into()));
    assert_eq!(orphans(&[first.clone(), part.clone()]), ["j1-2"]);
    first.state = State::Landed;
    assert!(orphans(&[first, part.clone()]).is_empty());
    let (abandoned, _) = step(
        &part,
        &Event::Abandoned {
            error: "the part before it failed".into(),
        },
        AT,
        &Dials::default(),
    );
    assert_eq!(abandoned.state, State::Failed);
}
