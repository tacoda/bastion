//! The acceptance-test steps the factory runs itself (A8 step 3): baseline,
//! verify and acceptance. Each runs the spec's checks in the job's VM, and
//! `bastion_core::acceptance` decides what the exit codes say.

use bastion_core::acceptance::{self, Ran};
use bastion_core::{Event, Stage};
use serde_json::{Value, json};

use super::{BEFORE_TURN, DEFAULT_NAMESPACE, Factory, REPO};
use crate::engine::Engine;
use crate::spec::{self, Spec};
use crate::store::Record;

/// The VM tag on the tree the baseline approved. Verify compares the spec's
/// files against it.
// ponytail: the tag lives in the VM, so a VM rebuilt by revive (A8 step 5) has none.
const SPEC_TAG: &str = "bastion-spec";
/// How long one check may run.
const CHECK_SECS: u64 = 15 * 60;
/// How much of a check's output goes into a brief or a note.
const TAIL: usize = 3000;

impl Factory {
    /// Run baseline, verify or acceptance, and say how it went.
    pub(super) async fn acceptance_step(&self, record: &Record, stage: Stage) -> Event {
        let ran = async {
            let sandbox = record.runtime.sandbox.as_deref().ok_or("no VM")?;
            let vm = Vm {
                engine: &*self.engine,
                sandbox,
                spec: self.latest_spec(&record.job.id).await?,
            };
            match stage {
                Stage::Baseline if record.job.falsify_tried => vm.judge_falsify().await,
                Stage::Baseline => vm.baseline().await,
                _ => vm.verify().await,
            }
        };
        ran.await
            .unwrap_or_else(|detail: String| Event::StepStuck { detail })
    }

    /// The spec the latest specify turn wrote.
    async fn latest_spec(&self, job: &str) -> Result<Spec, String> {
        let artifacts = self.store.artifacts(job).await?;
        let reply = artifacts
            .iter()
            .rev()
            .find(|a| a.stage == Stage::Specify)
            .ok_or("no spec: specify has not run")?;
        spec::read(&reply.body)
    }
}

/// One job's VM, with the spec whose checks run in it.
struct Vm<'a> {
    engine: &'a dyn Engine,
    sandbox: &'a str,
    spec: Spec,
}

impl Vm<'_> {
    /// Before the build, every check must fail.
    async fn baseline(&self) -> Result<Event, String> {
        self.commit_spec().await?;
        let absent = self.absent().await?;
        let runs = self.run_checks().await?;
        self.put_back_test_writes().await?;
        self.approve(acceptance::baseline(&runs, &absent)).await
    }

    /// After falsify: the checks against the broken code, then the code put
    /// back to the tag made before the turn, tests included.
    async fn judge_falsify(&self) -> Result<Event, String> {
        let broken = self.run_checks().await?;
        let touched = self.changed_since(BEFORE_TURN).await?;
        self.sh(&format!(
            "git reset -q --hard {BEFORE_TURN} && git clean -qfd"
        ))
        .await?;
        self.approve(acceptance::falsified(&broken, &touched)).await
    }

    /// After the build, every check passes on the tests the baseline approved.
    async fn verify(&self) -> Result<Event, String> {
        let changed = self.changed_since(SPEC_TAG).await?;
        let runs = self.run_checks().await?;
        Ok(acceptance::verified(&runs, &changed))
    }

    /// A baseline that passes marks the tree its checks were approved on.
    async fn approve(&self, event: Event) -> Result<Event, String> {
        if event == Event::StepPassed {
            self.sh(&format!("git tag -f {SPEC_TAG}")).await?;
        }
        Ok(event)
    }

    /// Commit the spec's files, if the specify turn left them uncommitted, so
    /// they land with the change and a reset to a tag keeps them.
    async fn commit_spec(&self) -> Result<(), String> {
        let list = self.files();
        self.sh(&format!(
            "git add -- {list} 2>/dev/null; git diff --cached --quiet || git commit -q -m 'Add the acceptance tests'"
        ))
        .await
        .map(|_| ())
    }

    /// The spec's files missing from the tree.
    async fn absent(&self) -> Result<Vec<String>, String> {
        let list = self.files();
        let out = self
            .sh(&format!(
                "for f in {list}; do [ -e \"$f\" ] || echo \"$f\"; done"
            ))
            .await?;
        Ok(lines(&out))
    }

    /// Of the spec's files, those that differ from `tag` in the tree.
    async fn changed_since(&self, tag: &str) -> Result<Vec<String>, String> {
        let list = self.files();
        let out = self
            .sh(&format!("git diff --name-only {tag} -- {list}"))
            .await?;
        Ok(lines(&out))
    }

    /// Files the checks wrote that git tracks and the spec does not name go back.
    // ponytail: untracked files a check leaves stay; wipp puts back tracked files only too.
    async fn put_back_test_writes(&self) -> Result<(), String> {
        let approved = self.spec.files();
        let written = lines(&self.sh("git diff --name-only").await?);
        let stray: Vec<String> = written
            .into_iter()
            .filter(|f| !approved.contains(f))
            .collect();
        if !stray.is_empty() {
            self.sh(&format!("git checkout -- {}", quoted(&stray)))
                .await?;
        }
        Ok(())
    }

    async fn run_checks(&self) -> Result<Vec<Ran>, String> {
        let mut runs = Vec::new();
        for check in &self.spec.checks {
            let out = self.exec(&check.run, CHECK_SECS).await?;
            runs.push(Ran {
                criterion: check.criterion.clone(),
                code: out["exit_code"].as_i64().unwrap_or(-1),
                output: tail(&format!(
                    "{}{}",
                    out["stdout"].as_str().unwrap_or_default(),
                    out["stderr"].as_str().unwrap_or_default()
                )),
            });
        }
        Ok(runs)
    }

    /// The spec's files as shell words.
    fn files(&self) -> String {
        quoted(&self.spec.files())
    }

    /// Run a shell line in the repository, and fail unless it exits 0.
    async fn sh(&self, line: &str) -> Result<String, String> {
        let out = self.exec(line, 120).await?;
        match out["exit_code"].as_i64() {
            Some(0) | None => Ok(out["stdout"].as_str().unwrap_or_default().to_string()),
            Some(code) => Err(format!("{line} exited {code}: {}", out["stderr"])),
        }
    }

    /// Run a shell line in the repository, whatever it exits with.
    async fn exec(&self, line: &str, secs: u64) -> Result<Value, String> {
        let argv = ["sh", "-c", &format!("cd {REPO} && {line}")];
        self.engine
            .call_for(
                DEFAULT_NAMESPACE,
                "sandbox::exec",
                json!({ "sandbox_id": self.sandbox, "argv": argv, "timeout_ms": secs * 1000 }),
                secs * 1000 + 60_000,
            )
            .await
    }
}

/// Paths as single-quoted shell words. A spec's paths are safe already
/// (`spec::read`), so none holds a quote.
fn quoted(paths: &[String]) -> String {
    let words: Vec<String> = paths.iter().map(|p| format!("'{p}'")).collect();
    words.join(" ")
}

fn lines(out: &str) -> Vec<String> {
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

fn tail(text: &str) -> String {
    let start = text.len().saturating_sub(TAIL);
    let start = (start..=text.len())
        .find(|i| text.is_char_boundary(*i))
        .unwrap_or(text.len());
    text[start..].to_string()
}
