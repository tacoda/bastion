//! N7 (PLAN.md A3): can a job's code go into a VM, get a commit from an agent
//! turn there, and come back to its worktree?
//!
//! A scratch repo gets a worktree from `worktree::create`. Its branch goes into
//! a fresh VM as a git bundle, a `harness` turn appends a line and commits in
//! the VM through `sandbox::*` (pinned to that VM by a `pre-trigger` hook, as
//! N3), and the commit comes back as a bundle through the content channel (N4).
//!
//! Paid: one short turn, about $0.01 on gpt-5-mini. Run against the stack:
//! `cargo run -p bastion-spikes --example n7_roundtrip`.

#![allow(deprecated)]

use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result};
use base64::Engine as _;
use bastion_spikes::{Checks, call, call_default, connect, engine_url, ensure};
use iii_sdk::channel::{ChannelReader, StreamChannelRef};
use iii_sdk::protocol::RegisterTriggerInput;
use iii_sdk::{Error, RegisterFunction};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::time::timeout;

fn git(dir: &std::path::Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").args(args).current_dir(dir).output()?;
    ensure(
        out.status.success(),
        format!("git {args:?}: {}", String::from_utf8_lossy(&out.stderr)),
    )?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[tokio::main]
async fn main() {
    let iii = connect("spike-n7");
    let mut checks = Checks::default();
    let tag = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    let session_id = format!("s_spike_n7_{tag}");
    let repo = std::env::temp_dir().join(format!("bastion-n7-{tag}"));
    let mut box_id = String::new();
    let mut worktree_id = String::new();

    let (done_tx, mut done) = mpsc::channel::<Value>(1);
    iii.register_function(
        "bastion::spike::n7::pin",
        RegisterFunction::new_async(|envelope: Value| async move {
            let mut arguments = envelope["call"]["arguments"].clone();
            arguments["sandbox_id"] = envelope["metadata"]["sandbox_id"].clone();
            Ok::<_, Error>(
                json!({ "decision": "continue", "mutations": { "arguments": arguments } }),
            )
        }),
    );
    iii.register_function(
        "bastion::spike::n7::done",
        RegisterFunction::new_async(move |event: Value| {
            let done_tx = done_tx.clone();
            async move {
                let _ = done_tx.send(event).await;
                Ok::<_, Error>(json!({}))
            }
        }),
    );
    let _ = iii.wait_until_registered(Duration::from_secs(5)).await;

    checks
        .run("a commit made in the VM comes back to the job's worktree", async {
            // A scratch repository with one commit.
            std::fs::create_dir_all(&repo)?;
            git(&repo, &["init", "-q", "-b", "main"])?;
            std::fs::write(repo.join("README.md"), "# n7\n")?;
            git(&repo, &["add", "."])?;
            git(&repo, &["-c", "user.name=n7", "-c", "user.email=n7@local", "commit", "-q", "-m", "start"])?;

            let created = call(&iii, "worktree::create", json!({ "repo_path": repo.display().to_string(), "session_id": session_id })).await?;
            let path = created["path"].as_str().context(format!("no path: {created}"))?.to_string();
            worktree_id = created["worktree_id"].as_str().or(created["id"].as_str()).unwrap_or_default().to_string();
            let wt = std::path::PathBuf::from(&path);
            let base = git(&wt, &["rev-parse", "HEAD"])?;
            let branch = git(&wt, &["rev-parse", "--abbrev-ref", "HEAD"])?;

            // The branch goes into a fresh VM as a bundle.
            box_id = call_default(&iii, "sandbox::create", json!({ "image": "python", "network": false, "idle_timeout_secs": 600 }), 120_000).await?
                ["sandbox_id"].as_str().context("sandbox_id")?.to_string();
            let bundle = repo.with_extension("in.bundle");
            git(&wt, &["bundle", "create", &bundle.display().to_string(), "HEAD", &branch])?;
            let bytes = std::fs::read(&bundle)?;
            call_default(&iii, "sandbox::fs::write", json!({ "sandbox_id": box_id, "path": "/work/in.bundle", "content_b64": base64::engine::general_purpose::STANDARD.encode(&bytes), "parents": true }), 60_000).await?;
            let exec = |argv: Vec<String>| call_default(&iii, "sandbox::exec", json!({ "sandbox_id": box_id, "argv": argv, "timeout_ms": 60_000 }), 90_000);
            let cloned = exec(vec!["git".into(), "clone".into(), "-q".into(), "-b".into(), branch.clone(), "/work/in.bundle".into(), "/work/repo".into()]).await?;
            ensure(cloned["exit_code"] == 0 || cloned["exit_code"].is_null(), format!("clone: {cloned}"))?;

            // One turn appends a line and commits in the VM.
            let hook = iii.register_trigger(RegisterTriggerInput::new(
                "harness::hook::pre-trigger",
                "bastion::spike::n7::pin",
                json!({ "functions": ["sandbox::*"], "on_error": "fail_closed", "timeout_ms": 5000 }),
            ))?;
            let turn_done = iii.register_trigger(RegisterTriggerInput::new(
                "harness::turn-completed",
                "bastion::spike::n7::done",
                json!({ "session_id": session_id }),
            ))?;
            call(&iii, "approval::set-mode", json!({ "session_id": session_id, "mode": "full" })).await?;
            call(&iii, "harness::send", json!({
                "session_id": session_id, "model": "gpt-5-mini", "provider": "openai",
                "message": "In the git repository at /work/repo, append the line `n7 was here` to README.md, then commit it with git: run `git -C /work/repo -c user.name=agent -c user.email=agent@local commit -am \"n7\"`. Use sandbox::exec with an argv array. Reply DONE when the commit exists.",
                "options": { "max_turns": 6, "functions": { "allow": ["sandbox::exec", "sandbox::fs::read", "sandbox::fs::write"] }, "metadata": { "sandbox_id": box_id } },
            })).await?;
            timeout(Duration::from_secs(300), done.recv()).await?.context("turn-completed never fired")?;
            hook.unregister();
            turn_done.unregister();

            // The commit comes back as a bundle through the channel.
            let made = exec(vec!["git".into(), "-C".into(), "/work/repo".into(), "bundle".into(), "create".into(), "/work/out.bundle".into(), format!("{base}..{branch}")]).await?;
            ensure(made["exit_code"] == 0 || made["exit_code"].is_null(), format!("no new commit in the VM: {made}"))?;
            let read = call_default(&iii, "sandbox::fs::read", json!({ "sandbox_id": box_id, "path": "/work/out.bundle" }), 60_000).await?;
            let channel: StreamChannelRef = serde_json::from_value(read["content"].clone())?;
            let out = ChannelReader::new(&engine_url(), &channel).read_all().await?;
            let out_path = repo.with_extension("out.bundle");
            std::fs::write(&out_path, &out)?;
            git(&wt, &["pull", "-q", "--ff-only", &out_path.display().to_string(), &branch])?;
            let readme = std::fs::read_to_string(wt.join("README.md"))?;
            ensure(readme.contains("n7 was here"), format!("README: {readme:?}"))?;
            let head = git(&wt, &["log", "-1", "--format=%s"])?;
            Ok(format!("worktree {} got commit {head:?} from VM {}, in a {}-byte bundle", path, &box_id[..8], out.len()))
        })
        .await;

    if !box_id.is_empty() {
        let _ = call_default(
            &iii,
            "sandbox::stop",
            json!({ "sandbox_id": box_id, "wait": true }),
            60_000,
        )
        .await;
    }
    if !worktree_id.is_empty() {
        let _ = call(
            &iii,
            "worktree::remove",
            json!({ "worktree_id": worktree_id, "force": true, "delete_branch": true }),
        )
        .await;
    }
    let _ = std::fs::remove_dir_all(&repo);
    checks.finish(iii)
}
