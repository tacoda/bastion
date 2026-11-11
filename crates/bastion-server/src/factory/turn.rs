//! A turn's life in the factory: its session names its job and its step, it
//! ends on its event or its cap, and it ends once. A turn's result reaches
//! the core only while the job still runs that turn, checked in the same
//! save that applies it, so a turn that ended twice, or a stale session
//! found after a restart, changes nothing.

use std::sync::Arc;
use std::time::Duration;

use bastion_core::{Event, Outcome, Stage, State};
use serde_json::json;

use super::resume::cap_left;
use super::{Factory, NS, now, outcome, short_id};
use crate::engine::Engine;
use crate::evals;
use crate::store::{Record, name};
use crate::{learn, spec};

/// One turn: the job it runs for and its session.
#[derive(Clone, Debug)]
pub struct Turn {
    pub job: String,
    pub session: String,
}

impl Turn {
    /// A new turn at `stage`: its session is `<job>-<step>-<random>`.
    pub fn start(job: &str, stage: Stage) -> Self {
        let session = format!("{job}-{}-{}", name(stage), short_id());
        Turn {
            job: job.into(),
            session,
        }
    }

    /// Whether this is the turn `record` runs now: its session, at its
    /// step, while it runs. A session from before steps were named in it
    /// counts at any step.
    pub fn owns(&self, record: &Record) -> bool {
        let job = &record.job;
        let named = self.session.strip_prefix(&format!("{}-", job.id));
        let step = named
            .and_then(|rest| rest.rsplit_once('-'))
            .map(|(step, _)| step);
        let at_step = step.is_none_or(|s| s == name(job.stage));
        let current = record.runtime.session.as_ref() == Some(&self.session);
        current && at_step && job.state == State::Running
    }
}

/// Refuse a turn whose provider the router has no key for, so it fails at
/// its start, naming the provider, instead of waiting out its cap with no
/// reply. Codex signs in instead of keeping a key, so it is not checked;
/// nor is a provider the router does not list, or a list it cannot give.
pub async fn provider_ready(engine: &dyn Engine, provider: &str) -> Result<(), String> {
    if provider == "openai-codex" {
        return Ok(());
    }
    let Ok(listed) = engine.call(NS, "router::provider::list", json!({})).await else {
        return Ok(());
    };
    let found = listed["providers"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|p| p["id"] == provider);
    match found {
        Some(p) if p["configured"] != true => Err(format!(
            "{provider} has no key: set one in Settings, or pick another model"
        )),
        _ => Ok(()),
    }
}

/// Cancel a session's turn, if one is still out. Best effort: a turn that
/// already ended has nothing to cancel.
pub async fn stop(engine: &dyn Engine, session: &str) {
    let _ = engine
        .call(NS, "harness::stop", json!({ "session_id": session }))
        .await;
}

/// The text of a session's last assistant message.
pub async fn final_reply(engine: &dyn Engine, session: &str) -> Result<String, String> {
    let out = engine
        .call(NS, "session::messages", json!({ "session_id": session }))
        .await?;
    out["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .find(|m| m["message"]["role"] == "assistant")
        .map(|m| {
            m["message"]["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .ok_or_else(|| format!("session {session} has no reply"))
}

impl Factory {
    pub(super) fn listen_for_turn(self: &Arc<Self>, turn: &Turn) -> Result<(), String> {
        let (factory, ended) = (self.clone(), turn.clone());
        let binding = self.engine.bind(
            "harness::turn-completed",
            json!({ "session_id": turn.session }),
            Arc::new(move |_| {
                let (factory, ended) = (factory.clone(), ended.clone());
                crate::rt::spawn(async move { factory.turn_done(&ended, false).await });
                json!({})
            }),
        )?;
        self.hold(&turn.job, binding);
        Ok(())
    }

    /// End the turn as failed if it is still running after `secs`. A turn
    /// a person extended meanwhile waits out what is left of its new cap.
    pub(super) fn arm_cap(self: &Arc<Self>, turn: &Turn, secs: u64) {
        let (factory, turn) = (self.clone(), turn.clone());
        crate::rt::spawn(async move {
            tokio::time::sleep(Duration::from_secs(secs)).await;
            let left = match factory.store.load(&turn.job).await {
                Ok(r) if turn.owns(&r) => cap_left(&r.job, now()),
                _ => 0,
            };
            if left > 0 {
                return factory.arm_cap(&turn, left);
            }
            factory.turn_done(&turn, true).await;
        });
    }

    /// A turn ended, or ran past its cap. Ignored when the job moved on, and
    /// when another call is finishing the same turn.
    pub(super) async fn turn_done(self: &Arc<Self>, turn: &Turn, timed_out: bool) {
        if !self.finishing.lock().unwrap().insert(turn.session.clone()) {
            return;
        }
        self.finish(turn, timed_out).await;
        self.finishing.lock().unwrap().remove(&turn.session);
    }

    async fn finish(self: &Arc<Self>, turn: &Turn, timed_out: bool) {
        let Ok(record) = self.store.load(&turn.job).await else {
            return;
        };
        if !turn.owns(&record) {
            return;
        }
        let Some(result) = self.result(&record, turn, timed_out).await else {
            return;
        };
        self.release(&turn.job);
        evals::meter(&*self.engine, &*self.store, &record.job, &turn.session).await;
        self.tidy_after_turn(&record).await;
        let still = turn.clone();
        let holds = Arc::new(move |r: &Record| still.owns(r));
        let finished = Event::TurnFinished { outcome: result };
        let _ = self.apply_when(&turn.job, finished, holds).await;
    }

    /// What the turn came to, or none while it still runs.
    async fn result(&self, record: &Record, turn: &Turn, timed_out: bool) -> Option<Outcome> {
        if timed_out {
            stop(&*self.engine, &turn.session).await;
            let minutes = bastion_core::TURN_CAP_SECS / 60;
            let error = format!("the turn ran past {minutes} minutes");
            return Some(Outcome::Failed { error });
        }
        let asked = json!({ "session_id": turn.session });
        let status = match self.engine.call(NS, "harness::status", asked).await {
            Ok(status) => status,
            Err(error) => return Some(Outcome::Failed { error }),
        };
        if status["status"] != "completed" {
            return None;
        }
        if let Some(error) = status["result_error"].as_str() {
            return Some(Outcome::Failed {
                error: error.into(),
            });
        }
        Some(match final_reply(&*self.engine, &turn.session).await {
            Ok(reply) => usable(record.job.stage, outcome(&reply)),
            Err(error) => Outcome::Failed { error },
        })
    }
}

/// A specify or learn reply the server cannot read goes back to the core as
/// unusable, so the turn gets one more try with the reason.
pub(super) fn usable(stage: Stage, outcome: Outcome) -> Outcome {
    let read = match (stage, &outcome) {
        (Stage::Specify, Outcome::Done { report }) => spec::read(report).err(),
        (Stage::Learn, Outcome::Done { report }) => learn::read(report).err(),
        _ => None,
    };
    match (read, outcome) {
        (Some(problem), Outcome::Done { report }) => Outcome::Unusable { report, problem },
        (_, other) => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Runtime;
    use bastion_core::Job;

    fn record(stage: Stage, session: &str) -> Record {
        let mut job = Job::new("j1-2".into(), "w".into(), "r".into(), 0);
        (job.stage, job.state) = (stage, State::Running);
        let runtime = Runtime {
            session: Some(session.into()),
            ..Runtime::default()
        };
        Record {
            job,
            runtime,
            rev: 1,
            created_at: 0,
        }
    }

    fn turn(session: &str) -> Turn {
        Turn {
            job: "j1-2".into(),
            session: session.into(),
        }
    }

    #[test]
    fn a_turn_counts_only_at_the_step_it_was_started_for() {
        let build = Turn::start("j1-2", Stage::Build);
        assert!(build.session.starts_with("j1-2-build-"));
        assert!(build.owns(&record(Stage::Build, &build.session)));
        assert!(
            !build.owns(&record(Stage::Verify, &build.session)),
            "a build's reply at verify"
        );
        assert!(!turn("j1-2-build-other").owns(&record(Stage::Build, &build.session)));
        let review = Turn::start("j1-2", Stage::CodeReview);
        assert!(review.owns(&record(Stage::CodeReview, &review.session)));
    }

    #[test]
    fn a_turn_on_a_provider_without_a_key_is_refused_by_name() {
        use crate::engine::fake::FakeEngine;
        use bastion_protocol::Link;
        let listed = json!({ "providers": [
            { "id": "openai", "configured": false },
            { "id": "deepseek", "configured": true },
        ]});
        let engine = FakeEngine::new(Link::Up).answer("router::provider::list", Ok(listed));
        crate::rt::block_on(async {
            let refused = provider_ready(&engine, "openai").await.unwrap_err();
            assert!(refused.starts_with("openai has no key"), "{refused}");
            assert!(provider_ready(&engine, "deepseek").await.is_ok());
            assert!(provider_ready(&engine, "openai-codex").await.is_ok());
            assert!(provider_ready(&engine, "unlisted").await.is_ok());
        });
    }

    #[test]
    fn a_session_from_before_steps_were_named_counts_at_any_step() {
        assert!(turn("j1-2-a1b2c3d4").owns(&record(Stage::Verify, "j1-2-a1b2c3d4")));
    }
}
