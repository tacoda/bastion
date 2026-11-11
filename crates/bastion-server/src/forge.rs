//! The GitHub forge and tracker adapter (DECISIONS.md D23). Branches are
//! pushed from the job's worktree with the owner's own git; pull requests,
//! CI checks, merges and issues go through the stock `github` worker's `gh`.
//! A new forge is a new adapter beside this one; the core only sees
//! `PrOpened`, `CiPassed`, `CiFailed` and `Landed`.

use std::path::Path;

use serde_json::{Value, json};

use bastion_core::Job;

use crate::engine::Engine;
use crate::engine::NS;
use crate::machine::Machine;

/// The most of a failing run's log a fix turn reads.
const LOG_CHARS: usize = 4000;

/// Run `gh` through the github worker. A non-zero exit is an error with gh's stderr.
pub async fn gh(engine: &dyn Engine, args: &[&str]) -> Result<String, String> {
    let out = engine
        .call_for(NS, "github::exec", json!({ "args": args }), 120_000)
        .await?;
    match out["exit_code"].as_i64() {
        Some(0) => Ok(out["stdout"].as_str().unwrap_or_default().to_string()),
        _ => Err(format!(
            "gh {}: {}",
            args.join(" "),
            out["stderr"].as_str().unwrap_or_default().trim()
        )),
    }
}

/// `owner/name` from a GitHub remote URL, ssh or https.
pub fn slug_of(url: &str) -> Option<String> {
    let rest = url
        .trim()
        .strip_prefix("git@github.com:")
        .or_else(|| url.trim().strip_prefix("https://github.com/"))?;
    let slug = rest.trim_end_matches('/').trim_end_matches(".git");
    (slug.split('/').count() == 2).then(|| slug.to_string())
}

/// Whether a checkout has an `origin` remote to push to.
pub fn has_origin(machine: &dyn Machine, path: &Path) -> bool {
    let out = machine.run("git", &["remote", "get-url", "origin"], Some(path));
    out.is_ok_and(|o| o.success)
}

/// The GitHub repository a checkout's `origin` points at.
pub fn slug(machine: &dyn Machine, path: &Path) -> Result<String, String> {
    let out = machine.run("git", &["remote", "get-url", "origin"], Some(path))?;
    if !out.success {
        return Err(format!("{} has no origin remote", path.display()));
    }
    slug_of(&out.text).ok_or(format!(
        "origin is not a GitHub repository: {}",
        out.text.trim()
    ))
}

/// Push the worktree's HEAD as the branch `head`.
pub fn push(machine: &dyn Machine, path: &Path, head: &str) -> Result<(), String> {
    let refspec = format!("HEAD:refs/heads/{head}");
    let out = machine.run("git", &["push", "-q", "origin", &refspec], Some(path))?;
    if out.success {
        Ok(())
    } else {
        Err(format!("git push: {}", out.text.trim()))
    }
}

/// What a pull request carries beyond the request: what the job's
/// later steps wrote, what a person waived, and the issue it closes.
#[derive(Clone, Debug, Default)]
pub struct Notes {
    pub briefing: Option<String>,
    pub uat: Option<String>,
    pub proof: Option<String>,
    pub waived: Vec<String>,
    pub issue: Option<String>,
}

/// The most of one section a pull request's body keeps; GitHub takes
/// 65,536 characters in all.
const SECTION_CHARS: usize = 8_000;

/// What a pull request says: the request's first line as its title, the
/// request in full, then the plan, the briefing, the proof and what was
/// waived, each when there is one, and a line that closes the issue it came
/// from. It names no tool as its author.
pub fn describe(job: &Job, base: &str, head: &str, notes: &Notes) -> PullRequest {
    let request = &job.request;
    let first = request.lines().next().unwrap_or_default();
    let title: String = first.chars().take(72).collect();
    let waived = notes.waived.iter().map(|w| format!("- {w}\n")).collect();
    let sections = [
        ("Plan", job.plan.clone().unwrap_or_default()),
        ("Briefing", notes.briefing.clone().unwrap_or_default()),
        ("UAT", notes.uat.clone().unwrap_or_default()),
        ("Proof", notes.proof.clone().unwrap_or_default()),
        ("Waived", waived),
    ];
    let mut body = request.clone();
    for (heading, text) in sections.iter().filter(|(_, t)| !t.trim().is_empty()) {
        let kept: String = text.trim().chars().take(SECTION_CHARS).collect();
        body.push_str(&format!("\n\n## {heading}\n\n{kept}"));
    }
    let issue = notes.issue.as_deref();
    if let Some(number) = issue.and_then(|url| url.rsplit('/').next()) {
        body.push_str(&format!("\n\nCloses #{number}"));
    }
    PullRequest {
        head: head.into(),
        base: base.into(),
        title,
        body,
    }
}

/// Refuse a pull request whose title or body credits an AI or holds a
/// secret.
pub fn publishable(pr: &PullRequest) -> Result<(), String> {
    let found = bastion_core::publish::findings(&format!("{}\n{}", pr.title, pr.body));
    match found.is_empty() {
        true => Ok(()),
        false => Err(format!(
            "The pull request's text cannot be published: {}. Change the request's text, then retry.",
            found.join("; ")
        )),
    }
}

/// One GitHub repository, reached through the github worker.
pub struct GitHub<'a> {
    pub engine: &'a dyn Engine,
    pub slug: String,
}

/// What a new pull request says and where it goes.
pub struct PullRequest {
    pub head: String,
    pub base: String,
    pub title: String,
    pub body: String,
}

impl GitHub<'_> {
    async fn gh(&self, args: &[&str]) -> Result<String, String> {
        let mut all = args.to_vec();
        all.extend(["--repo", self.slug.as_str()]);
        gh(self.engine, &all).await
    }

    /// Open a pull request and give its URL.
    pub async fn open(&self, pr: &PullRequest) -> Result<String, String> {
        let args = [
            "pr", "create", "--head", &pr.head, "--base", &pr.base, "--title", &pr.title, "--body",
            &pr.body,
        ];
        Ok(self.gh(&args).await?.trim().to_string())
    }

    pub async fn checks(&self, pr: &str) -> Result<Checks, String> {
        let args = json!({ "args": ["pr", "checks", pr, "--repo", self.slug, "--json", "name,bucket,link"] });
        let out = self
            .engine
            .call_for(NS, "github::exec", args, 60_000)
            .await?;
        // gh exits 8 while checks are pending and 1 when one failed; the JSON is the answer.
        let parsed: Value =
            serde_json::from_str(out["stdout"].as_str().unwrap_or_default()).unwrap_or(Value::Null);
        let (state, runs) = read_checks(&parsed);
        if state != Checks::Failed(String::new()) {
            return Ok(state);
        }
        let mut log = String::new();
        for run in runs {
            log.push_str(
                &self
                    .gh(&["run", "view", &run, "--log-failed"])
                    .await
                    .unwrap_or_else(|e| e),
            );
        }
        Ok(Checks::Failed(for_fix(&log)))
    }

    /// The newest deployment of a branch (`ref=`) or a commit (`sha=`), and
    /// its newest status; none when nothing deploys it.
    pub async fn deployment(&self, filter: &str) -> Result<Option<Deployment>, String> {
        let api = |path: String| async move { gh(self.engine, &["api", &path]).await };
        let listed = api(format!(
            "repos/{}/deployments?{filter}&per_page=1",
            self.slug
        ))
        .await?;
        let Some(id) = first(&listed)["id"].as_u64() else {
            return Ok(None);
        };
        let statuses = api(format!(
            "repos/{}/deployments/{id}/statuses?per_page=1",
            self.slug
        ))
        .await?;
        Ok(Some(deployment_of(first(&statuses))))
    }

    /// The commit a merged pull request made on its base branch.
    pub async fn merge_commit(&self, pr: &str) -> Result<String, String> {
        let out = self
            .gh(&[
                "pr",
                "view",
                pr,
                "--json",
                "mergeCommit",
                "--jq",
                ".mergeCommit.oid",
            ])
            .await?;
        Some(out.trim().to_string())
            .filter(|sha| !sha.is_empty())
            .ok_or_else(|| format!("{pr} has no merge commit"))
    }

    /// Say the pull request's body again, as it stands before the merge.
    pub async fn edit(&self, pr: &str, body: &str) -> Result<(), String> {
        self.gh(&["pr", "edit", pr, "--body", body])
            .await
            .map(|_| ())
    }

    pub async fn merge(&self, pr: &str) -> Result<(), String> {
        self.gh(&["pr", "merge", pr, "--merge", "--delete-branch"])
            .await
            .map(|_| ())
    }

    pub async fn issues(&self) -> Result<Vec<Issue>, String> {
        let out = self
            .gh(&[
                "issue",
                "list",
                "--state",
                "open",
                "--json",
                "number,title,body,url",
            ])
            .await?;
        let all: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
        Ok(all
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(issue_of)
            .collect())
    }

    pub async fn issue(&self, number: u64) -> Result<Issue, String> {
        let n = number.to_string();
        let out = self
            .gh(&["issue", "view", &n, "--json", "number,title,body,url"])
            .await?;
        serde_json::from_str::<Value>(&out)
            .ok()
            .as_ref()
            .and_then(issue_of)
            .ok_or(format!("issue {number} did not read"))
    }
}

/// A `gh run view --log-failed` log as a fix turn should read it: each
/// line without its job, step and timestamp columns, and without the setup
/// lines GitHub folds into groups (the command and its environment).
/// It ends with the log's last `LOG_CHARS` characters.
fn for_fix(log: &str) -> String {
    let mut folded = false;
    let mut out = Vec::new();
    for line in log.lines() {
        let text = line.splitn(3, '\t').nth(2).unwrap_or(line);
        let text = text.trim_start_matches('\u{feff}');
        let text = match text.split_once(' ') {
            Some((stamp, rest)) if stamp.len() > 20 && stamp.ends_with('Z') => rest,
            _ => text,
        };
        if text.starts_with("##[group]") {
            folded = true;
        } else if text.starts_with("##[endgroup]") {
            folded = false;
        } else if !folded {
            out.push(text.trim_start_matches("##[error]"));
        }
    }
    last_chars(out)
}

/// The lines joined, cut to their last `LOG_CHARS` characters.
fn last_chars(lines: Vec<&str>) -> String {
    let chars: Vec<char> = lines.join("\n").chars().collect();
    if chars.is_empty() {
        return "A check failed and gave no log.".into();
    }
    chars[chars.len().saturating_sub(LOG_CHARS)..]
        .iter()
        .collect()
}

/// Where a deployment stands, and the address it serves, if it gave one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deployment {
    pub state: Deployed,
    pub url: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deployed {
    Pending,
    Succeeded,
    Failed,
}

/// The first item of a JSON array `gh api` printed, or null.
fn first(text: &str) -> Value {
    let all: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    all.get(0).cloned().unwrap_or(Value::Null)
}

/// A deployment status as GitHub gives it; a deployment with no status yet
/// is pending.
fn deployment_of(status: Value) -> Deployment {
    let state = match status["state"].as_str() {
        Some("success") => Deployed::Succeeded,
        Some("failure" | "error") => Deployed::Failed,
        _ => Deployed::Pending,
    };
    let url = status["environment_url"].as_str().filter(|u| !u.is_empty());
    Deployment {
        state,
        url: url.map(String::from),
    }
}

/// Where CI stands on a pull request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Checks {
    Pending,
    Passed,
    /// The failing runs' logs, cut to what a fix turn can read.
    Failed(String),
}

/// Read a pull request's checks from `gh pr checks --json name,bucket,link`.
pub fn read_checks(json: &Value) -> (Checks, Vec<String>) {
    let checks = json.as_array().cloned().unwrap_or_default();
    let bucket = |b: &str| checks.iter().filter(|c| c["bucket"] == b).count();
    let failed: Vec<String> = checks
        .iter()
        .filter(|c| c["bucket"] == "fail")
        .filter_map(|c| c["link"].as_str().and_then(run_id))
        .collect();
    let state = if checks.is_empty() || bucket("pending") > 0 {
        Checks::Pending
    } else if bucket("fail") > 0 || bucket("cancel") > 0 {
        Checks::Failed(String::new())
    } else {
        Checks::Passed
    };
    (state, failed)
}

/// The run id in a check's link, `.../actions/runs/<id>/job/<job>`.
fn run_id(link: &str) -> Option<String> {
    let after = link.split("/runs/").nth(1)?;
    Some(after.split('/').next()?.to_string())
}

/// One issue as a request source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub url: String,
}

fn issue_of(v: &Value) -> Option<Issue> {
    Some(Issue {
        number: v["number"].as_u64()?,
        title: v["title"].as_str()?.into(),
        body: v["body"].as_str().unwrap_or_default().into(),
        url: v["url"].as_str()?.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::fake::FakeEngine;
    use bastion_protocol::Link;

    #[test]
    fn a_checkout_has_an_origin_only_when_git_names_one() {
        let at = Path::new("/repo");
        let named = crate::machine::fake::FakeMachine::new().run_ok("git", "git@github.com:a/b");
        assert!(has_origin(&named, at));
        let none = crate::machine::fake::FakeMachine::new().run_fails("git", "No such remote");
        assert!(!has_origin(&none, at));
    }

    #[test]
    fn a_remote_url_gives_its_slug() {
        assert_eq!(
            slug_of("git@github.com:ianparo/bastion.git\n"),
            Some("ianparo/bastion".into())
        );
        assert_eq!(
            slug_of("https://github.com/ianparo/bastion"),
            Some("ianparo/bastion".into())
        );
        assert_eq!(slug_of("git@gitlab.com:o/r.git"), None);
        assert_eq!(slug_of("/tmp/demo-a"), None);
    }

    #[test]
    fn checks_read_as_pending_passed_or_failed_with_their_runs() {
        let link = "https://github.com/o/r/actions/runs/123/job/9";
        let pending = json!([{ "name": "test", "bucket": "pending", "link": link }]);
        assert_eq!(read_checks(&pending).0, Checks::Pending);
        assert_eq!(read_checks(&json!([])).0, Checks::Pending, "no checks yet");
        let passed = json!([{ "name": "test", "bucket": "pass", "link": link }, { "name": "x", "bucket": "skipping", "link": link }]);
        assert_eq!(read_checks(&passed).0, Checks::Passed);
        let failed = json!([{ "name": "test", "bucket": "fail", "link": link }]);
        assert_eq!(
            read_checks(&failed),
            (Checks::Failed(String::new()), vec!["123".to_string()])
        );
    }

    #[test]
    fn a_failed_check_brings_its_log() {
        let engine = FakeEngine::new(Link::Up).answer(
            "github::exec",
            Ok(json!({ "exit_code": 0, "stdout": "[{\"name\":\"test\",\"bucket\":\"fail\",\"link\":\"https://github.com/o/r/actions/runs/7/job/1\"}]" })),
        );
        let hub = GitHub {
            engine: &engine,
            slug: "o/r".into(),
        };
        let found = crate::rt::block_on(hub.checks("https://github.com/o/r/pull/1")).unwrap();
        let Checks::Failed(log) = found else {
            panic!("a failed check")
        };
        assert!(!log.is_empty());
        let calls = engine.calls.lock().unwrap();
        assert!(
            calls.iter().any(|c| c.2["args"][0] == "run"
                && c.2["args"][2] == "7"
                && c.2["args"][5] == "o/r")
        );
    }

    #[test]
    fn a_pull_request_closes_its_issue() {
        let job = Job::new("j1".into(), "w".into(), "Add modulo\nwith a test".into(), 1);
        let head = "bastion/j1";
        let notes = Notes {
            issue: Some("https://github.com/o/r/issues/4".into()),
            ..Notes::default()
        };
        let pr = describe(&job, "main", head, &notes);
        assert_eq!(
            (pr.title.as_str(), pr.head.as_str()),
            ("Add modulo", "bastion/j1")
        );
        assert!(pr.body.ends_with("Closes #4"));
        assert!(
            !describe(&job, "main", head, &Notes::default())
                .body
                .contains("Closes")
        );
        assert!(publishable(&pr).is_ok());
    }

    #[test]
    fn a_pull_request_carries_the_plan_briefing_proof_and_waivers() {
        let mut job = Job::new("j1".into(), "w".into(), "Add modulo".into(), 1);
        job.plan = Some("1. Add mod(a, b)".into());
        let notes = Notes {
            briefing: Some("Try mod(7, 3).".into()),
            uat: Some("1. Run mod(7, 3); see 1.".into()),
            proof: Some("```\n3 passed\n```\nVERDICT: yes".into()),
            waived: vec!["ci: shipped anyway by ada, waiving: lint".into()],
            issue: None,
        };
        let body = describe(&job, "main", "bastion/j1", &notes).body;
        let order =
            ["## Plan", "## Briefing", "## UAT", "## Proof", "## Waived"].map(|h| body.find(h));
        assert!(order.iter().all(Option::is_some), "{body}");
        assert!(order.is_sorted(), "{body}");
        assert!(body.contains("- ci: shipped anyway by ada, waiving: lint"));
        let bare = describe(&job, "main", "bastion/j1", &Notes::default()).body;
        assert!(!bare.contains("## Proof") && !bare.contains("## Waived"));
    }

    #[test]
    fn a_deployment_reads_its_newest_status_and_address() {
        let ok = json!({ "state": "success", "environment_url": "https://pr-1.example.dev" });
        assert_eq!(
            deployment_of(ok),
            Deployment {
                state: Deployed::Succeeded,
                url: Some("https://pr-1.example.dev".into())
            }
        );
        assert_eq!(
            deployment_of(json!({ "state": "error" })).state,
            Deployed::Failed
        );
        let none = deployment_of(Value::Null);
        assert_eq!((none.state, none.url), (Deployed::Pending, None));
        assert_eq!(first("[]"), Value::Null);
    }

    #[test]
    fn a_pull_request_that_credits_an_ai_is_not_published() {
        let request = "Fix it\n\nCo-Authored-By: Claude <noreply@anthropic.com>";
        let job = Job::new("j1".into(), "w".into(), request.into(), 1);
        let refused = publishable(&describe(&job, "main", "bastion/j1", &Notes::default()));
        assert!(refused.unwrap_err().contains("credits an author"));
    }

    #[test]
    fn a_log_keeps_its_errors_and_drops_its_columns_and_setup() {
        let log = "test\tChangelog\t\u{feff}2026-10-31T17:37:30.6227850Z ##[group]Run python check.py\n\
                   test\tChangelog\t2026-10-31T17:37:30.6255863Z   pythonLocation: /opt/x\n\
                   test\tChangelog\t2026-10-31T17:37:30.6257695Z ##[endgroup]\n\
                   test\tChangelog\t2026-10-31T17:37:30.6505436Z CHANGELOG.md has no entry for this change.\n\
                   test\tChangelog\t2026-10-31T17:37:30.6547741Z ##[error]Process completed with exit code 1.";
        assert_eq!(
            for_fix(log),
            "CHANGELOG.md has no entry for this change.\nProcess completed with exit code 1."
        );
    }

    #[test]
    fn a_long_log_keeps_its_end() {
        let log = "x".repeat(LOG_CHARS) + "the failure";
        assert!(for_fix(&log).ends_with("the failure"));
        assert_eq!(for_fix(&log).chars().count(), LOG_CHARS);
    }
}
