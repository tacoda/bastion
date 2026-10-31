//! Every transition of the short pipeline, as plain function calls.

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
        current = step(&current.0, e, AT + 1 + i as u64);
    }
    current
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
    assert_eq!((j.stage, j.state), (Stage::Approve, State::NeedsYou));
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
fn review_approval_lands_and_landing_tears_down() {
    let mut events = planned();
    events.extend([Event::Approved, done("Added /health"), Event::Approved]);
    let (j, effects) = run(&events);
    assert_eq!(
        (j.stage, j.state, effects),
        (Stage::Land, State::Running, vec![Effect::Land])
    );
    events.push(Event::Landed);
    let (j, effects) = run(&events);
    assert_eq!((j.state, effects), (State::Landed, vec![Effect::Teardown]));
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
    let (_, effects) = step(&j, &Event::Approved, AT + 9);
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
    let (after, effects) = step(&j, &Event::Admitted, AT + 9);
    assert_eq!((after, effects), (j, Vec::new()));
}

#[test]
fn an_event_that_does_not_fit_changes_nothing() {
    let j = job();
    assert_eq!(step(&j, &Event::Approved, AT + 1), (j.clone(), Vec::new()));
    assert_eq!(step(&j, &done("x"), AT + 1), (j, Vec::new()));
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
