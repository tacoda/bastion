//! The GitHub tracker adapter (DECISIONS.md D23): a workspace's
//! repository by its `origin`, the owner's account, and issues as
//! requests, through the stock `github` worker's `gh`. Pull requests, CI
//! and merges are a workflow's own shell steps now (D39).

use std::path::Path;

use serde_json::{Value, json};

use crate::engine::Engine;
use crate::engine::NS;
use crate::machine::Machine;

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

/// One GitHub repository, reached through the github worker.
pub struct GitHub<'a> {
    pub engine: &'a dyn Engine,
    pub slug: String,
}

impl GitHub<'_> {
    async fn gh(&self, args: &[&str]) -> Result<String, String> {
        let mut all = args.to_vec();
        all.extend(["--repo", self.slug.as_str()]);
        gh(self.engine, &all).await
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
}
