//! A/B evals (PLAN.md A6, D22): the same requests on the same snapshot under
//! two harness configurations. Each configuration is the owner's policy with
//! some rungs moved. A run is tallied from its audit record and its turns, and
//! the two arms are compared per request and in total.

use serde::{Deserialize, Serialize};

use crate::Stage;
use crate::audit::Record;
use crate::govern::{Access, Call, Capability, Constraint, Decision, Enforcement, Verdict};

/// One rung an arm sets, on a constraint or on a grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Override {
    Constraint {
        id: String,
        rung: Enforcement,
    },
    Capability {
        stage: Stage,
        function: String,
        rung: Access,
    },
}

/// The policy an arm runs under: the owner's, with the arm's rungs set.
pub fn apply(constraints: &mut [Constraint], grants: &mut [Capability], overrides: &[Override]) {
    for o in overrides {
        match o {
            Override::Constraint { id, rung } => {
                constraints
                    .iter_mut()
                    .filter(|c| &c.id == id)
                    .for_each(|c| c.rung = *rung);
            }
            Override::Capability {
                stage,
                function,
                rung,
            } => grants
                .iter_mut()
                .filter(|g| g.stage == *stage && &g.function == function)
                .for_each(|g| g.rung = *rung),
        }
    }
}

/// The audit line for one gate decision. `tally` reads it back.
pub fn describe(call: &Call, verdict: &Verdict) -> String {
    let decision = match &verdict.decision {
        Decision::Continue => "continue".to_string(),
        Decision::Deny { reason } => format!("deny ({reason})"),
        Decision::Hold { reason } => format!("hold ({reason})"),
    };
    let mut detail = format!("{decision} {} {}", call.function, call.text);
    if !verdict.flagged.is_empty() {
        detail.push_str(&format!(" flagged: {}", verdict.flagged.join(", ")));
    }
    detail
}

/// What one turn cost, as the harness metered it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub turns: u32,
    pub calls: u32,
    pub call_errors: u32,
    /// Millionths of a US dollar, so runs add up exactly.
    pub cost_micros: u64,
}

impl std::ops::Add for Usage {
    type Output = Usage;

    fn add(self, o: Usage) -> Usage {
        Usage {
            turns: self.turns + o.turns,
            calls: self.calls + o.calls,
            call_errors: self.call_errors + o.call_errors,
            cost_micros: self.cost_micros + o.cost_micros,
        }
    }
}

/// One request under one arm, tallied.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    /// The build's diff reached review.
    pub reviewed: bool,
    /// Calls the gate let through, refused and held.
    pub passed: u32,
    pub refused: u32,
    pub held: u32,
    /// Calls a checked constraint recorded.
    pub flagged: u32,
    /// Calls the repository's hooks refused.
    pub hook_refused: u32,
    /// Constraints the review diff broke.
    pub findings: u32,
    pub usage: Usage,
    pub seconds: u64,
    /// The judge's grade from 0 to 10, once graded.
    pub grade: Option<u8>,
}

/// Count a run's audit records by what they decided.
pub fn tally(records: &[Record]) -> Run {
    let mut run = Run::default();
    for r in records {
        match r.kind.as_str() {
            "gate" if r.detail.starts_with("continue") => run.passed += 1,
            "gate" if r.detail.starts_with("deny") => run.refused += 1,
            "gate" if r.detail.starts_with("hold") => run.held += 1,
            "hook-refuse" => run.hook_refused += 1,
            "review" => run.findings += 1,
            _ => {}
        }
        if r.kind == "gate" && r.detail.contains(" flagged: ") {
            run.flagged += 1;
        }
    }
    run
}

/// The sum of an arm's runs. The grade is the mean of the graded runs.
pub fn total(runs: &[Run]) -> Run {
    let graded: Vec<u32> = runs.iter().filter_map(|r| r.grade).map(u32::from).collect();
    let mean =
        (!graded.is_empty()).then(|| (graded.iter().sum::<u32>() / graded.len() as u32) as u8);
    runs.iter().fold(
        Run {
            grade: mean,
            ..Run::default()
        },
        |t, r| Run {
            reviewed: false,
            passed: t.passed + r.passed,
            refused: t.refused + r.refused,
            held: t.held + r.held,
            flagged: t.flagged + r.flagged,
            hook_refused: t.hook_refused + r.hook_refused,
            findings: t.findings + r.findings,
            usage: t.usage + r.usage,
            seconds: t.seconds + r.seconds,
            grade: t.grade,
        },
    )
}

/// Each measure that differs between A and B, as "name: a -> b".
pub fn differences(a: &Run, b: &Run) -> Vec<String> {
    let money = |m: u64| format!("${:.4}", m as f64 / 1e6);
    let grade = |g: Option<u8>| g.map_or("-".to_string(), |g| g.to_string());
    let measures = [
        ("refused", a.refused.to_string(), b.refused.to_string()),
        ("held", a.held.to_string(), b.held.to_string()),
        ("flagged", a.flagged.to_string(), b.flagged.to_string()),
        (
            "hook refused",
            a.hook_refused.to_string(),
            b.hook_refused.to_string(),
        ),
        (
            "review findings",
            a.findings.to_string(),
            b.findings.to_string(),
        ),
        (
            "turns",
            a.usage.turns.to_string(),
            b.usage.turns.to_string(),
        ),
        ("calls", a.passed.to_string(), b.passed.to_string()),
        (
            "cost",
            money(a.usage.cost_micros),
            money(b.usage.cost_micros),
        ),
        ("grade", grade(a.grade), grade(b.grade)),
    ];
    measures
        .into_iter()
        .filter(|(_, x, y)| x != y)
        .map(|(name, x, y)| format!("{name}: {x} -> {y}"))
        .collect()
}

/// The grade a judge's reply gives, from its `GRADE: n` line.
pub fn grade(reply: &str) -> Option<u8> {
    reply
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix("GRADE:"))
        .and_then(|n| {
            n.trim()
                .split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse::<u8>()
                .ok()
        })
        .filter(|g| *g <= 10)
}

/// What the judge is asked about one run.
pub fn grading_brief(request: &str, diff: &str) -> String {
    format!(
        "You grade one change an agent made for a request. You cannot run anything.\n\n\
         The request:\n<request>\n{request}\n</request>\n\n\
         The change as a diff:\n<diff>\n{diff}\n</diff>\n\n\
         Grade from 0 to 10 how fully and correctly the change does the request, with nothing it \
         did not ask for. Give one or two sentences of reasons, then a last line exactly like \
         GRADE: 7"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::govern::{Source, default_capabilities, default_constraints};

    fn record(kind: &str, detail: &str) -> Record {
        Record {
            kind: kind.into(),
            subject: "j1".into(),
            detail: detail.into(),
        }
    }

    #[test]
    fn an_arm_moves_only_its_rungs() {
        let (mut rules, mut grants) = (default_constraints(), default_capabilities());
        apply(
            &mut rules,
            &mut grants,
            &[
                Override::Constraint {
                    id: "keep-tests".into(),
                    rung: Enforcement::Enforced,
                },
                Override::Capability {
                    stage: Stage::Plan,
                    function: "sandbox::fs::write".into(),
                    rung: Access::Allowed,
                },
            ],
        );
        let keep = rules.iter().find(|c| c.id == "keep-tests").unwrap();
        assert_eq!(
            (keep.rung, keep.source),
            (Enforcement::Enforced, Source::Owner)
        );
        assert_eq!(
            rules
                .iter()
                .find(|c| c.id == "no-login-shell")
                .unwrap()
                .rung,
            Enforcement::Enforced
        );
        let write = grants
            .iter()
            .find(|g| g.function == "sandbox::fs::write")
            .unwrap();
        assert_eq!(write.rung, Access::Allowed);
    }

    #[test]
    fn a_run_is_tallied_from_the_lines_describe_writes() {
        let call = Call {
            function: "sandbox::exec".into(),
            text: "git rm a".into(),
        };
        let deny = Verdict {
            decision: Decision::Deny {
                reason: "no".into(),
            },
            flagged: vec![],
        };
        let pass = Verdict {
            decision: Decision::Continue,
            flagged: vec!["keep-tests".into()],
        };
        let records = [
            record("gate", &describe(&call, &deny)),
            record("gate", &describe(&call, &pass)),
            record("hook-refuse", "x"),
            record("review", "tests-kept (Checked): Deleted tests: test_add"),
            record("promote", "Told -> Checked"),
        ];
        let run = tally(&records);
        assert_eq!(
            (
                run.refused,
                run.passed,
                run.flagged,
                run.hook_refused,
                run.findings
            ),
            (1, 1, 1, 1, 1)
        );
    }

    #[test]
    fn totals_add_and_grades_average() {
        let one = Run {
            refused: 2,
            usage: Usage {
                turns: 2,
                cost_micros: 1500,
                ..Usage::default()
            },
            grade: Some(8),
            ..Run::default()
        };
        let two = Run {
            refused: 1,
            usage: Usage {
                turns: 3,
                cost_micros: 500,
                ..Usage::default()
            },
            grade: Some(6),
            ..Run::default()
        };
        let sum = total(&[one, two, Run::default()]);
        assert_eq!(
            (
                sum.refused,
                sum.usage.turns,
                sum.usage.cost_micros,
                sum.grade
            ),
            (3, 5, 2000, Some(7))
        );
        assert_eq!(total(&[]).grade, None);
    }

    #[test]
    fn differences_name_each_measure_that_moved() {
        let a = Run {
            refused: 0,
            grade: Some(8),
            ..Run::default()
        };
        let b = Run {
            refused: 3,
            grade: Some(8),
            ..Run::default()
        };
        assert_eq!(differences(&a, &b), ["refused: 0 -> 3"]);
        assert!(differences(&a, &a).is_empty());
    }

    #[test]
    fn the_grade_comes_from_the_last_grade_line() {
        assert_eq!(grade("Good change.\nGRADE: 8"), Some(8));
        assert_eq!(grade("GRADE: 3\nthen\nGRADE: 9/10"), Some(9));
        assert_eq!(grade("GRADE: 11"), None);
        assert_eq!(grade("no grade"), None);
    }
}
