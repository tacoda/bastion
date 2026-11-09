# Bastion plan

Bastion is a software factory you can govern. A request goes in, agents work
it through a workflow of steps a person can watch and gate, and a reviewed
change comes out. Bastion is a server and its clients: `bastion-server`
runs the factory on the iii engine, and clients such as the iced desktop app
reach it over HTTP. More interfaces and more servers come later, so every
client goes through one client library and one API. This plan replaces the
plan that built Bastion phase by phase (2026-10-30 to 2026-11-09); git
history keeps it. Written 2026-11-09.

## 1. Where Bastion stands

**Built and seen working live:**

- The factory runs workflows of steps. A step is a Bastion stage, a skill, a
  prompt or a command. Each step has a dial: lights-on waits for a person,
  dark goes on at once. The full pipeline (scope to learn, 20 steps) merged a
  real pull request, and scope splits a request into parts that land in
  order.
- Governance: constraints on a ladder (told, checked, enforced), capabilities
  per stage (denied, ask, allowed), the repository's own charter, a gate on
  every sandboxed call, held calls a person allows or refuses, and an audit
  hash chain.
- A/B evals that compare two harness configurations on the same requests.
- People and roles on a server, with issued tokens. Model provider keys in
  the stack's vault, with OpenAI-compatible hosts as providers of their own.
- `bastion-server`: an HTTP and server-sent events API in front of the
  factory, the store, the gate and the stack.
- `bastion-desktop`: an iced client with every page (Jobs, New request, the
  job, Workflows, Constraints, Capabilities, the Audit Log, Evals, Engine,
  Settings), several servers at once, and live pushes.

**Not yet sound:** a server review on 2026-11-09 found that restarts and
outages can lose audit records, stall jobs, or apply a stale turn's result,
and that an agent can rewrite the charter that binds it (§5, phase R).

**Not yet built:** most of what the wipp factory and the ghola kit do beyond
this core: recovery controls, workspace setup, quality gates, the learning
loop, cost and session views, preview and deploy, notifications and
packaging (§5, phase F).

## 2. Goals and non-goals

**Goals:**

1. A person sees what every job does and decides what it may do.
2. A change Bastion lands passed the checks the owner asked for, with the
   evidence on the job.
3. A restart, an outage or a lost VM never loses a record or corrupts a job.
4. Any client and any number of servers, through one API.

**Non-goals for now:** a web client, managed hosting, a plugin system for
code-defined guards (rules stay data, per D20 and D26), and a sign-in with a
Claude subscription (D36). Multi-user servers and hosting stay open
questions (§7), not non-goals.

## 3. Decisions

Each decision keeps the number the code cites. Numbers not listed were
retired with the old plan.

| # | Decision |
|---|---|
| D1 | Rust for all Bastion code. Another language only where no Rust path exists, named in §4 with its reason. |
| D4 | One dark theme, purple as the main color. Its tokens live in `crates/bastion-desktop/src/theme.rs`. |
| D6 | Every Bastion worker is a Rust binary on `iii-sdk` 0.25.0, started by compose through `scripts.run`. |
| D7 | A stock iii worker comes first. Bastion writes a worker only for what no pinned worker does. |
| D8 | Records live in the `database` worker as typed columns. Each row carries a `rev`, and a stale write is refused. |
| D10 | Updates are pushed, never polled: the server observes the engine, and clients follow the server's events (D29). |
| D13 | The local server starts the stack (`iii compose`) and attaches to one that already runs. Quitting a client never stops it unasked. |
| D14 | Every agent turn runs in an `iii-sandbox` VM on its job's worktree. No credential enters the VM; pushes and pull requests happen on the host. |
| D16 | A note on a step goes to the agent word for word as its next brief. A fix loop stops after a set count, or sooner when a refusal repeats word for word. |
| D17 | Every job runs a workflow. A free-form task is a short workflow, not a second mode. |
| D18 | The factory's rules live in `bastion-core`: pure functions from an event and a state to a new state and effects, with no I/O and no `async`. |
| D19 | The seams are Cargo dependencies, and `scripts/check-seams.sh` fails CI when one is crossed. Only the engine adapter imports `iii-sdk`. Clients depend on `bastion-protocol` and the client library only. |
| D20 | Every constraint and capability is enforced in a hook, and its rung is data, so an eval can name what changed. |
| D21 | The audit record is a hash chain with one writer. Each entry carries an HMAC-SHA256 of its fields and the entry before it, keyed with a secret outside the record. |
| D22 | An eval compares two harness configurations on the same requests from the same commit, and reports the difference with the decisions behind it. |
| D23 | Forge, tracker, CI, model providers and notifications are adapters behind seams, chosen in Settings. |
| D25 | Every rule belongs to one layer: the charter (the repository's own), the harness (an agent's calls), or the factory (the workflow). |
| D26 | Rules, grants and settings are data in the server's database, versioned, edited on the pages; first-run defaults come from `seed/governance.sql`. |
| D27 | The server is the source of truth and holds all business logic. A client shows state and sends a person's actions, nothing more. |
| D28 | The API is HTTP with JSON, one route per action on one resource, typed in `bastion-protocol`, with a bearer token on every request. |
| D29 | Push is server-sent events on `GET /events`, with ids. A client that reconnects with `Last-Event-ID` gets what it missed, or a `resync`. |
| D30 | A client keeps a list of servers, each a name, a URL and a token in the OS keychain. `local` is the server on this machine. A job runs on one server. |
| D31 | A server has people with roles: an admin manages it, a member submits and acts on their own jobs, a viewer reads. Tokens are issued, shown once and kept as a hash. Every record names its actor. |
| D32 | A server listens on loopback unless it has a TLS certificate and key. |
| D33 | Each step is lights-on or dark. A workflow's level follows from its steps. No step waits on a timer. Every decision is logged, whatever the dial. |
| D34 | Model provider keys live in the stack's vault, read only by `llm-router`, never in the environment and never returned. |
| D35 | A workflow is a graph of steps a person edits on a canvas. Steps pass declared results to the steps linked after them. |
| D36 | Claude takes an API key only. No Claude subscription sign-in. |
| D37 | The desktop client is a native Rust app on `iced`. It holds no factory logic and reaches a server only through the client library. |
| D38 | Every client goes through `crates/bastion-client`: the server list, typed calls, and the event stream. A new interface adds pages, not a second way to talk to a server. |

## 4. The repository

```
crates/
  bastion-core/        the factory's rules (D18)
  bastion-protocol/    the API's types and routes (D28)
  bastion-server/      the server: HTTP, events, factory adapter, store,
                       gate, audit, stack; src/engine/ is the iii seam
  bastion-client/      the client library (D38), from phase I
  bastion-desktop/     the iced client (D37)
workers/
  bastion-openai-compat/   OpenAI-compatible hosts as router providers
seed/                  first-run rules, grants and workflows (D26)
worker-compose.yaml    the stack the local server starts
docs/engine-findings.md   what the engine was found to do, and on which versions
```

Code in another language (D1): `.pi/extensions/entire/index.ts`, generated
by the Entire tool, not Bastion code.

## 5. Phases

Each phase ends with its exit check, run live. Every commit passes the
CodeScene gate and every CI step. Phases run in this order: C, R, I, then
F in the order listed.

### C — Cleanup and cutover

**Goal:** the repository holds only the server-client design, and every
document says what is true.

- **C1, retire the old client.** Delete `src/`, `src-tauri/`, `index.html`,
  `Trunk.toml`, `styles.css`, `.taurignore`, `dist/`. Keep the app icon in
  `crates/bastion-desktop/`. Make the root `Cargo.toml` a virtual manifest.
  Drop the wasm target from `rust-toolchain.toml`, the Tauri and wasm steps
  from CI, the old rules from `scripts/check-seams.sh`, and the Tauri entries
  from `.gitignore` and `.vscode/`. The page parity check (old G3) is the
  owner's review of every page in G2, done 2026-11-09.
- **C2, remove what only the old client used.** In `bastion-protocol`: the
  window commands (`QUIT`, `SERVERS`, `SERVER_ADD`, `SERVER_REMOVE`) and
  `mod event`. In `bastion-server`: CORS and its test, and the unused
  `fakes` feature. In `bastion-desktop`: `profile.rs`, replaced by the
  checkout and the engine URL.
- **C3, remove dead code and stale notes.** The fixed grading model in
  `factory.rs` (use the settings model), the dead `bastion-steps` queue and
  spike N2, `PersonView` and `TokenView` duplicates (the server returns the
  protocol types), the N14 spike crate and spike N12's server half (both
  about the old client),
  references to old plan phases in comments, "profile", "app", "page" and
  "window" wording in server code, `cargo test -p bastion` (now
  `bastion-server`), and `make up` (no Makefile).
- **C4, the stack file.** List which pinned workers the server or the harness
  calls. Remove those nobody calls, or record why each stays.
- **C5, documents.** Rewrite `README.md` (what it is, how to run the server
  and the client, the checks) and `CLAUDE.md` (the layout, the seams, every
  CI step before a push, the CodeScene gate). Mark the version line in
  the spike record historical, and add spike N10. Then remove the spikes
  crate, whose findings are in the code, and keep their record as
  `docs/engine-findings.md`.
- **C6, local leftovers.** Delete the untracked `floyd` data and config, the
  observability dump in `data/`, `agents/`, and `skills/provider-claude-code`.

**Exit:** CI passes with no Tauri, Leptos or wasm step; `rg -i
"tauri|leptos|trunk|profile"` finds nothing but history in `docs/engine-findings.md`;
the desktop client and the server run a short workflow to Approve.

### R — Robustness

**Goal:** a restart, an outage, or two people acting at once never loses a
record, stalls a job, or runs a call twice.

- **R1, the audit writer keeps every record.** Keep the chain's tail in
  memory and read only the last entry at start, instead of the whole chain
  on each append. Retry a failed write with backoff instead of dropping it,
  spool queued records to disk so they survive a restart. Keep the tail's
  hash outside the database, so deleting the newest entries breaks
  verification.
- **R2, resume is exact.** A turn's session names its job and its step, and
  its result applies only while the job still runs that session at that
  step, checked in the same save; the core ignores a turn's result at a step
  that runs no turn. Resume runs again whenever the engine link
  comes back, re-arms each turn's time cap from `updated_at`, and starts again
  any factory step that was running. A part whose earlier part failed stops
  instead of waiting forever.
- **R3, the charter binds the agent.** Read the charter from the workspace's
  base commit, not from the job's worktree the agent writes to. An enforced
  rule refuses writes to `.claude/` and `CLAUDE.md`.
- **R4, one action, one effect.** Claim a held call atomically before running
  it. Keep approved-test protections in the store. Stop the harness turn when
  a job stops or hits its cap. Serialize stack start and stop. Lock the
  router entry while setting a key. `decide` credits a person only for a move
  their event made.
- **R5, nothing waits forever.** Timeouts on host commands (`git push`,
  `git pull`), on every client call and on the event stream (a half-open
  connection reads as down), and on the openai-compat worker's requests. The
  worker retries its state reads at start. TLS handshakes run concurrently.
- **R6, reads stay small.** The admission pump reads only active jobs. Page
  `GET /jobs` and `GET /audit`. A row that fails to parse is logged and skipped, not fatal to
  the list.
- **R7, security edges.** Answer 404 for an unknown job and 502 or 503 for an
  engine failure, not 401 or 400. Compare tokens in constant time; create the
  token file with mode 0600. A client accepts `http://` only for loopback.
  The audit key falls back to a 0600 file where there is no Keychain.

**Exit:** with a job at each kind of step, stopping and starting the stack
and restarting the server loses no audit record and leaves no job stuck;
two clicks on Allow run a held call once; a Build turn that rewrites
`.claude/settings.json` does not change the next turn's rules. Each has a
test.

### I — Interfaces and servers

**Goal:** a second interface or a remote server costs pages, not plumbing.

- **I1, the client library.** Move the server list, the local server start,
  the HTTP client and the event stream out of `bastion-desktop` into
  `crates/bastion-client` (D38).
- **I2, typed calls.** Replace the command strings and the runtime route
  table with one typed function per route, checked against the server's
  router, so a mismatch fails to compile or fails a test.
- **I3, servers anywhere.** The keychain works on Linux too; an added server
  whose token is missing shows as such instead of failing the list; a remote
  server runs on Linux with its audit key in a file (R7).
- **I4, a command-line interface.** `bastion` lists jobs, submits a request,
  approves, sends back, and follows events, through the client library.

**Exit:** the CLI and the desktop client act on one job on a remote server
over TLS, and each sees the other's action pushed.

### F — Features

Each feature names what it takes from wipp (W) or ghola (G). The order is by
value; each is a phase of its own when it starts.

- **F1, recovery.** Retry a failed or stuck step, ship anyway, extend a turn,
  revive a lost VM, all as a person's decisions; retry transient sandbox and
  engine errors before failing; a "still working" mark on long turns (W).
- **F2, workspace setup.** Prepare and cleanup commands, environment, network
  for setup only, a branch prefix, a revision budget, a concurrency limit per
  workspace, and an onboarding turn that drafts a new workspace's settings
  (W, G).
- **F3, quality gates.** Lint, static analysis, coverage, mutation, secrets
  and weakening gates over changed files before a push, with an autofix
  command run once; rules that check file content, not only a call's words;
  a publishing gate over the pull request text (no AI attribution, no
  secrets); proof downgraded when it cites no evidence (W, G).
- **F4, a machine verdict on promotion.** A push, merge, land or deploy needs
  a verdict from the `opengantry` worker as well as the dial (G).
- **F5, what it cost and what it did.** A Costs page (the data is already in
  `turns.cost_micros`), a Sessions page of every turn, a turn shown step by
  step with the gate's verdict on each call, and audit tallies (W, G).
- **F6, learning.** Learn proposes rule changes with the evidence behind
  them; a person accepts one as a constraint or a spec; escapes traced to a
  landed change; a verification page; graders that check a contract, not
  only a judge's score (W, G).
- **F7, ship, preview and deploy.** A pull request body with the briefing,
  the proof and what was waived; a preview for reviewers; a UAT script
  signed off by the requester; a CD watch on the merge commit; a revert
  watch (W).
- **F8, requests.** A request's kind, criteria, links and screenshots;
  approving or editing scope's split; a model per job; pausing intake (W).
- **F9, notifications.** A notification on `needs_you`, `failed` and
  `landed`, with a tray count, and a chat adapter (D23).
- **F10, packaging and updates.** A signed macOS build and a Linux build of
  the server and the client, with signed updates.
- **F11, the browser agent.** Browser tools for turns, with screenshots and
  recordings as artifacts.

## 6. Verification

- Every phase ends with its exit check run live, recorded in its commit.
- Every commit passes the CodeScene gate; every push passes fmt, the seam
  check, clippy with `-D warnings`, and the tests.
- A fix for a bug comes with a test that fails without it.

## 7. Open questions

- **Dark to merge.** ghola never lets a factory merge unattended; Bastion's
  dark dial can run to merge. Should merge always need a person, or a machine
  verdict (F4)?
- **Configuration in git.** ghola keeps configuration in files under review;
  Bastion keeps it in the database with versions (D26). Should rules also be
  importable as files, such as team rule packs shared by servers?
- **Multi-user hosting.** A shared server with people and TLS works today.
  Is hosted Bastion a goal?
- **Unused stack workers.** C4 lists them; some may be the harness's
  dependencies.

## 8. Progress

| Phase | State | Evidence |
|---|---|---|
| C | done | C1 retired the old client; C2 removed the protocol and server parts only it used, and the client's profiles; C3 removed the fixed grading model, the dead queue, spikes N2, N12's server half and N14, the people and token type duplicates, every citation of an old plan phase, and the old client's words in server code. C4 removed the stack's `cron`, `storage`, `ide`, `workflow`, `memory` and `eval`, which nothing calls; `harness` calls `session-manager`, `context-manager`, `iii-directory`, `fp`, `judge` and `approval-gate` (from its binary), so they stay; `opengantry` stays for F4. C5 rewrote the README and CLAUDE.md, and the spikes crate went: its findings are in the code, and their record is `docs/engine-findings.md`. Exit, live (2026-11-09): the server and the stack restarted on the trimmed stack file, a short workflow's plan turn reached Approve, and the desktop client listed it; CI's steps pass with no Tauri, Leptos or wasm step. C6, with the owner's yes, deleted the untracked `floyd` data and config, 885 MB of engine telemetry, a database backup, `agents/` and `skills/provider-claude-code`. |
| R | in progress | R1 (2026-11-09): the audit writer reads only the chain's tail, keeps each record in `audit-spool.jsonl` until it is written, tries a failed write again with backoff up to 30 s, and keeps the newest entry's sequence and hash in `audit-anchor`, so deleting entries from the end breaks the chain. Live: a stack stop's record waited in the spool while the stack was down and was written, with the start's, once it came back (entries 1347 and 1348), and the chain verified. The queue stays unbounded, as the spool keeps it on disk; a stop's record no longer needs writing before the stop. R2 (2026-11-09): a turn's session is `<job>-<step>-<random>`, and its result applies only when the record still owns that session at that step, checked inside the save (`apply_when`); a turn that ends twice counts once. Resume runs at start and each time the engine link comes back up, re-arms what is left of a turn's cap, starts a running factory step's work again (a land with its pull request up watches CI again), and a queued part whose earlier part failed is abandoned. Live: the server was restarted mid-turn and the job still reached needs_you. |
| I | not started | |
| F | not started | |
