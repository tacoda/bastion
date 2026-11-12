//! The job state machine on workflows of steps (D39).

use super::*;
use crate::workflow::{Options, Reach, Step};

const AT: u64 = 1_000;

fn prompt(id: &str, dial: Dial) -> Step {
    Step {
        id: id.into(),
        stage: Stage::Prompt,
        run: String::new(),
        dial,
        note: "Do it.".into(),
        needs: Vec::new(),
        gives: Vec::new(),
        reach: Reach::default(),
        options: Options::default(),
        on_failure: None,
        failure_only: false,
    }
}

fn shell(id: &str, dial: Dial) -> Step {
    Step {
        stage: Stage::Shell,
        run: "make test".into(),
        ..prompt(id, dial)
    }
}

/// A running job on plan, build and tests, at its first step.
fn job(dials: [Dial; 3]) -> Job {
    let mut plan = prompt("plan", dials[0]);
    plan.reach.reads_only = true;
    plan.gives = vec!["plan".into()];
    let mut build = prompt("build", dials[1]);
    build.needs = vec!["plan".into()];
    let flow = vec![plan, build, shell("tests", dials[2])];
    let mut j = Job::new("j1".into(), "w".into(), "Add x".into(), AT).running(flow);
    j.state = State::Running;
    j
}

fn go(j: &Job, e: Event) -> (Job, Vec<Effect>) {
    step(j, &e, AT + 1, &Bounds::default())
}

fn done(report: &str) -> Event {
    Event::TurnFinished {
        outcome: Outcome::Done {
            report: report.into(),
        },
    }
}

fn kinds(effects: &[Effect]) -> Vec<&'static str> {
    effects
        .iter()
        .map(|e| match e {
            Effect::Provision => "provision",
            Effect::StartTurn { .. } => "turn",
            Effect::RecordArtifact { .. } => "artifact",
            Effect::CaptureDiff { .. } => "diff",
            Effect::RunStep { .. } => "run",
            Effect::Finish => "finish",
            Effect::Teardown => "teardown",
            Effect::DarkDecided { .. } => "dark",
            Effect::Review { .. } => "review",
        })
        .collect()
}

#[test]
fn a_job_runs_its_steps_waits_where_lit_and_finishes_past_the_last() {
    let j = job([Dial::LightsOn, Dial::Dark, Dial::Dark]);
    let (j, effects) = go(&j, done("RESULT plan:\n1. Add x."));
    assert_eq!(
        kinds(&effects),
        ["artifact"],
        "a step that only reads leaves no diff"
    );
    assert!(matches!(&effects[0], Effect::RecordArtifact { step, .. } if step == "plan"));
    assert_eq!(
        (j.state, j.results["plan"].as_str()),
        (State::NeedsYou, "1. Add x.")
    );
    let (j, effects) = go(&j, Event::Approved);
    assert_eq!(
        (j.node.as_deref(), kinds(&effects)),
        (Some("build"), vec!["turn"])
    );
    let Effect::StartTurn { brief, .. } = &effects[0] else {
        panic!()
    };
    assert!(
        brief.contains("<result name=\"plan\">\n1. Add x."),
        "{brief}"
    );
    let (j, effects) = go(&j, done("Built."));
    assert_eq!(kinds(&effects), ["artifact", "diff", "dark", "run"]);
    let (j, effects) = go(
        &j,
        Event::Ran {
            output: "3 passed".into(),
        },
    );
    assert_eq!(kinds(&effects), ["artifact", "dark", "finish"]);
    assert_eq!(
        (j.state, j.node.as_deref(), j.finishing()),
        (State::Running, None, true)
    );
    let (landed, effects) = go(&j, Event::Landed);
    assert_eq!(
        (landed.state, kinds(&effects)),
        (State::Landed, vec!["teardown"])
    );
}

#[test]
fn a_failed_finish_waits_and_an_approve_or_a_restart_finishes_again() {
    let mut j = job([Dial::Dark; 3]);
    j.node = None;
    let (stuck, _) = go(
        &j,
        Event::LandFailed {
            error: "conflict".into(),
        },
    );
    assert_eq!(
        (stuck.state, stuck.error.as_deref()),
        (State::NeedsYou, Some("conflict"))
    );
    let (again, effects) = go(&stuck, Event::Approved);
    assert_eq!(
        (again.state, kinds(&effects)),
        (State::Running, vec!["finish"])
    );
    assert_eq!(kinds(&restart(&again)), ["finish"]);
    let trial = Bounds {
        trial: true,
        ..Bounds::default()
    };
    let mut last = job([Dial::Dark; 3]);
    last.node = Some("tests".into());
    last.stage = Stage::Shell;
    let (stopped, _) = step(
        &last,
        &Event::Ran {
            output: "ok".into(),
        },
        AT,
        &trial,
    );
    assert_eq!(stopped.state, State::Stopped, "a trial lands nothing");
}

#[test]
fn a_failing_shell_step_goes_back_to_the_step_that_writes_then_waits() {
    let mut j = job([Dial::Dark; 3]);
    (j.node, j.stage) = (Some("tests".into()), Stage::Shell);
    j.results.insert("plan".into(), "1. Add x.".into());
    let (fixing, effects) = go(
        &j,
        Event::StepFailed {
            detail: "1 failed".into(),
        },
    );
    assert_eq!(fixing.node.as_deref(), Some("build"), "plan only reads");
    assert_eq!(kinds(&effects), ["artifact", "turn"]);
    assert!(
        matches!(&effects[0], Effect::RecordArtifact { step, .. } if step == "tests"),
        "the output is the failing step's, not the fixer's"
    );
    assert!(fixing.feedback.as_deref().unwrap().contains("1 failed"));
    let mut again = fixing.clone();
    (again.node, again.stage) = (Some("tests".into()), Stage::Shell);
    let (same, _) = go(
        &again,
        Event::StepFailed {
            detail: "1 failed".into(),
        },
    );
    assert_eq!(
        same.state,
        State::NeedsYou,
        "the same failure word for word waits"
    );
    let spent = Bounds {
        revision_budget: Some(1),
        ..Bounds::default()
    };
    let (budget, _) = step(
        &again,
        &Event::StepFailed {
            detail: "2 failed".into(),
        },
        AT,
        &spent,
    );
    assert!(
        budget
            .error
            .unwrap()
            .starts_with("The revision budget of 1 is spent.")
    );
}

#[test]
fn a_note_sends_the_work_back_and_an_answer_goes_to_the_step_that_asked() {
    let mut j = job([Dial::LightsOn; 3]);
    (j.node, j.stage, j.state) = (Some("tests".into()), Stage::Shell, State::NeedsYou);
    j.results.insert("plan".into(), "1. Add x.".into());
    let (back, effects) = go(
        &j,
        Event::Commented {
            text: "Name it y.".into(),
        },
    );
    assert_eq!(
        (back.node.as_deref(), kinds(&effects)),
        (Some("build"), vec!["turn"])
    );
    assert_eq!(back.feedback.as_deref(), Some("Name it y."));
    let asked = Event::TurnFinished {
        outcome: Outcome::Asked {
            question: "Which file?".into(),
        },
    };
    let (waits, _) = go(&job([Dial::Dark; 3]), asked);
    assert_eq!(actions(&waits), [Action::Answer, Action::Stop]);
    let (answered, _) = go(
        &waits,
        Event::Commented {
            text: "calc.py".into(),
        },
    );
    assert_eq!(answered.node.as_deref(), Some("plan"));
    assert!(answered.feedback.unwrap().contains("The answer: calc.py"));
}

#[test]
fn a_reply_without_its_result_gets_one_more_try_then_waits() {
    let j = job([Dial::Dark; 3]);
    let (retry, effects) = go(&j, done("No plan here."));
    assert_eq!(
        (retry.state, kinds(&effects)),
        (State::Running, vec!["artifact", "turn"])
    );
    assert!(retry.feedback.unwrap().contains("RESULT plan:"));
    let (j, _) = go(&j, done("No plan here."));
    let (waits, _) = go(&j, done("Still none."));
    assert_eq!(waits.state, State::NeedsYou);
}

#[test]
fn a_step_shows_where_it_stands_and_a_queue_admits_by_its_limits() {
    let mut j = job([Dial::Dark; 3]);
    j.node = Some("build".into());
    let shown: Vec<StepState> = steps(&j).into_iter().map(|s| s.2).collect();
    assert_eq!(
        shown,
        [StepState::Done, StepState::Running, StepState::Pending]
    );
    let queued = |id: &str, w: &str| Job::new(id.into(), w.into(), "r".into(), AT);
    let jobs = [queued("a", "w"), queued("b", "w"), queued("c", "v")];
    assert_eq!(admit(&jobs, 2, |w| (w == "w").then_some(1)), ["a", "c"]);
}

#[test]
fn a_step_sets_how_often_its_failure_sends_work_back() {
    let mut j = job([Dial::Dark; 3]);
    (j.node, j.stage) = (Some("tests".into()), Stage::Shell);
    j.results.insert("plan".into(), "1. Add x.".into());
    j.flow[2].options.revisions = Some(0);
    let failed = Event::StepFailed {
        detail: "1 failed".into(),
    };
    let (none, _) = go(&j, failed.clone());
    assert_eq!(
        (none.state, none.node.as_deref()),
        (State::NeedsYou, Some("tests")),
        "no revision allowed, so it waits"
    );
    j.flow[2].options.revisions = Some(5);
    j.revisions.insert("tests".into(), 3);
    let (more, _) = go(&j, failed);
    assert_eq!(
        more.node.as_deref(),
        Some("build"),
        "past the default of two"
    );
}

#[test]
fn a_spent_step_starts_no_more_turns_by_itself_but_a_person_may_send_it_back() {
    let mut j = job([Dial::Dark; 3]);
    (j.node, j.stage) = (Some("tests".into()), Stage::Shell);
    j.results.insert("plan".into(), "1. Add x.".into());
    j.flow[1].options.spend_micros = Some(10_000);
    let spent = Bounds {
        spent: [("build".to_string(), 12_000)].into(),
        ..Bounds::default()
    };
    let failed = Event::StepFailed {
        detail: "1 failed".into(),
    };
    let (waits, effects) = step(&j, &failed, AT, &spent);
    assert_eq!(
        (waits.state, waits.node.as_deref(), kinds(&effects)),
        (State::NeedsYou, Some("build"), vec!["artifact"])
    );
    assert!(
        waits
            .error
            .as_deref()
            .unwrap()
            .starts_with("This step's budget of $0.0100 is spent: its turns cost $0.0120."),
        "{:?}",
        waits.error
    );
    let note = Event::Commented {
        text: "Try once more.".into(),
    };
    let (again, effects) = step(&waits, &note, AT, &spent);
    assert_eq!(
        (again.state, kinds(&effects)),
        (State::Running, vec!["turn"])
    );
    let under = Bounds {
        spent: [("build".to_string(), 9_000)].into(),
        ..Bounds::default()
    };
    let (fixing, _) = step(&j, &failed, AT, &under);
    assert_eq!(fixing.state, State::Running, "under its budget, it runs");
}

#[test]
fn a_failure_goes_along_its_link_and_a_step_only_a_failure_reaches_is_skipped() {
    let mut j = job([Dial::Dark; 3]);
    // plan, build, tests, then notify, which only a failure of tests reaches.
    j.flow.push(shell("notify", Dial::Dark));
    j.flow[2].on_failure = Some("notify".into());
    j.flow[3].failure_only = true;
    (j.node, j.stage) = (Some("tests".into()), Stage::Shell);
    j.results.insert("plan".into(), "1. Add x.".into());
    let (branched, effects) = go(
        &j,
        Event::StepFailed {
            detail: "1 failed".into(),
        },
    );
    assert_eq!(
        (branched.node.as_deref(), kinds(&effects)),
        (Some("notify"), vec!["artifact", "run"]),
        "past build, which would otherwise fix it"
    );
    let shown = |j: &Job| -> Vec<StepState> { steps(j).into_iter().map(|s| s.2).collect() };
    assert_eq!(
        shown(&branched)[2..],
        [StepState::Failed, StepState::Running],
        "tests failed; notify runs"
    );
    assert!(
        branched
            .feedback
            .unwrap()
            .starts_with("The step tests failed, and its workflow sends the failure here.")
    );
    let (passed, _) = go(
        &j,
        Event::Ran {
            output: "3 passed".into(),
        },
    );
    assert_eq!(
        (passed.node.clone(), passed.finishing()),
        (None, true),
        "a pass skips notify and finishes"
    );
    assert_eq!(
        shown(&passed)[2..],
        [StepState::Running, StepState::Skipped],
        "the finish stands on tests, and notify was passed by"
    );
}

#[test]
fn a_failure_link_back_loops_until_the_step_runs_out_of_revisions() {
    let mut j = job([Dial::Dark; 3]);
    j.flow[2].on_failure = Some("plan".into());
    j.flow[2].options.revisions = Some(1);
    (j.node, j.stage) = (Some("tests".into()), Stage::Shell);
    j.results.insert("plan".into(), "1. Add x.".into());
    let failed = |detail: &str| Event::StepFailed {
        detail: detail.into(),
    };
    let (back, _) = go(&j, failed("1 failed"));
    assert_eq!(
        back.node.as_deref(),
        Some("plan"),
        "back to plan, not build"
    );
    let mut again = back.clone();
    (again.node, again.stage, again.state) = (Some("tests".into()), Stage::Shell, State::Running);
    let (waits, _) = go(&again, failed("2 failed"));
    assert_eq!(
        (waits.state, waits.node.as_deref()),
        (State::NeedsYou, Some("tests")),
        "one revision, then a person decides"
    );
}

/// The build step of a dark job, with `reviewers` on it.
fn reviewed_build() -> (Job, Bounds) {
    let mut j = job([Dial::Dark; 3]);
    (j.node, j.stage) = (Some("build".into()), Stage::Prompt);
    j.results.insert("plan".into(), "1. Add x.".into());
    let bounds = Bounds {
        reviewers: vec!["security".into()],
        ..Bounds::default()
    };
    (j, bounds)
}

#[test]
fn a_step_with_a_reviewer_waits_for_its_verdict() {
    let (j, bounds) = reviewed_build();
    let (waiting, effects) = step(&j, &done("Added x."), AT, &bounds);
    assert_eq!(kinds(&effects), ["artifact", "diff", "review"]);
    assert_eq!(
        waiting.node.as_deref(),
        Some("build"),
        "it waits at the step"
    );
    let pass = Event::Reviewed {
        verdict: review::Verdict::Pass,
    };
    let (on, _) = step(&waiting, &pass, AT, &bounds);
    assert_eq!(on.node.as_deref(), Some("tests"), "a pass goes on");
}

#[test]
fn a_failed_review_sends_the_change_back_and_a_broken_one_waits() {
    let (j, bounds) = reviewed_build();
    let (waiting, _) = step(&j, &done("Added x."), AT, &bounds);
    let fail = |detail: &str| Event::Reviewed {
        verdict: review::Verdict::Fail {
            detail: detail.into(),
        },
    };
    let (back, effects) = step(&waiting, &fail("No test."), AT, &bounds);
    assert_eq!(kinds(&effects), ["turn"], "the build goes again");
    assert!(
        back.feedback
            .as_deref()
            .unwrap()
            .contains("failed its review")
    );
    let broken = Event::Reviewed {
        verdict: review::Verdict::Broken {
            error: "no agent file".into(),
        },
    };
    let (held, _) = step(&waiting, &broken, AT, &bounds);
    assert_eq!(held.state, State::NeedsYou);
}
