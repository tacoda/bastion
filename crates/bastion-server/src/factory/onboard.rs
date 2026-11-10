//! The onboarding turn: an agent reads a repository in an offline VM and
//! drafts a new workspace's settings. It belongs to no job, so nothing is
//! saved; a person reads the draft, changes it, and adds the workspace.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bastion_core::Stage;
use serde::Deserialize;
use serde_json::json;

use super::setup::Repo;
use super::{Factory, MAX_TURN_STEPS, NS, TOOL_GUIDE, TOOLS, short_id, turn};
use crate::settings_store;
use crate::store::Workspace;

/// How long the onboarding turn may run.
pub const ONBOARD_SECS: u64 = 300;

/// The brief for the onboarding turn.
const BRIEF: &str = "\
Read this repository and draft the settings Bastion needs to work in it.
Do not change any file. Look at the build files, the CI configuration and the README.
Reply with one JSON object and nothing after it:
{\"checks\": \"...\", \"setup\": \"...\", \"cleanup\": \"...\", \"env\": \"...\"}
- checks: one shell command that runs the tests and the linters, as CI does.
- setup: one shell command that installs what the checks need, such as dependencies.
  It runs once per job, with the network. Leave it empty if nothing must be installed.
- cleanup: one shell command that removes what setup started. Usually empty.
- env: KEY=VALUE pairs, separated by spaces, that the checks need. Never a secret.
";

/// The settings the onboarding turn drafts.
#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Draft {
    pub checks: String,
    pub setup: String,
    pub cleanup: String,
    pub env: String,
}

impl Draft {
    /// The JSON object in a reply: from its first `{` to its last `}`.
    pub fn read(reply: &str) -> Result<Self, String> {
        let (Some(start), Some(end)) = (reply.find('{'), reply.rfind('}')) else {
            return Err("the onboarding turn replied with no settings".into());
        };
        serde_json::from_str(&reply[start..=end])
            .map_err(|e| format!("the onboarding turn's settings are not JSON: {e}"))
    }
}

impl Factory {
    /// Draft the settings for the repository at `path`, on `branch`.
    pub async fn draft(&self, path: &Path, branch: &str) -> Result<Draft, String> {
        let id = format!("onboard-{}", short_id());
        let bundle = self.bundle(&id, path, branch)?;
        let vm = self.create_vm(false, &Workspace::default()).await?;
        let repo = Repo {
            bundle,
            branch,
            base: "",
        };
        let reply = match self.clone_into(&vm, &repo).await {
            Ok(()) => self.onboard_turn(&id, &vm).await,
            Err(e) => Err(e),
        };
        self.stop_vm(&vm).await;
        Draft::read(&reply?)
    }

    /// Run the onboarding turn in `vm`, and wait for its reply.
    async fn onboard_turn(&self, id: &str, vm: &str) -> Result<String, String> {
        let session = format!("{id}-{}", short_id());
        let (done, ended) = tokio::sync::oneshot::channel();
        let done = Mutex::new(Some(done));
        let binding = self.engine.bind(
            "harness::turn-completed",
            json!({ "session_id": session }),
            Arc::new(move |_| {
                if let Some(done) = done.lock().unwrap().take() {
                    let _ = done.send(());
                }
                json!({})
            }),
        )?;
        let reply = match self.send_onboard(id, vm, &session).await {
            Ok(()) => self.onboard_reply(&session, ended).await,
            Err(e) => Err(e),
        };
        self.engine.unbind(&binding);
        reply
    }

    async fn send_onboard(&self, id: &str, vm: &str, session: &str) -> Result<(), String> {
        let settings = settings_store::current(&*self.store).await?;
        let model = settings.model(Stage::Plan);
        turn::provider_ready(&*self.engine, &model.provider).await?;
        let mode = json!({ "session_id": session, "mode": "full" });
        self.engine.call(NS, "approval::set-mode", mode).await?;
        // The gate judges the turn's calls as a read-only step's.
        let send = json!({
            "session_id": session, "provider": model.provider, "model": model.model,
            "message": format!("{TOOL_GUIDE}\n\n{BRIEF}"),
            "options": { "max_turns": MAX_TURN_STEPS, "functions": { "allow": TOOLS },
                "metadata": { "sandbox_id": vm, "job": id, "stage": Stage::Discover } },
        });
        self.engine
            .call_for(NS, "harness::send", send, 60_000)
            .await
            .map(|_| ())
    }

    /// The turn's reply once it ends, or why it did not end in time.
    async fn onboard_reply(
        &self,
        session: &str,
        ended: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<String, String> {
        let waited = tokio::time::timeout(Duration::from_secs(ONBOARD_SECS), ended).await;
        if waited.is_err() {
            turn::stop(&*self.engine, session).await;
            return Err(format!(
                "the onboarding turn ran past {} minutes",
                ONBOARD_SECS / 60
            ));
        }
        turn::final_reply(&*self.engine, session).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Engine;
    use crate::engine::fake::FakeEngine;
    use crate::machine::fake::FakeMachine;
    use crate::store::Store;
    use crate::store::fake::FakeStore;
    use bastion_protocol::Link;

    const REPLY: &str =
        "I read it.\n{\"checks\": \"cargo test\", \"setup\": \"\", \"env\": \"CI=1\"}";

    #[test]
    fn a_draft_is_the_json_object_in_the_reply() {
        let draft = Draft::read(REPLY).unwrap();
        assert_eq!(draft.checks, "cargo test");
        assert_eq!(draft.env, "CI=1");
        assert_eq!(draft.cleanup, "");
        assert!(Draft::read("no settings").is_err());
        assert!(Draft::read("{checks}").is_err());
    }

    #[test]
    fn the_onboarding_turn_runs_read_only_in_an_offline_vm_and_drafts_settings() {
        let messages = json!({ "messages": [
            { "message": { "role": "assistant", "content": [{ "type": "text", "text": REPLY }] } },
        ]});
        let engine = Arc::new(
            FakeEngine::new(Link::Up)
                .answer("sandbox::create", Ok(json!({ "sandbox_id": "vm1" })))
                .answer("sandbox::fs::write", Ok(json!({})))
                .answer("sandbox::exec", Ok(json!({ "exit_code": 0 })))
                .answer("sandbox::stop", Ok(json!({})))
                .answer("approval::set-mode", Ok(json!({})))
                .answer("harness::send", Ok(json!({})))
                .answer("session::messages", Ok(messages)),
        );
        let factory = Factory::new(
            engine.clone() as Arc<dyn Engine>,
            Arc::new(FakeStore::default()) as Arc<dyn Store>,
            Arc::new(FakeMachine::new().run_ok("git", "")),
            Arc::new(|_| {}),
            "/scratch".into(),
        );
        crate::rt::block_on(async {
            let drafting = factory.clone();
            let draft =
                tokio::spawn(async move { drafting.draft(Path::new("/repo"), "main").await });
            let sent = loop {
                let calls = engine.calls.lock().unwrap().clone();
                if let Some(c) = calls.iter().find(|c| c.1 == "harness::send") {
                    break c.2.clone();
                }
                drop(calls);
                tokio::time::sleep(Duration::from_millis(10)).await;
            };
            engine.fire("harness::turn-completed", "session_id", sent.clone());
            assert_eq!(draft.await.unwrap().unwrap().checks, "cargo test");
            assert_eq!(sent["options"]["metadata"]["stage"], "discover");
            let calls = engine.calls.lock().unwrap();
            let vm = calls.iter().find(|c| c.1 == "sandbox::create").unwrap();
            assert_eq!(vm.2["network"], false);
            assert!(calls.iter().any(|c| c.1 == "sandbox::stop"));
        });
    }
}
