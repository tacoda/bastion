//! The tables, the columns added since, and the numbered migrations.

use bastion_core::Stage;
use serde_json::{Value, json};

use super::*;

/// `CHECK (column IN (...))` over every stage the core has.
pub(super) fn stage_check(column: &str) -> String {
    let names: Vec<String> = Stage::ALL
        .iter()
        .map(|s| format!("'{}'", name(s)))
        .collect();
    format!("CHECK ({column} IN ({}))", names.join(","))
}

/// The tables whose rows name a stage, with the columns a rebuild keeps.
/// SQLite cannot change a `CHECK` in place, so a new stage in the core means
/// a new migration that rebuilds these (`migrations`).
pub(super) fn staged_tables() -> [(&'static str, &'static str, String); 4] {
    let stage = stage_check("stage");
    [
        jobs_table(&stage),
        (
            "artifacts",
            "id, job, stage, step, body, created_at",
            format!(
                "CREATE TABLE IF NOT EXISTS artifacts (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        job TEXT NOT NULL REFERENCES jobs(id),
        stage TEXT NOT NULL {stage},
        step TEXT NOT NULL DEFAULT '',
        body TEXT NOT NULL,
        created_at INTEGER NOT NULL)"
            ),
        ),
        (
            "capabilities",
            "stage, step, function, rung",
            format!(
                "CREATE TABLE IF NOT EXISTS capabilities (
        stage TEXT NOT NULL {stage},
        step TEXT NOT NULL DEFAULT '',
        function TEXT NOT NULL CHECK (length(function) > 0),
        rung TEXT NOT NULL CHECK (rung IN ('denied','ask','allowed')),
        PRIMARY KEY (stage, step, function))"
            ),
        ),
        turns_table(&stage),
    ]
}

/// `jobs`, under the stage check `stage`.
pub(super) fn jobs_table(stage: &str) -> (&'static str, &'static str, String) {
    let back_to = stage_check("back_to");
    (
        "jobs",
        LEGACY_JOB_COLUMNS,
        format!(
                "CREATE TABLE IF NOT EXISTS jobs (
        id TEXT PRIMARY KEY,
        workspace TEXT NOT NULL REFERENCES workspaces(name),
        request TEXT NOT NULL CHECK (length(request) > 0),
        stage TEXT NOT NULL {stage},
        state TEXT NOT NULL CHECK (state IN ('queued','provisioning','running','needs_you','landed','failed','stopped')),
        question TEXT,
        feedback TEXT,
        plan TEXT,
        failures INTEGER NOT NULL CHECK (failures >= 0),
        error TEXT,
        worktree TEXT,
        sandbox TEXT,
        session TEXT,
        rev INTEGER NOT NULL CHECK (rev >= 1),
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        pr TEXT,
        ci_fixes INTEGER NOT NULL DEFAULT 0 CHECK (ci_fixes >= 0),
        issue TEXT,
        pipeline TEXT NOT NULL DEFAULT 'short' CHECK (pipeline IN ('short','full','full_no_acceptance')),
        back_to TEXT {back_to},
        last_refusal TEXT,
        done_tasks INTEGER NOT NULL DEFAULT 0 CHECK (done_tasks >= 0),
        criteria TEXT,
        falsify_tried INTEGER NOT NULL DEFAULT 0 CHECK (falsify_tried IN (0, 1)),
        requester TEXT,
        workflow TEXT,
        workflow_version INTEGER,
        node TEXT,
        parent TEXT,
        after_job TEXT,
        part TEXT,
        approved TEXT,
        model TEXT)"
            ),
    )
}

/// `turns`, under the stage check `stage`.
pub(super) fn turns_table(stage: &str) -> (&'static str, &'static str, String) {
    (
        "turns",
        TURN_COLUMNS,
        format!(
            "CREATE TABLE IF NOT EXISTS turns (
    session TEXT PRIMARY KEY,
    job TEXT NOT NULL REFERENCES jobs(id),
    stage TEXT NOT NULL {stage},
    turns INTEGER NOT NULL CHECK (turns >= 0),
    calls INTEGER NOT NULL CHECK (calls >= 0),
    call_errors INTEGER NOT NULL CHECK (call_errors >= 0),
    cost_micros INTEGER NOT NULL CHECK (cost_micros >= 0),
    ended_at INTEGER NOT NULL)"
        ),
    )
}

/// The columns of `turns`, which named only the short pipeline's
/// stages until migration 2.
pub(super) const TURN_COLUMNS: &str =
    "session, job, stage, turns, calls, call_errors, cost_micros, ended_at";

/// The schema (D8): typed columns with constraints, no JSON in a text column.
pub(super) fn schema() -> Vec<String> {
    let staged = staged_tables().map(|(_, _, sql)| sql);
    SCHEMA.map(String::from).into_iter().chain(staged).collect()
}

pub(super) const SCHEMA: [&str; 8] = [
    "CREATE TABLE IF NOT EXISTS workspaces (
        name TEXT PRIMARY KEY CHECK (length(name) > 0),
        path TEXT NOT NULL UNIQUE,
        branch TEXT NOT NULL CHECK (length(branch) > 0),
        created_at INTEGER NOT NULL,
        checks TEXT NOT NULL DEFAULT '',
        max_jobs INTEGER CHECK (max_jobs > 0),
        revision_budget INTEGER CHECK (revision_budget > 0),
        branch_prefix TEXT NOT NULL DEFAULT 'bastion/',
        setup TEXT NOT NULL DEFAULT '',
        cleanup TEXT NOT NULL DEFAULT '',
        env TEXT NOT NULL DEFAULT '',
        gates TEXT NOT NULL DEFAULT '',
        autofix TEXT NOT NULL DEFAULT '',
        image TEXT NOT NULL DEFAULT 'python',
        finish TEXT NOT NULL DEFAULT 'merge' CHECK (finish IN ('merge','leave')))",
    // Discovery's tasks for implement, in order.
    "CREATE TABLE IF NOT EXISTS tasks (
        job TEXT NOT NULL REFERENCES jobs(id),
        n INTEGER NOT NULL CHECK (n >= 0),
        text TEXT NOT NULL CHECK (length(text) > 0),
        PRIMARY KEY (job, n))",
    // The results a job's steps gave (D35), each kept as its document.
    "CREATE TABLE IF NOT EXISTS job_results (
        job TEXT NOT NULL REFERENCES jobs(id),
        name TEXT NOT NULL CHECK (length(name) > 0),
        body TEXT NOT NULL,
        PRIMARY KEY (job, name))",
    // How many times each step sent a job's work back.
    "CREATE TABLE IF NOT EXISTS revisions (
        job TEXT NOT NULL REFERENCES jobs(id),
        stage TEXT NOT NULL CHECK (length(stage) > 0),
        count INTEGER NOT NULL CHECK (count >= 1),
        PRIMARY KEY (job, stage))",
    "CREATE TABLE IF NOT EXISTS dials (
        checkpoint TEXT PRIMARY KEY CHECK (checkpoint IN ('approve','review')),
        dial TEXT NOT NULL CHECK (dial IN ('ask','hold','auto')))",
    "CREATE TABLE IF NOT EXISTS constraints (
        id TEXT PRIMARY KEY CHECK (length(id) > 0),
        rule TEXT NOT NULL CHECK (length(rule) > 0),
        function TEXT NOT NULL,
        contains TEXT NOT NULL,
        rung TEXT NOT NULL CHECK (rung IN ('told','checked','enforced')),
        source TEXT NOT NULL CHECK (source IN ('charter','owner')))",
    // `arguments` is the engine's JSON, kept opaque to run the call later.
    "CREATE TABLE IF NOT EXISTS held_calls (
        id TEXT PRIMARY KEY,
        job TEXT NOT NULL REFERENCES jobs(id),
        session TEXT NOT NULL,
        turn TEXT NOT NULL,
        function TEXT NOT NULL,
        text TEXT NOT NULL,
        arguments TEXT NOT NULL,
        at INTEGER NOT NULL)",
    // The keyed chain (D21). An older unkeyed `audit` table, if any, is left as it was.
    "CREATE TABLE IF NOT EXISTS audit_log (
        seq INTEGER PRIMARY KEY CHECK (seq >= 1),
        at INTEGER NOT NULL,
        kind TEXT NOT NULL,
        subject TEXT NOT NULL,
        detail TEXT NOT NULL,
        prev TEXT NOT NULL CHECK (length(prev) = 64),
        hash TEXT NOT NULL UNIQUE CHECK (length(hash) = 64))",
];

/// The statements in a seed file (D26): `--` comment lines dropped, split on `;`.
// ponytail: a plain split; a seed value that holds a `;` needs a real SQL tokenizer.
pub fn seed_statements(sql: &str) -> Vec<Value> {
    let kept: String = sql
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    kept.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| json!({ "sql": s, "params": [] }))
        .collect()
}

pub(super) const JOB_COLUMNS: &str = "id, workspace, request, stage, state, question, feedback, plan, failures, error, worktree, sandbox, session, rev, created_at, updated_at, pr, ci_fixes, issue, pipeline, back_to, last_refusal, done_tasks, criteria, falsify_tried, requester, workflow, workflow_version, node, parent, after_job, part, approved, model";
/// The jobs the admission pump and resume need: every unfinished one, and
/// every job a queued part waits on, so a part whose earlier part failed
/// is seen as abandoned rather than admitted.
pub(super) const ACTIVE: &str = "state NOT IN ('landed','failed','stopped')";
/// The job columns before migration 1, which its rebuild copies.
pub(super) const LEGACY_JOB_COLUMNS: &str = "id, workspace, request, stage, state, question, feedback, plan, failures, error, worktree, sandbox, session, rev, created_at, updated_at, pr, ci_fixes, issue";

/// Columns added after a table was first made. Each runs once; on a table
/// that already has the column it fails, and that failure is expected.
// They run before the numbered migrations, which need these columns to exist.
pub(super) const ALTERS: [&str; 33] = [
    "ALTER TABLE jobs ADD COLUMN pr TEXT",
    "ALTER TABLE jobs ADD COLUMN ci_fixes INTEGER NOT NULL DEFAULT 0 CHECK (ci_fixes >= 0)",
    "ALTER TABLE jobs ADD COLUMN issue TEXT",
    "ALTER TABLE workspaces ADD COLUMN checks TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE jobs ADD COLUMN requester TEXT",
    "ALTER TABLE jobs ADD COLUMN workflow TEXT",
    "ALTER TABLE jobs ADD COLUMN workflow_version INTEGER",
    "ALTER TABLE jobs ADD COLUMN node TEXT",
    "ALTER TABLE jobs ADD COLUMN parent TEXT",
    "ALTER TABLE jobs ADD COLUMN after_job TEXT",
    "ALTER TABLE jobs ADD COLUMN part TEXT",
    "ALTER TABLE jobs ADD COLUMN approved TEXT",
    "ALTER TABLE workspaces ADD COLUMN max_jobs INTEGER CHECK (max_jobs > 0)",
    "ALTER TABLE workspaces ADD COLUMN branch_prefix TEXT NOT NULL DEFAULT 'bastion/'",
    "ALTER TABLE workspaces ADD COLUMN setup TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE workspaces ADD COLUMN cleanup TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE workspaces ADD COLUMN env TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE workspaces ADD COLUMN gates TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE workspaces ADD COLUMN autofix TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE workspaces ADD COLUMN revision_budget INTEGER CHECK (revision_budget > 0)",
    "ALTER TABLE eval_runs ADD COLUMN contract INTEGER CHECK (contract IN (0, 1))",
    "ALTER TABLE jobs ADD COLUMN model TEXT",
    "ALTER TABLE workspaces ADD COLUMN image TEXT NOT NULL DEFAULT 'python'",
    "ALTER TABLE workspaces ADD COLUMN finish TEXT NOT NULL DEFAULT 'merge' CHECK (finish IN ('merge','leave'))",
    "ALTER TABLE workflow_nodes ADD COLUMN network INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE workflow_nodes ADD COLUMN secrets TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE workflow_nodes ADD COLUMN reads_only INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE artifacts ADD COLUMN step TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE workflow_nodes ADD COLUMN model TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE workflow_nodes ADD COLUMN revisions INTEGER CHECK (revisions >= 0)",
    "ALTER TABLE workflow_nodes ADD COLUMN spend_micros INTEGER CHECK (spend_micros >= 0)",
    "ALTER TABLE capabilities ADD COLUMN step TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE constraints ADD COLUMN steps TEXT NOT NULL DEFAULT ''",
];

/// Numbered migrations, each run once in order; `PRAGMA user_version` is the
/// last one applied. Migration 1 rebuilds the tables that name a stage, for
/// the full pipeline; migration 2 rebuilds `turns`, which it missed;
/// migration 3 rebuilds them all for a workflow's own steps (D35).
pub(super) fn migrations() -> Vec<Vec<String>> {
    let staged = staged_tables();
    // Every column now, as the alters ran before the migrations: a rebuild
    // with fewer would drop the alters' columns' values.
    let current: Vec<_> = staged
        .iter()
        .map(|(table, columns, create)| {
            let columns = if *table == "jobs" {
                JOB_COLUMNS
            } else {
                columns
            };
            (*table, columns, create.clone())
        })
        .collect();
    // Before migration 4 the stage `command` was a shell step; from it until
    // D45 it was one of the repository's commands, which is now a prompt.
    let (old, new) = (|t| rebuild(t, "shell"), |t| rebuild(t, "prompt"));
    vec![
        staged.iter().flat_map(old).collect(),
        staged
            .iter()
            .filter(|t| t.0 == "turns")
            .flat_map(old)
            .collect(),
        current.iter().flat_map(old).collect(),
        // A shell step was the stage `command`; `command` now names one of
        // the repository's commands, and agents joined the stages.
        current
            .iter()
            .flat_map(old)
            .chain(
                staged
                    .iter()
                    .map(|t| t.0)
                    .chain(["workflow_nodes"])
                    .map(|table| {
                        format!("UPDATE {table} SET stage = 'shell' WHERE stage = 'command'")
                    }),
            )
            .collect(),
        // A workflow is its steps (D39): a stage an agent ran becomes a
        // prompt, one that only read it reading only, and a stage the
        // factory ran goes with its links, as the finish lands the job.
        current
            .iter()
            .flat_map(new)
            .chain(nodes_as_steps())
            .collect(),
        // A grant may be for one workflow step (D44), so its key gains the step.
        current
            .iter()
            .filter(|t| t.0 == "capabilities")
            .flat_map(new)
            .collect(),
        // A repository's command is a prompt that reads its file (D45).
        current
            .iter()
            .flat_map(new)
            .chain([COMMANDS_AS_PROMPTS.to_string()])
            .collect(),
    ]
}

/// A command step as the prompt that does what it did: read the command's
/// file and carry it out, then the step's own words.
const COMMANDS_AS_PROMPTS: &str = "UPDATE workflow_nodes SET stage = 'prompt', \
    note = 'Read /work/repo/.claude/commands/' || run || '.md and carry it out for the request. ' || note, \
    run = '' WHERE stage = 'command'";

/// The kept workflows' steps of before D39 as steps.
fn nodes_as_steps() -> Vec<String> {
    let reading = "'plan','scope','discover','define','refactor','code_review','prove','comprehend','uat','learn'";
    let factory = format!(
        "{FACTORY_STAGES},'approve','review','clarify','your_checks','pick_refactors','pick_findings'"
    );
    let gone =
        format!("SELECT workflow, version, id FROM workflow_nodes WHERE stage IN ({factory})");
    vec![
        format!(
            "DELETE FROM workflow_links WHERE (workflow, version, source) IN ({gone}) OR (workflow, version, target) IN ({gone})"
        ),
        format!("DELETE FROM workflow_results WHERE (workflow, version, node) IN ({gone})"),
        format!("DELETE FROM workflow_nodes WHERE stage IN ({factory})"),
        format!(
            "UPDATE workflow_nodes SET stage = 'prompt', reads_only = 1, note = 'Do not change any file. ' || note WHERE stage IN ({reading})"
        ),
        format!("UPDATE workflow_nodes SET stage = 'prompt' WHERE stage IN ({AGENT_STAGES})"),
    ]
}

/// Rebuild a table under its new schema, keeping its rows. The rows go back
/// under the same name, so with deferred foreign keys the rows that reference
/// it still resolve at commit. Renaming a new table into place does not:
/// both were tried on a copy of `data/bastion.db` with foreign keys on.
pub(super) fn rebuild(
    (table, columns, create): &(&str, &str, String),
    command: &str,
) -> Vec<String> {
    let copy = format!("{table}_copy");
    std::iter::once(format!("CREATE TEMP TABLE {copy} AS SELECT * FROM {table}"))
        .chain(as_steps(table, &copy, command))
        .chain([
            format!("DROP TABLE {table}"),
            create.clone(),
            format!("INSERT INTO {table} ({columns}) SELECT {columns} FROM {copy}"),
            format!("DROP TABLE {copy}"),
        ])
        .collect()
}

/// The stages an agent ran, before a workflow was its steps (D39).
const AGENT_STAGES: &str = "'plan','approve','build','review','scope','discover','clarify','define',\
    'your_checks','specify','implement','refactor','pick_refactors','code_review','pick_findings',\
    'ci_fix','prove','comprehend','uat','learn','falsify'";
/// The stages the factory ran itself, before D39.
const FACTORY_STAGES: &str = "'land','prepare','baseline','checks','verify','acceptance','ship',\
    'ci','merge','deploy'";

/// A copy's rows with the stages of before D39 as the step kinds that
/// stand for them: an agent's stage as a prompt, the factory's as a shell
/// step. A grant at an old stage has no kind to move to, so it goes.
fn as_steps(table: &str, copy: &str, command: &str) -> Vec<String> {
    if table == "capabilities" {
        let old = format!("{AGENT_STAGES},{FACTORY_STAGES}");
        let mut gone = vec![format!("DELETE FROM {copy} WHERE stage IN ({old})")];
        // A command's grant goes with it, as a prompt has its own.
        gone.push(match command {
            "shell" => format!("UPDATE {copy} SET stage = 'shell' WHERE stage = 'command'"),
            _ => format!("DELETE FROM {copy} WHERE stage = 'command'"),
        });
        return gone;
    }
    let mut mapped = vec![format!(
        "UPDATE {copy} SET stage = CASE WHEN stage IN ({AGENT_STAGES}) THEN 'prompt' \
         WHEN stage IN ({FACTORY_STAGES}) THEN 'shell' WHEN stage = 'command' THEN '{command}' \
         ELSE stage END"
    )];
    if table == "jobs" {
        mapped.push(format!("UPDATE {copy} SET back_to = NULL"));
    }
    mapped
}
