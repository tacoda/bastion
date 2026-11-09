//! The full pipeline's transitions, as plain function calls.
//! `go` runs with every dial dark, so a test sees the flow; `judge` runs
//! with every dial lights-on, so a test sees the waits (D33).

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

/// Every dial dark, without the audit notes and the step results that
/// dark steps make (`a_dark_step_notes_its_decision_and_keeps_its_result`).
fn go(j: &Job, e: &Event) -> (Job, Vec<Effect>) {
    let (next, effects) = step(j, e, AT + 1, &Dials::all(Dial::Dark));
    let flow = |e: &Effect| match e {
        Effect::DarkDecided { .. } => false,
        Effect::RecordArtifact { stage, .. } => stage.kind() != Kind::Factory,
        _ => true,
    };
    (next, effects.into_iter().filter(flow).collect())
}

fn judge(j: &Job, e: &Event) -> (Job, Vec<Effect>) {
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
fn without_acceptance_tests_the_plan_goes_to_implement() {
    let mut j = job(Pipeline::FullNoAcceptance);
    j.stage = Stage::Plan;
    j.state = State::Running;
    let (j, effects) = go(&j, &done());
    assert_eq!(j.stage, Stage::Implement);
    assert!(starts_turn(&effects, Stage::Implement));
}

#[test]
fn each_step_obeys_its_own_dial_and_a_step_that_acts_waits_before_it_acts() {
    let mut dials = Dials::default();
    dials.set(Stage::Discover, Dial::Dark);
    let (j, effects) = step(&at(Stage::Discover, State::Running), &done(), AT, &dials);
    assert_eq!(j.stage, Stage::Plan, "a dark discover goes on");
    assert!(starts_turn(&effects, Stage::Plan));

    let (j, effects) = step(&at(Stage::Comprehend, State::Running), &done(), AT, &dials);
    assert_eq!((j.stage, j.state), (Stage::Comprehend, State::NeedsYou));
    assert!(
        effects
            .iter()
            .all(|e| matches!(e, Effect::RecordArtifact { .. }))
    );
    let (j, effects) = step(&j, &Event::Approved, AT, &dials);
    assert_eq!(
        (j.stage, j.state, effects),
        (Stage::Merge, State::NeedsYou, vec![])
    );
    let (j, effects) = step(&j, &Event::Approved, AT, &dials);
    assert_eq!((j.state, effects), (State::Running, vec![Effect::Merge]));

    dials.set(Stage::Merge, Dial::Dark);
    let (_, effects) = step(
        &at(Stage::Comprehend, State::NeedsYou),
        &Event::Approved,
        AT,
        &dials,
    );
    assert_eq!(
        effects,
        vec![
            Effect::DarkDecided {
                stage: Stage::Merge
            },
            Effect::Merge
        ],
        "a dark merge merges alone, and says so"
    );
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
fn notes_on_refactor_review_and_comprehend_send_the_work_to_implement() {
    let note = Event::Commented {
        text: "Extract the parser".into(),
    };
    let (j, _) = judge(&at(Stage::Refactor, State::NeedsYou), &note);
    assert_eq!(
        (j.stage, j.back_to),
        (Stage::Implement, Some(Stage::Acceptance))
    );
    let (j, _) = judge(&at(Stage::CodeReview, State::NeedsYou), &note);
    assert_eq!(
        (j.stage, j.back_to),
        (Stage::Implement, Some(Stage::Checks))
    );

    let mut uat = at(Stage::Comprehend, State::NeedsYou);
    uat.revisions.insert(Stage::Checks, 2);
    let (j, _) = judge(&uat, &note);
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
fn discovery_with_questions_waits_and_the_answers_reach_the_plan() {
    let (j, effects) = judge(
        &at(Stage::Discover, State::Running),
        &Event::TurnFinished {
            outcome: Outcome::Done {
                report: DISCOVERY.into(),
            },
        },
    );
    assert_eq!((j.stage, j.state), (Stage::Discover, State::NeedsYou));
    assert_eq!(j.tasks.len(), 2);
    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::RecordArtifact {
            stage: Stage::Discover,
            ..
        }
    )));
    let (j, effects) = judge(
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
fn a_dark_discovery_goes_on_to_the_plan_with_its_questions_open() {
    let (j, _) = go(&at(Stage::Discover, State::Running), &reply(DISCOVERY));
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
    let (j, _) = judge(&at(Stage::Define, State::Running), &reply("C1: it answers"));
    assert_eq!(
        (j.stage, j.state, j.criteria.as_deref()),
        (Stage::Define, State::NeedsYou, Some("C1: it answers"))
    );
    let mut j = j;
    for _ in 0..MAX_REDEFINES {
        assert!(actions(&j).contains(&Action::Comment));
        (j, _) = judge(
            &j,
            &Event::Commented {
                text: "fewer".into(),
            },
        );
        assert_eq!((j.stage, j.state), (Stage::Define, State::Running));
        (j, _) = judge(&j, &reply("C1: it answers"));
    }
    assert_eq!(actions(&j), vec![Action::Approve, Action::Stop]);
    let (_, effects) = judge(&j, &Event::Approved);
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
fn a_refactor_waits_for_a_pick_unless_its_dial_is_dark() {
    let (j, effects) = go(&at(Stage::Refactor, State::Running), &reply("REFACTORS:\n"));
    assert_eq!(j.stage, Stage::Acceptance);
    assert!(effects.contains(&Effect::RunStep {
        stage: Stage::Acceptance
    }));
    let (j, _) = judge(
        &at(Stage::Refactor, State::Running),
        &reply("REFACTORS:\n1. Extract the parser"),
    );
    assert_eq!((j.stage, j.state), (Stage::Refactor, State::NeedsYou));
}

#[test]
fn a_review_captures_the_diff_and_waits_for_a_pick_unless_its_dial_is_dark() {
    let (j, effects) = go(
        &at(Stage::CodeReview, State::Running),
        &reply("FINDINGS:\n"),
    );
    assert_eq!(j.stage, Stage::Ship);
    assert!(effects.contains(&Effect::CaptureDiff));
    let (j, _) = judge(
        &at(Stage::CodeReview, State::Running),
        &reply("FINDINGS:\n1. calc.py:3 divides by zero"),
    );
    assert_eq!((j.stage, j.state), (Stage::CodeReview, State::NeedsYou));
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

#[test]
fn prove_goes_on_on_yes_waits_on_partial_and_sends_no_back_once() {
    let (j, effects) = go(
        &at(Stage::Prove, State::Running),
        &reply("tests pass\nVERDICT: yes"),
    );
    assert_eq!(j.stage, Stage::Comprehend);
    assert!(starts_turn(&effects, Stage::Comprehend));

    let (j, _) = go(
        &at(Stage::Prove, State::Running),
        &reply("VERDICT: partial"),
    );
    assert_eq!((j.stage, j.state), (Stage::Prove, State::NeedsYou));
    let (j, _) = go(&j, &Event::Approved);
    assert_eq!(j.stage, Stage::Comprehend, "the owner may go on anyway");

    let (j, _) = go(
        &at(Stage::Prove, State::Running),
        &reply("1 failed\nVERDICT: no"),
    );
    assert_eq!(
        (j.stage, j.back_to),
        (Stage::Implement, Some(Stage::Checks))
    );
    let mut again = j;
    again.stage = Stage::Prove;
    again.state = State::Running;
    let (j, _) = go(&again, &reply("still failing\nVERDICT: no"));
    assert_eq!(j.state, State::NeedsYou, "prove sends the work back once");
}

#[test]
fn with_no_pull_request_ship_passes_straight_through_ci() {
    let (j, effects) = go(&at(Stage::Ship, State::Running), &Event::StepPassed);
    assert_eq!(j.stage, Stage::Prove);
    assert!(starts_turn(&effects, Stage::Prove));
}

#[test]
fn a_lost_vm_is_provisioned_again_and_its_stage_starts_over() {
    let (j, effects) = go(&at(Stage::Implement, State::Running), &Event::VmLost);
    assert_eq!((j.stage, j.state), (Stage::Implement, State::Provisioning));
    assert_eq!(effects, vec![Effect::Provision]);
    let (j, effects) = go(&j, &Event::Provisioned);
    assert_eq!((j.stage, j.state), (Stage::Implement, State::Running));
    assert!(starts_turn(&effects, Stage::Implement));
    let mut j = j;
    for _ in 0..MAX_REVIVES {
        j.state = State::Running;
        (j, _) = go(&j, &Event::VmLost);
    }
    assert_eq!(
        j.state,
        State::Failed,
        "a VM that keeps dying fails the job"
    );
}

#[test]
fn a_dark_step_notes_its_decision_and_keeps_its_result() {
    let checks = at(Stage::Checks, State::Running);
    let passed = Effect::RecordArtifact {
        stage: Stage::Checks,
        body: "Passed.".into(),
    };
    let (_, effects) = step(&checks, &Event::StepPassed, AT, &Dials::all(Dial::Dark));
    assert_eq!(
        effects,
        [
            passed.clone(),
            Effect::DarkDecided {
                stage: Stage::Checks
            },
            Effect::RunStep {
                stage: Stage::Verify
            },
        ]
    );
    let (j, effects) = judge(&checks, &Event::StepPassed);
    assert_eq!((j.state, effects), (State::NeedsYou, vec![passed]));
}

/// Plan gives `design-notes` into build; review is left out.
fn notes_flow() -> Job {
    use crate::workflow::Graph;
    let mut g = Graph::chain(&[Stage::Plan, Stage::Build, Stage::Land], |_| Dial::Dark);
    g.nodes[0].results = vec!["design-notes".into()];
    g.nodes[1].note = "Keep the change small.".into();
    let mut j = job(Pipeline::Short).running(g.steps().unwrap());
    j.state = State::Running;
    j
}

#[test]
fn a_steps_results_are_kept_and_reach_the_step_linked_to_it() {
    let (j, effects) = go(
        &notes_flow(),
        &reply("1. Add it\nRESULT design-notes:\nUse a dict."),
    );
    assert_eq!(j.results["design-notes"], "Use a dict.");
    let brief = match effects.last() {
        Some(Effect::StartTurn {
            stage: Stage::Build,
            brief,
            ..
        }) => brief.clone(),
        other => panic!("{other:?}"),
    };
    assert!(
        brief.contains("<result name=\"design-notes\">\nUse a dict."),
        "{brief}"
    );
    assert!(brief.contains("Keep the change small."), "{brief}");
}

#[test]
fn a_step_missing_its_result_tries_once_more_then_waits() {
    let (j, effects) = go(&notes_flow(), &reply("1. Add it"));
    assert_eq!((j.stage, j.state), (Stage::Plan, State::Running));
    let Some(Effect::StartTurn { brief, .. }) = effects.last() else {
        panic!("{effects:?}")
    };
    assert!(brief.contains("RESULT design-notes:"), "{brief}");
    let (j, _) = go(&j, &reply("still none"));
    assert_eq!((j.stage, j.state), (Stage::Plan, State::NeedsYou));
}

#[test]
fn a_step_whose_linked_result_is_missing_waits_before_it_runs() {
    let mut j = notes_flow();
    j.stage = Stage::Build;
    j.state = State::NeedsYou;
    j.error = Some("x".into());
    let mut at_plan = j.clone();
    at_plan.stage = Stage::Plan;
    let (j, effects) = go(&at_plan, &Event::Approved);
    assert_eq!((j.stage, j.state), (Stage::Build, State::NeedsYou));
    assert!(j.error.unwrap().contains("design-notes"));
    assert!(effects.is_empty());
}

/// A skill, then a command that gives `lint-report`, then land.
fn custom_flow() -> Job {
    use crate::workflow::{Graph, Link, Node};
    let node = |id: &str, stage, run: &str, results: &[&str], x| Node {
        id: id.into(),
        stage,
        run: run.into(),
        dial: Dial::Dark,
        note: String::new(),
        results: results.iter().map(|r| r.to_string()).collect(),
        x,
        y: 0,
    };
    let g = Graph {
        nodes: vec![
            node("tidy", Stage::Skill, "tidy-imports", &[], 0),
            node("lint", Stage::Command, "make lint", &["lint-report"], 100),
            node("land", Stage::Land, "", &[], 200),
        ],
        links: vec![Link {
            from: "tidy".into(),
            to: "lint".into(),
        }],
    };
    assert_eq!(g.check(), Ok(()));
    let mut j = job(Pipeline::Short).running(g.steps().unwrap());
    j.state = State::Running;
    j
}

#[test]
fn a_skill_step_follows_its_skill_and_a_command_gives_its_output_as_its_result() {
    let j = custom_flow();
    assert!(brief(&j).contains("/work/repo/.claude/skills/tidy-imports/SKILL.md"));
    let (j, effects) = go(&j, &done());
    assert_eq!((j.stage, j.node.as_deref()), (Stage::Command, Some("lint")));
    assert_eq!(
        effects.last(),
        Some(&Effect::RunStep {
            stage: Stage::Command
        })
    );
    let (j, _) = go(
        &j,
        &Event::Ran {
            output: "0 problems".into(),
        },
    );
    assert_eq!(j.results["lint-report"], "0 problems");
    assert_eq!((j.stage, j.node.as_deref()), (Stage::Land, Some("land")));
}

#[test]
fn a_failing_command_waits_on_a_person() {
    let mut j = custom_flow();
    j.stage = Stage::Command;
    j.node = Some("lint".into());
    let (j, _) = go(&j, &failed("3 problems"));
    assert_eq!(
        (j.stage, j.state, j.error.as_deref()),
        (Stage::Command, State::NeedsYou, Some("3 problems"))
    );
}

#[test]
fn a_workflow_job_is_at_its_node_when_a_stage_is_there_twice() {
    use crate::workflow::{Graph, Node};
    let skill = |id: &str, x| Node {
        id: id.into(),
        stage: Stage::Skill,
        run: id.into(),
        dial: Dial::Dark,
        note: String::new(),
        results: Vec::new(),
        x,
        y: 0,
    };
    let g = Graph {
        nodes: vec![skill("tidy", 0), skill("docs", 100)],
        links: Vec::new(),
    };
    let mut j = job(Pipeline::Short).running(g.steps().unwrap());
    j.state = State::Running;
    j.node = Some("docs".into());
    let states: Vec<StepState> = crate::steps(&j).into_iter().map(|s| s.1).collect();
    assert_eq!(states, [StepState::Done, StepState::Running]);
}

const THREE_PARTS: &str =
    "Three changes.\n\nPARTS:\n1. Add the model\n2. Add the endpoint\n3. Add the page";

#[test]
fn scope_splits_a_request_into_parts_that_land_in_order() {
    let (j, effects) = go(&at(Stage::Scope, State::Running), &reply(THREE_PARTS));
    assert_eq!(j.part.as_deref(), Some("Add the model"));
    assert_eq!(j.stage, Stage::Discover);
    let Some(parts) = effects.iter().find_map(|e| match e {
        Effect::Split { parts } => Some(parts),
        _ => None,
    }) else {
        panic!("{effects:?}")
    };
    let shape: Vec<_> = parts
        .iter()
        .map(|p| {
            (
                p.id.as_str(),
                p.after.as_deref(),
                p.part.as_deref(),
                p.stage,
            )
        })
        .collect();
    assert_eq!(
        shape,
        [
            ("j1-2", Some("j1"), Some("Add the endpoint"), Stage::Prepare),
            ("j1-3", Some("j1-2"), Some("Add the page"), Stage::Prepare),
        ]
    );
    assert!(parts.iter().all(|p| p.parent.as_deref() == Some("j1")));
    assert!(crate::brief::brief(&parts[0]).contains("<part>\nAdd the endpoint\n</part>"));
}

#[test]
fn one_part_or_a_part_itself_is_not_split() {
    let one = "PARTS:\n1. Add the model";
    let (j, effects) = go(&at(Stage::Scope, State::Running), &reply(one));
    assert!(j.part.is_none());
    assert!(!effects.iter().any(|e| matches!(e, Effect::Split { .. })));
    let mut part = at(Stage::Scope, State::Running);
    part.parent = Some("j0".into());
    let (_, effects) = go(&part, &reply(THREE_PARTS));
    assert!(!effects.iter().any(|e| matches!(e, Effect::Split { .. })));
}

#[test]
fn a_part_is_admitted_only_once_the_part_before_it_lands() {
    let mut first = job(Pipeline::Full);
    first.state = State::Running;
    let mut second = job(Pipeline::Full);
    second.id = "j1-2".into();
    second.after = Some("j1".into());
    assert!(crate::admit(&[first.clone(), second.clone()], 4).is_empty());
    first.state = State::Landed;
    assert_eq!(crate::admit(&[first, second], 4), ["j1-2"]);
}

#[test]
fn implement_after_specify_is_told_the_acceptance_tests_are_fixed() {
    let mut j = at(Stage::Implement, State::Running);
    j.tasks = vec!["Add tests for modulo".into()];
    assert!(!crate::brief::brief(&j).contains("bastion-spec"));
    j.criteria = Some("C1. modulo works".into());
    assert!(crate::brief::brief(&j).contains("Do not create, change, move or delete them"));
}
