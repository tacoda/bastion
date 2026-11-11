//! The workflows Bastion ships, as steps with their text filled in (D39).
//! Each is the owner's to change: one is saved only while no workflow has
//! its id, and a workflow seeded as stages is turned into steps only while
//! its first version is unchanged.

use bastion_core::workflow::{Graph, Link, Node, Reach};
use bastion_core::{Dial, Stage};

use crate::store::Store;
use crate::workflow_store::{self, Workflow};

/// A planning prompt: it reads, changes nothing, and gives the plan.
const PLAN: &str = "\
You are planning a change to the repository in /work/repo. Do not change any file.
Read what you need. Then reply with the plan, and nothing before it, in this shape:

RESULT plan:
1. The file to change, and what changes in it.
2. The test that proves it.

Write it in ASD-STE100 Simplified Technical English: one idea per sentence, at
most 20 words in an instruction and 25 in an explanation, active voice that
names what acts, and one word for each thing. Keep file, function and command
names exact.";

/// A building prompt: it carries out the plan linked into it.
const BUILD: &str = "\
Carry out the plan above in the repository in /work/repo. Change the code and
its tests, and run the tests until they pass. Do not weaken or delete a test to
make it pass. Commit what you change with git, with clear messages.";

/// A reviewing prompt: it reads the change against its base, and fixes it.
const REVIEW: &str = "\
Review the change in /work/repo: `git diff bastion-base` shows it. Check that
it does what the request asks. Look for bugs, missing tests and security
problems. Fix what you find and commit it. Reply with what you found and what
you fixed.";

/// A prompt that writes the acceptance criteria.
const CRITERIA: &str = "\
Write the acceptance criteria for the request: what its user can observe when
it is met. Do not change any file. Reply in this shape, and nothing before it:

RESULT criteria:
C1. One observable behaviour, in the requester's words.";

/// A prompt that writes failing acceptance tests for the criteria.
const ACCEPTANCE: &str = "\
Write acceptance tests for the criteria above. Drive the code the way its user
would. Change no product code. Each test must fail now, because the behaviour
is missing, not because the test is broken. Commit the tests.";

/// The workspace's checks, as its settings name them.
const CHECKS: &str = r#"if [ -z "$BASTION_CHECKS" ]; then
  echo "The workspace names no checks."
  exit 0
fi
sh -c "$BASTION_CHECKS""#;

/// What the GitHub steps share: a call to GitHub's API with the token.
const GITHUB: &str = r#"import json, os, sys, time, urllib.request

REPO = os.environ["BASTION_REPO"]
BRANCH = os.environ["BASTION_BRANCH"]


def call(method, path, body=None):
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(
        f"https://api.github.com/repos/{REPO}{path}",
        data=data,
        method=method,
        headers={
            "Authorization": "Bearer " + os.environ["GH_TOKEN"],
            "Accept": "application/vnd.github+json",
        },
    )
    with urllib.request.urlopen(request) as answer:
        return json.load(answer)


def pull_request():
    owner = REPO.split("/")[0]
    found = call("GET", f"/pulls?state=open&head={owner}:{BRANCH}")
    return found[0] if found else None
"#;

/// Push the branch with the token as a header, never in a URL, then open
/// its pull request if none is open.
const PULL_REQUEST: &str = r#"set -e
auth="Authorization: Basic $(printf 'x-access-token:%s' "$GH_TOKEN" | base64 | tr -d '\n')"
git -c http.extraHeader="$auth" push -q "https://github.com/$BASTION_REPO.git" "HEAD:refs/heads/$BASTION_BRANCH"
python3 - <<'PY'
@GITHUB@
import subprocess
pr = pull_request()
if pr is None:
    title = subprocess.run(["git", "log", "-1", "--format=%s"], capture_output=True, text=True).stdout.strip()
    pr = call("POST", "/pulls", {"title": title, "head": BRANCH, "base": os.environ["BASTION_BASE"]})
print(pr["html_url"])
PY"#;

/// Wait for the pull request's checks. The sandbox stops a command after
/// five minutes, so this waits four and a half, then asks for a retry.
const CI: &str = r#"python3 - <<'PY'
@GITHUB@
pr = pull_request()
if pr is None:
    sys.exit("No open pull request for " + BRANCH)
sha = pr["head"]["sha"]
for _ in range(27):
    runs = call("GET", f"/commits/{sha}/check-runs")["check_runs"]
    if runs and all(r["status"] == "completed" for r in runs):
        failed = [r["name"] for r in runs if r["conclusion"] not in ("success", "skipped", "neutral")]
        if failed:
            sys.exit("Failed: " + ", ".join(failed))
        print("Passed: " + ", ".join(r["name"] for r in runs))
        sys.exit(0)
    time.sleep(10)
waiting = ", ".join(r["name"] + " " + r["status"] for r in runs) or "none reported yet"
sys.exit("The checks are still running (" + waiting + "). Retry this step.")
PY"#;

/// Merge the pull request.
const MERGE: &str = r#"python3 - <<'PY'
@GITHUB@
pr = pull_request()
if pr is None:
    sys.exit("No open pull request for " + BRANCH)
merged = call("PUT", f"/pulls/{pr['number']}/merge", {"merge_method": "merge"})
print("Merged " + pr["html_url"] + " as " + merged["sha"])
PY"#;

/// The seeded workflows that are made of stages, by id.
const STAGED: [&str; 11] = [
    "research",
    "bug-fix",
    "dependency-upgrade",
    "test-coverage",
    "weekly-status",
    "decision-record",
    "onboarding-guide",
    "incident-response",
    "incident-postmortem",
    "runbook",
    "infrastructure-change",
];

/// One step, `x` from the left.
struct Step {
    id: &'static str,
    stage: Stage,
    dial: Dial,
    text: &'static str,
    results: &'static [&'static str],
    network: bool,
}

const fn prompt(id: &'static str, dial: Dial, text: &'static str) -> Step {
    Step {
        id,
        stage: Stage::Prompt,
        dial,
        text,
        results: &[],
        network: false,
    }
}

const fn shell(id: &'static str, dial: Dial, script: &'static str) -> Step {
    Step {
        stage: Stage::Shell,
        ..prompt(id, dial, script)
    }
}

/// A GitHub step: a shell script with the network and `GH_TOKEN`.
const fn github(id: &'static str, dial: Dial, script: &'static str) -> Step {
    Step {
        network: true,
        ..shell(id, dial, script)
    }
}

const PLAN_STEP: Step = Step {
    results: &["plan"],
    ..prompt("plan", Dial::LightsOn, PLAN)
};
const BUILD_STEP: Step = prompt("build", Dial::Dark, BUILD);

/// The steps a pull request adds after the checks.
const GITHUB_STEPS: [Step; 3] = [
    github("pull-request", Dial::Dark, PULL_REQUEST),
    github("ci", Dial::LightsOn, CI),
    github("merge", Dial::Dark, MERGE),
];

/// The workflows Bastion ships as steps: id, name, steps in order.
fn shipped() -> Vec<(&'static str, &'static str, Vec<Step>)> {
    let review = prompt("review", Dial::Dark, REVIEW);
    let criteria = Step {
        results: &["criteria"],
        ..prompt("criteria", Dial::LightsOn, CRITERIA)
    };
    let checks = |dial| shell("checks", dial, CHECKS);
    let to_github = |mut steps: Vec<Step>| {
        steps.extend(GITHUB_STEPS);
        steps
    };
    vec![
        (
            "short",
            "Short",
            vec![PLAN_STEP, BUILD_STEP, checks(Dial::LightsOn)],
        ),
        (
            "pull-request",
            "Pull request",
            to_github(vec![PLAN_STEP, BUILD_STEP, checks(Dial::Dark)]),
        ),
        (
            "full",
            "Full",
            to_github(vec![
                PLAN_STEP,
                criteria,
                prompt("acceptance-tests", Dial::Dark, ACCEPTANCE),
                BUILD_STEP,
                checks(Dial::Dark),
                prompt("review", Dial::Dark, REVIEW),
            ]),
        ),
        (
            "full_no_acceptance",
            "Full, without acceptance tests",
            to_github(vec![PLAN_STEP, BUILD_STEP, checks(Dial::Dark), review]),
        ),
    ]
}

/// The graph of steps in a row, each linked to the next.
fn chain(steps: Vec<Step>) -> Graph {
    let ids: Vec<&str> = steps.iter().map(|s| s.id).collect();
    let nodes = steps.into_iter().enumerate().map(|(i, s)| {
        let shell = s.stage == Stage::Shell;
        Node {
            id: s.id.into(),
            stage: s.stage,
            run: if shell {
                s.text.replace("@GITHUB@", GITHUB)
            } else {
                String::new()
            },
            dial: s.dial,
            note: if shell { String::new() } else { s.text.into() },
            results: s.results.iter().map(|r| r.to_string()).collect(),
            reach: Reach {
                network: s.network,
                secrets: if s.network {
                    vec!["GH_TOKEN".into()]
                } else {
                    Vec::new()
                },
            },
            x: 220 * i as i32,
            y: 0,
        }
    });
    let links = ids.windows(2).map(|w| Link {
        from: w[0].into(),
        to: w[1].into(),
    });
    Graph {
        nodes: nodes.collect(),
        links: links.collect(),
    }
}

/// A seeded workflow of stages as steps: plan reads and changes nothing,
/// build is the gate before the finish, and land goes, as the finish
/// lands (D41).
fn as_steps(g: &Graph) -> Graph {
    let nodes = g.nodes.iter().filter(|n| n.stage != Stage::Land).map(|n| {
        let mut n = n.clone();
        match n.stage {
            Stage::Plan => {
                n.stage = Stage::Prompt;
                n.note = format!("Do not change any file. {}", n.note);
            }
            Stage::Build => {
                n.stage = Stage::Prompt;
                n.dial = Dial::LightsOn;
            }
            _ => {}
        }
        n
    });
    let land: Vec<&str> = g
        .nodes
        .iter()
        .filter(|n| n.stage == Stage::Land)
        .map(|n| n.id.as_str())
        .collect();
    let links = g
        .links
        .iter()
        .filter(|l| !land.contains(&l.from.as_str()) && !land.contains(&l.to.as_str()));
    Graph {
        nodes: nodes.collect(),
        links: links.cloned().collect(),
    }
}

/// Save each shipped workflow no workflow has the id of, and turn each
/// seeded workflow still as seeded into steps.
pub async fn seed(store: &dyn Store) -> Result<(), String> {
    let kept = store.workflows().await?;
    let has = |id: &str| kept.iter().any(|k| k.0 == id);
    for (id, name, steps) in shipped() {
        if !has(id) {
            save(store, id, name, chain(steps)).await?;
        }
    }
    for (id, name, version) in kept
        .iter()
        .filter(|k| STAGED.contains(&k.0.as_str()) && k.2 == 1)
    {
        let Some(graph) = store.graph_at(id, *version).await? else {
            continue;
        };
        if graph
            .nodes
            .iter()
            .any(|n| matches!(n.stage, Stage::Plan | Stage::Build | Stage::Land))
        {
            save(store, id, name, as_steps(&graph)).await?;
        }
    }
    Ok(())
}

async fn save(store: &dyn Store, id: &str, name: &str, graph: Graph) -> Result<u64, String> {
    let w = Workflow {
        id: id.into(),
        name: name.into(),
        version: 0,
        graph,
    };
    workflow_store::save(store, &w, crate::factory::now()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_workflow_is_steps_only_and_checks() {
        for (id, _, steps) in shipped() {
            let g = chain(steps);
            assert_eq!(g.check(), Ok(()), "{id}");
            assert!(
                g.nodes
                    .iter()
                    .all(|n| bastion_core::workflow::custom(n.stage)),
                "{id}"
            );
            let script = g.nodes.iter().map(|n| n.run.as_str()).collect::<String>();
            assert!(!script.contains("@GITHUB@"), "{id}");
        }
        let pr = chain(shipped().remove(1).2);
        let merge = pr.nodes.iter().find(|n| n.id == "merge").unwrap();
        assert!(merge.reach.network && merge.reach.secrets == ["GH_TOKEN"]);
        assert!(merge.run.contains("def pull_request"));
    }

    #[test]
    fn a_seeded_workflow_of_stages_turns_into_steps_without_its_land() {
        let node = |id: &str, stage, note: &str| Node {
            id: id.into(),
            stage,
            run: String::new(),
            dial: Dial::Dark,
            note: note.into(),
            results: Vec::new(),
            reach: Reach::default(),
            x: 0,
            y: 0,
        };
        let link = |from: &str, to: &str| Link {
            from: from.into(),
            to: to.into(),
        };
        let staged = Graph {
            nodes: vec![
                node("plan", Stage::Plan, "Find the cause."),
                node("build", Stage::Build, "Fix it."),
                node("land", Stage::Land, ""),
            ],
            links: vec![link("plan", "build"), link("build", "land")],
        };
        let steps = as_steps(&staged);
        assert_eq!(steps.check(), Ok(()));
        assert_eq!(steps.nodes.len(), 2);
        assert_eq!(
            steps.nodes[0].note,
            "Do not change any file. Find the cause."
        );
        assert_eq!(
            (steps.nodes[1].stage, steps.nodes[1].dial),
            (Stage::Prompt, Dial::LightsOn)
        );
        assert_eq!(steps.links.len(), 1);
    }
}
