//! Acceptance tests: what each run of a
//! spec's checks means. The adapter reads the spec and runs the commands;
//! this module decides what their exit codes say.

use crate::Event;

/// One run of one check: its criterion, exit code and the tail of its output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ran {
    pub criterion: String,
    pub code: i64,
    pub output: String,
}

impl Ran {
    fn green(&self) -> bool {
        self.code == 0
    }

    /// The shell could not find or run the command at all (wipp's `NOT_RUN`).
    fn not_run(&self) -> bool {
        matches!(self.code, 126 | 127)
    }
}

fn not_run(runs: &[Ran]) -> Vec<String> {
    runs.iter()
        .filter(|r| r.not_run())
        .map(|r| {
            format!(
                "{} did not run (exit {}):\n{}",
                r.criterion, r.code, r.output
            )
        })
        .collect()
}

fn failed_with(problems: Vec<String>) -> Event {
    Event::StepFailed {
        detail: problems.join("\n\n"),
    }
}

/// The baseline, before anything is built: every check must fail. A check
/// that passes already goes to falsify, once per spec.
pub fn baseline(runs: &[Ran], absent: &[String]) -> Event {
    let mut problems = not_run(runs);
    problems.extend(
        absent
            .iter()
            .map(|f| format!("{f} is in the spec but not in the tree.")),
    );
    if !problems.is_empty() {
        return failed_with(problems);
    }
    if runs.iter().any(Ran::green) {
        return Event::ChecksGreen;
    }
    Event::StepPassed
}

/// The baseline after falsify: each check ran against the broken code, then
/// against the code put back. A check that passed against the broken code
/// cannot fail, so it checks nothing. A test the falsify turn changed proves
/// nothing either.
pub fn falsified(broken: &[Ran], touched: &[String]) -> Event {
    let mut problems = not_run(broken);
    problems.extend(broken.iter().filter(|r| r.green()).map(|r| {
        format!(
            "{} stayed green when the code was broken on purpose, so it cannot fail and checks nothing.",
            r.criterion
        )
    }));
    problems.extend(
        touched
            .iter()
            .map(|f| format!("The falsify turn changed {f}, which is a test.")),
    );
    if problems.is_empty() {
        Event::StepPassed
    } else {
        failed_with(problems)
    }
}

/// Verify, after the build: every check passes, and no approved file changed
/// since the baseline. A changed test, or one that cannot run, waits on the
/// owner; an unmet criterion goes back to implement.
pub fn verified(runs: &[Ran], changed: &[String]) -> Event {
    if !changed.is_empty() {
        let files: Vec<String> = changed.iter().map(|f| format!("- {f}")).collect();
        return Event::StepStuck {
            detail: format!(
                "The approved checks were changed after the baseline:\n{}\nA passing run of a changed check says nothing about the request.",
                files.join("\n")
            ),
        };
    }
    let cannot = not_run(runs);
    if !cannot.is_empty() {
        return Event::StepStuck {
            detail: format!(
                "The acceptance tests could not run. Make each command runnable in the workspace, then approve to go on.\n\n{}",
                cannot.join("\n\n")
            ),
        };
    }
    let unmet: Vec<String> = runs
        .iter()
        .filter(|r| !r.green())
        .map(|r| {
            format!(
                "{} is not met (exit {}):\n{}",
                r.criterion, r.code, r.output
            )
        })
        .collect();
    if unmet.is_empty() {
        Event::StepPassed
    } else {
        failed_with(unmet)
    }
}

/// The checks step: the workspace's own test command, run once in the VM.
/// A failure goes back to implement with its output; a command that cannot
/// run waits on the owner, because no fix to the code will help.
pub fn suite(ran: &Ran) -> Event {
    match ran.code {
        0 => Event::StepPassed,
        126 | 127 => Event::StepStuck {
            detail: format!(
                "The checks command could not run (exit {}). Fix it in the workspace's settings.\n{}",
                ran.code, ran.output
            ),
        },
        code => Event::StepFailed {
            detail: format!("The checks failed (exit {code}):\n{}", ran.output),
        },
    }
}

/// A workflow's command step (D35): exit 0 passes with what it printed,
/// which is its result; anything else fails with it.
pub fn command(ran: &Ran) -> Event {
    match ran.code {
        0 => Event::Ran {
            output: ran.output.clone(),
        },
        code => Event::StepFailed {
            detail: format!("The command failed (exit {code}):\n{}", ran.output),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ran(criterion: &str, code: i64) -> Ran {
        Ran {
            criterion: criterion.into(),
            code,
            output: "out".into(),
        }
    }

    #[test]
    fn the_suite_passes_fails_or_cannot_run() {
        assert_eq!(suite(&ran("checks", 0)), Event::StepPassed);
        assert!(
            matches!(suite(&ran("checks", 1)), Event::StepFailed { detail } if detail.contains("exit 1"))
        );
        assert!(matches!(
            suite(&ran("checks", 127)),
            Event::StepStuck { .. }
        ));
    }

    #[test]
    fn a_baseline_passes_only_when_every_check_fails() {
        assert_eq!(
            baseline(&[ran("C1", 1), ran("C2", 2)], &[]),
            Event::StepPassed
        );
        assert_eq!(
            baseline(&[ran("C1", 1), ran("C2", 0)], &[]),
            Event::ChecksGreen
        );
        assert!(matches!(
            baseline(&[ran("C1", 127)], &[]),
            Event::StepFailed { detail } if detail.contains("C1 did not run (exit 127)")
        ));
        assert!(matches!(
            baseline(&[ran("C1", 1)], &["t.py".into()]),
            Event::StepFailed { detail } if detail.contains("t.py is in the spec")
        ));
    }

    #[test]
    fn falsify_proves_a_check_only_when_it_goes_red_and_its_test_is_untouched() {
        assert_eq!(falsified(&[ran("C1", 1)], &[]), Event::StepPassed);
        assert!(matches!(
            falsified(&[ran("C1", 0)], &[]),
            Event::StepFailed { detail } if detail.contains("C1 stayed green")
        ));
        assert!(matches!(
            falsified(&[ran("C1", 1)], &["t.py".into()]),
            Event::StepFailed { detail } if detail.contains("changed t.py")
        ));
    }

    #[test]
    fn verify_waits_on_a_changed_test_and_sends_an_unmet_criterion_back() {
        assert_eq!(verified(&[ran("C1", 0)], &[]), Event::StepPassed);
        assert!(matches!(
            verified(&[ran("C1", 0)], &["t.py".into()]),
            Event::StepStuck { detail } if detail.contains("- t.py")
        ));
        assert!(matches!(
            verified(&[ran("C1", 127)], &[]),
            Event::StepStuck { .. }
        ));
        assert!(matches!(
            verified(&[ran("C1", 1)], &[]),
            Event::StepFailed { detail } if detail.contains("C1 is not met (exit 1)")
        ));
    }

    #[test]
    fn a_command_passes_with_its_output_or_fails_with_it() {
        let ran = |code| Ran {
            criterion: "make lint".into(),
            code,
            output: "0 problems".into(),
        };
        assert_eq!(
            command(&ran(0)),
            Event::Ran {
                output: "0 problems".into()
            }
        );
        assert!(
            matches!(command(&ran(2)), Event::StepFailed { detail } if detail.contains("exit 2"))
        );
    }
}
