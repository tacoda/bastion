//! The gate in the factory (PLAN.md A5, D20, D21). Every `sandbox::*` call a
//! turn makes reaches `judge` through a `pre-trigger` hook, and
//! `bastion_core::govern::gate` decides it. The hook reads the policy from
//! memory and hands every record to the audit writer, so a slow store never
//! stalls a turn. Without an audit writer it refuses every call.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use bastion_core::Stage;
use bastion_core::audit;
use bastion_core::govern::{self, Call, Capability, Constraint, Decision, Verdict};
use serde_json::{Value, json};

use crate::audit_log::AuditLog;
use crate::charter::{self, Charter};
use crate::engine::{DEFAULT_NAMESPACE, Engine};
use crate::factory::{OnChange, now};
use crate::store::{HeldCall, Store};

const NS: &str = "bastion";
const RUN_MS: u64 = 120_000;

/// The rules the gate judges by, as the store last gave them.
struct Policy {
    constraints: Vec<Constraint>,
    grants: Vec<Capability>,
}

pub struct Gate {
    store: Arc<dyn Store>,
    engine: Arc<dyn Engine>,
    on_change: OnChange,
    /// None until the first load. Until then every call is refused.
    policy: RwLock<Option<Policy>>,
    /// The writer, or why there is none (such as no Keychain key).
    audit: Result<Arc<AuditLog>, String>,
    /// Each job's charter, read from its worktree when a turn starts.
    charters: Mutex<HashMap<String, Arc<Charter>>>,
    /// Each job's repository hook runs, latest last.
    hook_runs: Mutex<HashMap<String, Vec<String>>>,
    /// What each job's latest review diff broke, by constraint.
    findings: Mutex<HashMap<String, Vec<String>>>,
}

/// What the rules read from a call: its command line in any of the shapes
/// `sandbox::exec` takes (`argv`, or `cmd` with `args`), else its path.
fn call_of(call: &Value) -> Call {
    let arguments = &call["arguments"];
    let words = |v: &Value| -> Vec<String> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|w| w.as_str().map(String::from))
            .collect()
    };
    let mut line = words(&arguments["argv"]);
    if let Some(cmd) = arguments["cmd"].as_str() {
        line.push(cmd.into());
        line.extend(words(&arguments["args"]));
    }
    let text = if line.is_empty() {
        arguments["path"].as_str().unwrap_or_default().to_string()
    } else {
        line.join(" ")
    };
    Call {
        function: call["function_id"].as_str().unwrap_or_default().into(),
        text,
    }
}

fn describe(call: &Call, verdict: &Verdict) -> String {
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

/// A call's arguments as a Claude Code hook reads `tool_input`.
fn tool_input(held: &HeldCall) -> Value {
    match held.call.function.as_str() {
        "sandbox::exec" => json!({ "command": held.call.text }),
        _ => json!({ "file_path": held.call.text, "content": held.arguments["content"] }),
    }
}

fn text_result(text: &str, details: Value, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "details": details, "is_error": is_error })
}

impl Gate {
    pub fn new(
        store: Arc<dyn Store>,
        engine: Arc<dyn Engine>,
        on_change: OnChange,
        audit: Result<Arc<AuditLog>, String>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            engine,
            on_change,
            policy: RwLock::new(None),
            audit,
            charters: Mutex::new(HashMap::new()),
            hook_runs: Mutex::new(HashMap::new()),
            findings: Mutex::new(HashMap::new()),
        })
    }

    /// Use this charter for the job's calls from now on, and record it.
    pub fn set_charter(&self, job: &str, charter: Charter) {
        let files = if charter.files.is_empty() {
            "none: owner defaults".into()
        } else {
            charter.files.join(", ")
        };
        let detail = format!(
            "{files}; {} rules, {} hooks before calls, {} stop hooks",
            charter.constraints.len(),
            charter.before.len(),
            charter.stop.len()
        );
        self.note("charter", job, detail);
        self.charters
            .lock()
            .unwrap()
            .insert(job.into(), Arc::new(charter));
    }

    pub fn charter(&self, job: &str) -> Option<Arc<Charter>> {
        self.charters.lock().unwrap().get(job).cloned()
    }

    pub fn hook_runs(&self, job: &str) -> Vec<String> {
        self.hook_runs
            .lock()
            .unwrap()
            .get(job)
            .cloned()
            .unwrap_or_default()
    }

    fn ran(&self, job: &str, kind: &str, ran: &charter::Ran) {
        self.note(kind, job, ran.line());
        self.hook_runs
            .lock()
            .unwrap()
            .entry(job.into())
            .or_default()
            .push(ran.line());
    }

    /// Read the constraints and grants again, after a change to either.
    pub async fn reload(&self) -> Result<(), String> {
        let policy = Policy {
            constraints: self.store.constraints().await?,
            grants: self.store.capabilities().await?,
        };
        *self.policy.write().unwrap() = Some(policy);
        Ok(())
    }

    pub fn audit(&self) -> Result<&Arc<AuditLog>, String> {
        self.audit.as_ref().map_err(Clone::clone)
    }

    fn note(&self, kind: &str, subject: &str, detail: String) {
        if let Ok(log) = &self.audit {
            log.note(audit::Record {
                kind: kind.into(),
                subject: subject.into(),
                detail,
            });
        }
    }

    /// Keep and record what a job's review diff broke.
    pub fn reviewed(&self, job: &str, findings: &[bastion_core::review::Finding]) {
        let lines: Vec<String> = findings
            .iter()
            .map(|f| format!("{} ({:?}): {}", f.constraint, f.rung, f.detail))
            .collect();
        for line in &lines {
            self.note("review", job, line.clone());
        }
        self.findings.lock().unwrap().insert(job.into(), lines);
    }

    pub fn findings(&self, job: &str) -> Vec<String> {
        self.findings
            .lock()
            .unwrap()
            .get(job)
            .cloned()
            .unwrap_or_default()
    }

    /// The owner's constraints and the job's charter's, together.
    pub fn constraints(&self, job: &str) -> Vec<Constraint> {
        let mut all: Vec<Constraint> = self
            .policy
            .read()
            .unwrap()
            .iter()
            .flat_map(|p| p.constraints.clone())
            .collect();
        all.extend(
            self.charter(job)
                .into_iter()
                .flat_map(|c| c.constraints.clone()),
        );
        all
    }

    /// Every rule, for the brief. The agent is told each rule at every rung.
    pub fn rules(&self, job: &str) -> String {
        let rules: Vec<String> = self
            .constraints(job)
            .iter()
            .map(|c| format!("- {}", c.rule))
            .collect();
        if rules.is_empty() {
            return String::new();
        }
        format!("Rules for this job:\n{}\n\n", rules.join("\n"))
    }

    /// The hook: decide one call, pin it to the job's VM, and record it.
    pub fn judge(self: &Arc<Self>, envelope: &Value) -> Value {
        let call = call_of(&envelope["call"]);
        let job = envelope["metadata"]["job"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let stage: Option<Stage> =
            serde_json::from_value(envelope["metadata"]["stage"].clone()).ok();
        let verdict = match (&*self.policy.read().unwrap(), stage, &self.audit) {
            (_, _, Err(why)) => Verdict {
                decision: Decision::Deny {
                    reason: format!("The gate cannot record this call, so it refuses it: {why}"),
                },
                flagged: vec![],
            },
            (Some(p), Some(stage), Ok(_)) => {
                govern::gate(&call, stage, &self.constraints(&job), &p.grants)
            }
            _ => Verdict {
                decision: Decision::Deny {
                    reason: "The gate cannot judge this call: it has no policy or no stage.".into(),
                },
                flagged: vec![],
            },
        };
        self.note("gate", &job, describe(&call, &verdict));
        let hooks: Vec<String> = self
            .charter(&job)
            .iter()
            .flat_map(|c| c.before.iter())
            .filter(|h| h.functions.contains(&call.function.as_str()))
            .map(|h| h.command.clone())
            .collect();
        let held = || HeldCall {
            id: envelope["call"]["id"].as_str().unwrap_or_default().into(),
            job: job.clone(),
            session: envelope["session_id"].as_str().unwrap_or_default().into(),
            turn: envelope["turn_id"].as_str().unwrap_or_default().into(),
            call: call.clone(),
            arguments: envelope["call"]["arguments"].clone(),
            at: now(),
        };
        match verdict.decision {
            // The repository's hooks run first, in the VM, so the call waits on them.
            Decision::Continue if !hooks.is_empty() => {
                let (gate, held) = (self.clone(), held());
                tauri::async_runtime::spawn(async move { gate.before(held, hooks).await });
                json!({ "decision": "hold", "reason": "The repository's hooks run first." })
            }
            Decision::Continue => {
                let mut arguments = envelope["call"]["arguments"].clone();
                arguments["sandbox_id"] = envelope["metadata"]["sandbox_id"].clone();
                json!({ "decision": "continue", "mutations": { "arguments": arguments } })
            }
            Decision::Deny { reason } => json!({ "decision": "deny", "reason": reason }),
            Decision::Hold { reason } => {
                self.keep(held());
                json!({ "decision": "hold", "reason": reason })
            }
        }
    }

    /// Save a held call for the job page, off the hook's path.
    fn keep(&self, held: HeldCall) {
        let (store, on_change) = (self.store.clone(), self.on_change.clone());
        tauri::async_runtime::spawn(async move {
            match store.hold(&held).await {
                Ok(()) => on_change(&held.job),
                Err(e) => eprintln!("job {}: the held call was not saved: {e}", held.job),
            }
        });
    }

    async fn find(&self, job: &str, id: &str) -> Result<HeldCall, String> {
        let held = self.store.held(job).await?;
        held.into_iter()
            .find(|h| h.id == id)
            .ok_or_else(|| format!("no held call {id} on job {job}"))
    }

    /// Send a held call its result, so the parked turn goes on, and record
    /// what the owner did as `kind`.
    async fn resolve(&self, held: &HeldCall, result: Value, kind: &str) -> Result<(), String> {
        let mut detail = format!("{} {}", held.call.function, held.call.text);
        if result["is_error"] == true {
            detail.push_str(&format!(
                ": {}",
                result["content"][0]["text"].as_str().unwrap_or_default()
            ));
        }
        let mut payload = json!({
            "session_id": held.session, "turn_id": held.turn, "function_call_id": held.id, "action": "deliver",
        });
        payload
            .as_object_mut()
            .unwrap()
            .extend(result.as_object().cloned().unwrap_or_default());
        self.engine
            .call(NS, "harness::function::resolve", payload)
            .await?;
        self.store.release(&held.id).await?;
        self.note(kind, &held.job, detail);
        (self.on_change)(&held.job);
        Ok(())
    }

    /// Run the repository's hooks on a call. Exit 2 from any of them refuses
    /// it with the hook's output; otherwise the call runs.
    async fn before(&self, held: HeldCall, hooks: Vec<String>) {
        let outcome = match self.sandbox(&held.job).await {
            Ok(sandbox) => {
                let input = json!({
                    "hook_event_name": "PreToolUse", "cwd": "/work/repo",
                    "tool_name": bastion_core::charter::tool_name(&held.call.function),
                    "tool_input": tool_input(&held),
                });
                let mut refused = None;
                for command in hooks {
                    let ran = charter::run(&*self.engine, &sandbox, &command, &input).await;
                    self.ran(&held.job, "hook", &ran);
                    if ran.refuses() {
                        refused = Some(ran);
                        break;
                    }
                }
                match refused {
                    Some(ran) => {
                        let text = format!(
                            "The repository's hook refused this call: {}",
                            ran.output.trim()
                        );
                        self.resolve(&held, text_result(&text, Value::Null, true), "hook-refuse")
                            .await
                    }
                    None => self.run_held(&held, "hook-pass").await,
                }
            }
            Err(e) => Err(e),
        };
        if let Err(e) = outcome {
            eprintln!("job {}: a hooked call was not settled: {e}", held.job);
        }
    }

    /// Run each `Stop` hook of the job's charter in its VM, and keep the results.
    pub async fn stop_hooks(&self, job: &str, sandbox: &str) {
        let Some(charter) = self.charter(job) else {
            return;
        };
        let input = json!({ "hook_event_name": "Stop", "cwd": "/work/repo" });
        for command in &charter.stop {
            let ran = charter::run(&*self.engine, sandbox, command, &input).await;
            self.ran(job, "stop-hook", &ran);
        }
    }

    async fn sandbox(&self, job: &str) -> Result<String, String> {
        Ok(self
            .store
            .load(job)
            .await?
            .runtime
            .sandbox
            .ok_or("the job has no VM")?)
    }

    /// Run exactly this call once, in the job's VM, and deliver its result.
    /// The hold dropped the VM pin, so the app pins it again here (N8).
    pub async fn allow_once(&self, job: &str, id: &str) -> Result<(), String> {
        let held = self.find(job, id).await?;
        self.run_held(&held, "allow-once").await
    }

    async fn run_held(&self, held: &HeldCall, kind: &str) -> Result<(), String> {
        let sandbox = self
            .store
            .load(&held.job)
            .await?
            .runtime
            .sandbox
            .ok_or("the job has no VM")?;
        let mut arguments = held.arguments.clone();
        arguments["sandbox_id"] = json!(sandbox);
        let ran = self
            .engine
            .call_for(DEFAULT_NAMESPACE, &held.call.function, arguments, RUN_MS)
            .await;
        let result = match ran {
            Ok(out) => text_result(&out.to_string(), out, false),
            Err(e) => text_result(&e, Value::Null, true),
        };
        self.resolve(held, result, kind).await
    }

    /// Refuse a held call, and tell the agent why and what to do instead.
    pub async fn refuse(&self, job: &str, id: &str, feedback: &str) -> Result<(), String> {
        let held = self.find(job, id).await?;
        let text = format!("The owner refused this call: {feedback}");
        self.resolve(&held, text_result(&text, Value::Null, true), "refuse")
            .await
    }

    /// Refuse every call still held for a job that stopped or finished.
    pub async fn clear(&self, job: &str) {
        for held in self.store.held(job).await.unwrap_or_default() {
            let result = text_result("The job stopped.", Value::Null, true);
            if let Err(e) = self.resolve(&held, result, "clear").await {
                eprintln!("job {job}: a held call was not cleared: {e}");
            }
        }
    }

    /// Move a constraint one rung up or down its ladder, and record it.
    pub async fn move_constraint(&self, id: &str, up: bool) -> Result<(), String> {
        let constraints = self.store.constraints().await?;
        let mut c = constraints
            .into_iter()
            .find(|c| c.id == id)
            .ok_or(format!("no constraint {id}"))?;
        let was = c.rung;
        c.rung = if up { was.promote() } else { was.demote() };
        self.store.set_constraint(&c).await?;
        self.reload().await?;
        self.note(
            if up { "promote" } else { "demote" },
            id,
            format!("{was:?} -> {:?}", c.rung),
        );
        Ok(())
    }

    /// Move a grant one rung up or down its ladder, and record it.
    pub async fn move_capability(
        &self,
        stage: Stage,
        function: &str,
        up: bool,
    ) -> Result<(), String> {
        let grants = self.store.capabilities().await?;
        let mut g = grants
            .into_iter()
            .find(|g| g.stage == stage && g.function == function)
            .ok_or(format!("no grant for {function} at {stage:?}"))?;
        let was = g.rung;
        g.rung = if up { was.promote() } else { was.demote() };
        self.store.set_capability(&g).await?;
        self.reload().await?;
        let subject = format!("{stage:?} {function}");
        self.note(
            if up { "promote" } else { "demote" },
            &subject,
            format!("{was:?} -> {:?}", g.rung),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bastion_core::Job;
    use bastion_protocol::Link;

    use super::*;
    use crate::engine::fake::FakeEngine;
    use crate::store::fake::FakeStore;
    use crate::store::{Record, Runtime};

    const KEY: &[u8] = b"test key";

    struct Rig {
        gate: Arc<Gate>,
        engine: Arc<FakeEngine>,
        store: Arc<FakeStore>,
    }

    fn rig() -> Rig {
        rig_with(
            FakeEngine::new(Link::Up)
                .answer("harness::function::resolve", Ok(json!({})))
                .answer("sandbox::fs::write", Ok(json!({ "written": 3 }))),
        )
    }

    fn rig_with(engine: FakeEngine) -> Rig {
        let engine = Arc::new(engine);
        let store = Arc::new(FakeStore::default());
        *store.constraints.lock().unwrap() = govern::default_constraints();
        *store.capabilities.lock().unwrap() = govern::default_capabilities();
        let job = Record {
            job: Job::new("j1".into(), "w".into(), "r".into(), 1),
            runtime: Runtime {
                sandbox: Some("vm1".into()),
                ..Runtime::default()
            },
            rev: 1,
            created_at: 1,
        };
        store.jobs.lock().unwrap().insert("j1".into(), job);
        let log = AuditLog::start(store.clone(), KEY.to_vec(), crate::audit_log::WRITE_LIMIT);
        let gate = Gate::new(
            store.clone(),
            engine.clone(),
            Arc::new(|_: &str| {}),
            Ok(log),
        );
        tauri::async_runtime::block_on(gate.reload()).unwrap();
        Rig {
            gate,
            engine,
            store,
        }
    }

    fn envelope(stage: &str, function: &str, arguments: Value) -> Value {
        json!({
            "session_id": "s1", "turn_id": "t1",
            "call": { "id": "fc1", "function_id": function, "arguments": arguments },
            "metadata": { "job": "j1", "stage": stage, "sandbox_id": "vm1" },
        })
    }

    fn write() -> Value {
        envelope(
            "plan",
            "sandbox::fs::write",
            json!({ "sandbox_id": "job", "path": "/work/repo/a.py" }),
        )
    }

    /// Wait for the writer task and the spawned saves to catch up.
    fn settle() {
        std::thread::sleep(Duration::from_millis(50));
    }

    #[test]
    fn a_passing_call_is_pinned_to_the_vm_and_recorded() {
        let r = rig();
        let answer = r.gate.judge(&envelope(
            "build",
            "sandbox::exec",
            json!({ "sandbox_id": "job", "argv": ["pytest", "-q"] }),
        ));
        assert_eq!(answer["decision"], "continue");
        assert_eq!(answer["mutations"]["arguments"]["sandbox_id"], "vm1");
        settle();
        let chain = r.store.audit.lock().unwrap().clone();
        assert_eq!(chain.len(), 1);
        assert!(
            chain[0]
                .record
                .detail
                .starts_with("continue sandbox::exec pytest -q"),
            "{:?}",
            chain[0]
        );
    }

    #[test]
    fn an_enforced_constraint_refuses_with_its_rule() {
        let r = rig();
        let answer = r.gate.judge(&envelope(
            "build",
            "sandbox::exec",
            json!({ "argv": ["bash", "-lc", "pytest"] }),
        ));
        assert_eq!(answer["decision"], "deny");
        assert!(
            answer["reason"]
                .as_str()
                .unwrap()
                .contains("no-login-shell")
        );
    }

    #[test]
    fn a_call_without_a_stage_or_before_the_policy_loads_is_refused() {
        let r = rig();
        let answer = r
            .gate
            .judge(&envelope("", "sandbox::exec", json!({ "argv": ["ls"] })));
        assert_eq!(answer["decision"], "deny");
        let log = AuditLog::start(r.store.clone(), KEY.to_vec(), crate::audit_log::WRITE_LIMIT);
        let unloaded = Gate::new(
            r.store.clone(),
            r.engine.clone(),
            Arc::new(|_: &str| {}),
            Ok(log),
        );
        assert_eq!(unloaded.judge(&write())["decision"], "deny");
        let unkeyed = Gate::new(
            r.store.clone(),
            r.engine.clone(),
            Arc::new(|_: &str| {}),
            Err("no key".into()),
        );
        tauri::async_runtime::block_on(unkeyed.reload()).unwrap();
        let answer = unkeyed.judge(&envelope(
            "build",
            "sandbox::exec",
            json!({ "argv": ["ls"] }),
        ));
        assert!(
            answer["reason"].as_str().unwrap().contains("no key"),
            "{answer}"
        );
    }

    #[test]
    fn allow_once_runs_exactly_that_call_and_the_next_one_waits_again() {
        let r = rig();
        assert_eq!(r.gate.judge(&write())["decision"], "hold");
        settle();
        assert_eq!(r.store.held.lock().unwrap().len(), 1);

        tauri::async_runtime::block_on(r.gate.allow_once("j1", "fc1")).unwrap();
        let calls = r.engine.calls.lock().unwrap().clone();
        let ran = calls.iter().find(|c| c.1 == "sandbox::fs::write").unwrap();
        assert_eq!(ran.2["sandbox_id"], "vm1");
        let resolved = calls
            .iter()
            .find(|c| c.1 == "harness::function::resolve")
            .unwrap();
        assert_eq!(
            (
                &resolved.2["function_call_id"],
                &resolved.2["turn_id"],
                &resolved.2["is_error"]
            ),
            (&json!("fc1"), &json!("t1"), &json!(false))
        );
        assert!(r.store.held.lock().unwrap().is_empty());

        assert_eq!(r.gate.judge(&write())["decision"], "hold");
    }

    #[test]
    fn refuse_delivers_the_feedback_as_an_error() {
        let r = rig();
        r.gate.judge(&write());
        settle();
        tauri::async_runtime::block_on(r.gate.refuse("j1", "fc1", "write the plan, not the code"))
            .unwrap();
        let calls = r.engine.calls.lock().unwrap().clone();
        assert!(!calls.iter().any(|c| c.1 == "sandbox::fs::write"));
        let resolved = calls
            .iter()
            .find(|c| c.1 == "harness::function::resolve")
            .unwrap();
        assert_eq!(resolved.2["is_error"], true);
        assert!(
            resolved.2["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("write the plan, not the code")
        );
    }

    #[test]
    fn promoting_a_constraint_changes_the_next_decision_and_is_recorded() {
        let r = rig();
        let rm = || {
            envelope(
                "build",
                "sandbox::exec",
                json!({ "argv": ["git", "rm", "test_calc.py"] }),
            )
        };
        assert_eq!(r.gate.judge(&rm())["decision"], "continue");
        tauri::async_runtime::block_on(async {
            r.gate.move_constraint("keep-tests", true).await.unwrap();
            r.gate.move_constraint("keep-tests", true).await.unwrap();
        });
        assert_eq!(r.gate.judge(&rm())["decision"], "deny");
        settle();
        let chain = r.store.audit.lock().unwrap().clone();
        let kinds: Vec<_> = chain.iter().map(|e| e.record.kind.as_str()).collect();
        assert_eq!(kinds, ["gate", "promote", "promote", "gate"]);
        assert_eq!(audit::verify(&chain, KEY), Ok(()));
    }

    fn charter() -> Charter {
        Charter {
            files: vec![".claude/settings.json".into()],
            constraints: bastion_core::charter::constraints(&["Bash(git push:*)".into()], &[]),
            before: vec![crate::charter::Hook {
                functions: vec!["sandbox::exec"],
                command: "./check.sh".into(),
            }],
            stop: vec![],
        }
    }

    fn hooked(exit_code: i64) -> Rig {
        let r = rig_with(
            FakeEngine::new(Link::Up)
                .answer("harness::function::resolve", Ok(json!({})))
                .answer(
                    "sandbox::exec",
                    Ok(json!({ "exit_code": exit_code, "stderr": "tests first" })),
                ),
        );
        r.gate.set_charter("j1", charter());
        r
    }

    #[test]
    fn a_charter_rule_refuses_and_reads_the_cmd_shape() {
        let r = hooked(0);
        let answer = r.gate.judge(&envelope(
            "build",
            "sandbox::exec",
            json!({ "cmd": "git push origin main" }),
        ));
        assert_eq!(answer["decision"], "deny");
        assert!(
            answer["reason"]
                .as_str()
                .unwrap()
                .contains("Bash(git push:*)")
        );
        assert!(r.gate.rules("j1").contains("Bash(git push:*)"));
    }

    #[test]
    fn a_hook_that_exits_2_refuses_the_call_with_its_output() {
        let r = hooked(2);
        let answer = r.gate.judge(&envelope(
            "build",
            "sandbox::exec",
            json!({ "argv": ["pytest"] }),
        ));
        assert_eq!(answer["decision"], "hold");
        settle();
        let calls = r.engine.calls.lock().unwrap().clone();
        let execs: Vec<_> = calls.iter().filter(|c| c.1 == "sandbox::exec").collect();
        assert_eq!(execs.len(), 1, "only the hook ran");
        assert!(
            execs[0].2["env"]["BASTION_HOOK_INPUT"]
                .as_str()
                .unwrap()
                .contains("\"command\":\"pytest\"")
        );
        let resolved = calls
            .iter()
            .find(|c| c.1 == "harness::function::resolve")
            .unwrap();
        assert_eq!(resolved.2["is_error"], true);
        assert!(
            resolved.2["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("tests first")
        );
        assert!(r.gate.hook_runs("j1")[0].contains("exit 2"));
    }

    #[test]
    fn a_hook_that_passes_lets_the_call_run_in_the_vm() {
        let r = hooked(0);
        r.gate.judge(&envelope(
            "build",
            "sandbox::exec",
            json!({ "argv": ["pytest"] }),
        ));
        settle();
        let calls = r.engine.calls.lock().unwrap().clone();
        let execs: Vec<_> = calls.iter().filter(|c| c.1 == "sandbox::exec").collect();
        assert_eq!(execs.len(), 2, "the hook, then the call");
        assert_eq!(execs[1].2["sandbox_id"], "vm1");
        let resolved = calls
            .iter()
            .find(|c| c.1 == "harness::function::resolve")
            .unwrap();
        assert_eq!(resolved.2["is_error"], false);
    }

    #[test]
    fn the_brief_names_every_rule() {
        let rules = rig().gate.rules("j1");
        for c in govern::default_constraints() {
            assert!(rules.contains(&c.rule), "{rules}");
        }
    }
}
