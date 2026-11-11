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
| D13 | The local server starts the stack (`iii compose`) and attaches to one that already runs. The stack goes down when the server stops. A client attaches to the local server or starts it, and quitting a client leaves both running. |
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
  A turn whose provider has no key fails at its start, naming the provider,
  instead of waiting out its cap with no reply.
- **R6, reads stay small.** The admission pump reads only active jobs. Page
  `GET /jobs` and `GET /audit`. A row that fails to parse is logged and skipped, not fatal to
  the list.
- **R7, security edges.** Answer 404 for an unknown job and 502 or 503 for an
  engine failure, not 401 or 400. Compare tokens in constant time; create the
  token file with mode 0600. A client accepts `http://` only for loopback.
  The audit key falls back to a 0600 file where there is no Keychain.
  Each worker VM gets only the environment it needs: `iii compose` today
  passes the whole host environment to every worker, third-party ones
  included.

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
  the server and the client, with signed updates, installed through a
  Homebrew tap or an install script run as `curl … | sh`. Either way installs
  the server alone (with its CLI) or the server and the desktop app. The
  server installs headless on any Linux VM or VPS, x86_64 or arm64, and runs
  as a systemd service. Its job VMs need `/dev/kvm`, so a host without KVM
  (no nested virtualization) is refused by the doctor with that reason.
  Once a release installs, the gh-pages site is brought up to date with it:
  what Bastion is now and how to install it.
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
| R | done | R1 (2026-11-09): the audit writer reads only the chain's tail, keeps each record in `audit-spool.jsonl` until it is written, tries a failed write again with backoff up to 30 s, and keeps the newest entry's sequence and hash in `audit-anchor`, so deleting entries from the end breaks the chain. Live: a stack stop's record waited in the spool while the stack was down and was written, with the start's, once it came back (entries 1347 and 1348), and the chain verified. The queue stays unbounded, as the spool keeps it on disk; a stop's record no longer needs writing before the stop. R2 (2026-11-09): a turn's session is `<job>-<step>-<random>`, and its result applies only when the record still owns that session at that step, checked inside the save (`apply_when`); a turn that ends twice counts once. Resume runs at start and each time the engine link comes back up, re-arms what is left of a turn's cap, starts a running factory step's work again (a land with its pull request up watches CI again), and a queued part whose earlier part failed is abandoned. Live: the server was restarted mid-turn and the job still reached needs_you. R3 (2026-11-09): the factory reads the charter and the brief's charter files with `git show <fork point>:<file>`, so a file the agent changes in its worktree binds nothing; the Govern page still reads the workspace's own checkout. The core gate refuses a `sandbox::fs::write` to `CLAUDE.md` or `AGENTS.md` at any depth, or under `.claude/`, before any constraint. A shell command can still write those files; reading from the base commit is what keeps that from binding. R4 (2026-11-09): allow and refuse claim a held call with `DELETE … RETURNING id`, so two settles at once run it once. The baseline stores the approved test files in the job's `approved` column, and each turn start and resume loads them into the gate. Teardown and the turn cap call `harness::stop`. Stack start and stop share one lock, and so do writes to the router entry (per server process). `apply_when` returns the job as it was before the event moved it, and `decide` credits from that. Not checked live: the live store tests wait forever for an engine link, and the engine on 49160 did not give one. R5 (2026-11-09): every host command stops after 300 s, with no stdin and `GIT_TERMINAL_PROMPT=0`. The desktop client's calls get 5 s to connect and 120 s in all; its event stream reads as down after 45 s of silence. The openai-compat worker ends a request silent for 120 s and reads its kept hosts again at start until state answers. Each TLS handshake runs in its own task. R6 (2026-11-09): the admission pump and resume read only unfinished jobs and the jobs a queued part waits on. `GET /jobs` and `GET /audit` take `limit` (200 by default, 1000 at most) and `offset`; `/audit` still reads the whole chain to verify it. A row that does not parse is logged and left out of the browse lists; the audit chain and the gate's rules stay strict. R7 (2026-11-09): a missing job answers 404; a failure while the engine link is down answers 503. The owner token is compared in constant time. The token file and, where there is no Keychain, the audit key file are made with mode 0600. The client refuses `http://` beyond loopback; a server added before this keeps its URL. The stack starts `iii compose` with only the basic environment and `III_*`, so worker VMs get no host secrets; a provider whose key came from the environment needs its key set in Settings. R exit (2026-11-10), live on DeepSeek: two Allows on one held call ran it once (one `act`, the second refused as settled); the server restarted mid-Build and the turn resumed to needs_you; the server stopped took its stack down, and on start the stack came back, a mid-plan job resumed to needs_you and a waiting job stayed put; the audit chain verified with no gap throughout. The live store tests found that the engine refuses a second worker named `bastion-app`; the name now carries the process id. |
| I | done | I1 (2026-11-10): `crates/bastion-client` holds the server list, the local start, the calls and the event stream; the desktop keeps only its iced subscription. I2: every route is a typed request in `bastion_protocol::api` with its answer type; the server's tests check each against the router and each body against its handler; the command table is gone. I3: tokens stay in the Keychain on macOS and in 0600 files elsewhere; a server without a readable token is listed and its calls say why; clients trust the OS trust store, so a private CA works. I4: `bastion` lists servers and jobs, submits, approves, sends back, stops and follows. Exit (2026-11-10), live: a TLS server on the LAN with a test-CA certificate; the CLI sent a job back and the desktop showed the new turn; the desktop stopped it and `bastion follow` printed the change. The run found two bugs, both fixed: clients and a hand-started server used different data folders (so different tokens), and a turn started on a VM that went while its job waited. |
| F | F1 to F11 done | F1 (2026-11-10): a turn whose provider has no key fails at its start, naming the provider. Retry puts a failed job back in the queue at its step, on its kept worktree, which also revives a job whose VM kept dying; at a stuck step it runs the step again. Extend gives a running turn its full cap again from now, counted from `updated_at`, and a cap timer that fires after an extend waits out the rest. Approving a stuck step reads Ship anyway, and the audit notes the waiver with the first line of what failed. Retry, extend and ship anyway are audit decisions with the person's name, buttons on the job page and `bastion retry` and `bastion extend`. An engine call that failed before the engine got it (`iii is not connected`, a websocket error) is tried again after 0.5, 1, 2 and 4 s; a timed-out call is not, as it may have run, and a sandbox command's own failure is not either. After 10 minutes a running turn shows "still working". Exit (2026-11-10), live on DeepSeek: a command step that exits 3 got stuck, retry ran it again (two results), and approve noted "shipped anyway by Ian Johnson, waiving: The command failed (exit 3):"; an extend mid-turn moved the cap's start and was noted; a plan turn on keyless OpenAI failed the job, and with DeepSeek back a retry queued it, gave it a VM, and its plan reached needs_you. The first extend arrived a second after its turn ended and correctly changed nothing. The 10-minute mark has a test, not a live run: the turns took about 10 s. F2 (2026-11-10): a workspace can be edited (its name and path stay), and names a job limit, a revision budget (the steps' send-backs in all, past which the job waits on a person), a branch prefix, setup and cleanup commands, and an environment of `K=V` pairs. Setup runs in a VM of its own, the only one with the network, on the fork point, so it never runs code the agent wrote; what it made reaches the job's offline VM as a tarball streamed through a channel, and stays out of the agent's commits by `.git/info/exclude`. Cleanup runs in the job's VM as it stops, and a failure is logged. The onboarding turn (`POST /workspaces/draft`) belongs to no job: an agent reads the repository in an offline VM, under the gate as a read-only step, and drafts checks, setup, cleanup and environment into the add form; nothing is saved until a person adds the workspace. Live: setup reached PyPI and the job's VM had its files, a clean status, the environment and no network; a 3 MiB upload round-tripped; an edit saved and the store refused a limit of 0. Exit (2026-11-10), live on DeepSeek: an onboarding draft on `demo-local` took 24 s and named pytest, flake8 and a pip setup (its commands then named `/work/repo`, so the brief now says they run from the repository's root); an edit saved every setting and refused a bad prefix and a bare `CI`; a job limit of 2 held a job queued behind three waiting jobs, and raising it admitted the job (an edit now runs the admission pump); setup installed pytest into `.deps` with the network, and the agent's offline VM ran it, 7 passed; the job landed on `main`. The run found two land bugs, both fixed: a repository with no `origin` failed under the GitHub forge (it now lands here), and a VM revived after a restart had no new commits, so git refused the empty bundle (the worktree now lands as it is). F3 (2026-11-10): a workspace names quality gates, one `name: command` per line with `{files}` for the change's added and modified files, and an autofix; before a land or a ship each gate runs in the job's VM, a failure gets the autofix once (committed), and a gate that still fails stops the push with what it printed. A build's diff is checked by four `review::diff` rules: deleted tests, secrets (enforced), assertions lost net of those added, and added skip marks. A rule's words bind what a write writes as well as its arguments. Before a push the pull request's title and body pass a publishing gate: no line crediting an AI as author, and no key, token or private key, each named by kind and never by value; the body no longer says Bastion opened it. A prove turn's yes with no quoted output counts as partial. Exit (2026-11-10), live on DeepSeek: a `todo: ! grep -n TODO {files}` gate failed on the agent's `# TODO` line, the autofix `sed -i /TODO/d {files}` removed it and committed, the gate passed, and the job landed both commits. Not run live: the publishing gate (it needs a GitHub workspace), the proof downgrade (the full pipeline), and the new diff and content rules; each has tests. F4 (2026-11-10): a repository that commits a `verify.yaml` at the job's fork point has each change judged by `gantry-verify` (`@jeger-ai/opengantry-verify` 4.0.2) before a land or a ship, and a refusal stops the push with what it printed. The packaged `opengantry` worker could not do this: it runs sandboxed and cannot see a host repository, and run on the host its `gantry::verify` would run the agent's code outside the VM. So the verify runs in a VM of its own from the `node` image with no network: the job's repository goes in as a tarball, and the package as one installed on the host once with no install scripts. The contract is read from the base, so a change cannot weaken it. The packaged worker and the governed listener on 49161 stay in the stack file, unused. Exit (2026-11-10), live on DeepSeek: on `demo-local` with a contract that runs pytest and forbids `print(`, a change adding `shout(x)` with `print(x)` passed its tests and was refused at `calc.py:30`; the branch stayed as it was. The run found that a turn the harness lost when the stack restarted waited forever; resume now starts such a turn again, which the same run then did. F5 (2026-11-10): one Costs page holds both the costs and the sessions: `GET /costs` tallies every metered turn's cost, turns, calls and failed calls in all, by workspace, by step and by UTC day, and lists the newest 200 turns. A turn opens step by step (`GET /sessions/{id}`): what the agent was asked, said and thought, each call as the gate read it with the decision the gate wrote, and each result, clipped. The Audit Log tallies its entries by kind, the gate's by decision. Exit (2026-11-10), live: `/costs` gave $0.3745 over 84 sessions across three workspaces and four days; the last build of `jfbc0b38c` read as 39 steps with a verdict on each of its 13 calls. F6 (2026-11-10): a learn turn replies with JSON proposals, each a constraint or a workspace gate with why and the evidence quoted from the job; a reply with no evidence is sent back, and the JSON is read wherever it sits after the model's reasoning. The Learning page lists each job's newest readable reply, open proposals first; accepting a constraint adds it as told, accepting a gate adds it to its workspace, and each decision is an audit entry. A land keeps its commits as an artifact; a person reports an escape by its commit, the server traces it to the landed job whose commits hold it and keeps it in the audit, and the Verification page lists each landed job with the decisions that let it through, its commits, its escapes and each workspace's escape rate. An eval's trial that reaches review runs its contract (the workspace's gates without autofix, then `verify.yaml`) before its VM stops, and the eval shows whether it held beside the judge's grade. A local land now goes on to a step after it, such as learn. Exit (2026-11-10), live on DeepSeek: on a workflow of plan, build, land and learn, a job landed three commits and its learn turn found that the autofix had deleted the requested `# TODO: tidy` line and the test assertion that checked it, with pytest still green; it proposed two constraints and a gate, each with evidence. One was declined and the gate accepted into `demo-local` (then removed by hand). An escape on `a7c2732` traced to `j6fd07e99`; a commit no job landed and a 3-digit id were refused. An eval graded both arms 9 and 10 with both contracts held. The run found four bugs, all fixed: a local land ended the job before learn; the reply's JSON came after the reasoning; the contract ran beside the teardown and lost its VM; and the judge ran on the built-in OpenAI default, so no eval run had ever been graded. Accepted constraints cannot be removed yet, and a pull request merged by squash or rebase lands commits an escape cannot trace (F7 fixed this: a merge keeps its merge commit). F7 (2026-11-10): a pull request's body carries the plan, the briefing, the UAT script, the proof and each waiver, and is said again through the publishing gate before the merge, since the proof and the briefing come after it opens. When its checks pass, the preview a host deployed for its branch (the branch's newest GitHub deployment) is kept on the job. UAT, an old checkpoint, is a read-only turn after comprehend that writes a numbered script; it waits whatever its dial, a dark UAT dial is refused, and only the job's requester may approve it. Deploy finds the merge commit and polls its newest deployment: success passes with its address, failure waits on a person, and a commit nothing deploys within two minutes passes with a note. A revert on a workspace's branch of a commit a job landed, its merge commit included, is kept once as that job's escape, swept after each land and merge and before the Verification page reads. Exit (2026-11-10), live on DeepSeek against `ianparo/bastion-demo` with GitHub Actions: a job on plan, build, ship, CI, prove, comprehend, UAT, merge and deploy opened PR #1, CI passed, the UAT turn wrote a numbered script the requester signed off, the merge said the body again with Plan, Briefing, UAT and Proof and merged as `285b19b`, and deploy passed with "Nothing deployed 285b19b". A revert pushed to `main` was kept as the job's escape on the next read, and once only on the one after. The run found that GitHub's Revert button reverts the merge commit, which no land kept; the merge now keeps it. Not run live: a preview and a deployment that succeeds or fails (the demo repository deploys nothing); each has tests. The UAT reply opens with the model's reasoning before its script. F8 (2026-11-10): a request carries a kind (feature, bug, chore or docs), acceptance criteria, links and screenshots; the server writes them into the request's text under their headings and refuses an unknown kind, a link that is not a web address, or a screenshot name that is not a plain image file. Given criteria become the job's. Screenshots, up to 10 MiB in all, are kept on the host under the job asked for, shared by its parts, and put in each job's VM at /work/request. A request may name the model all its turns run on, as provider/model. Scope's parts are kept as its result and split only on approval, a dark scope's at once; while scope waits a person can set them again, and one part or none keeps one job. Intake can be paused in the settings: requests queue and nothing new starts. Every workflow can now be changed, the built-ins too: a changed one is kept under its id and takes its place, and deleting it resets it; the workflows page starts a new one from a name. Exit (2026-11-10), live on DeepSeek: with intake paused two requests stayed queued, and opening it admitted both; one named openai/gpt-5-mini and failed at its first turn with "openai has no key", and the other's request read Kind, Acceptance criteria, Links and Screenshots, and its plan found the 29-byte PNG in /work/request; a malformed model and an unknown kind were refused. A scope on a scope, plan, build and land workflow proposed two parts and waited; set again and approved, the job took the first edited part and a second job queued after it with the second. Short was saved as "Short (changed)", listed in its place, and reset to the original; a second reset was refused. The CLI does not take the new request fields yet, and whether a model can look at a screenshot is up to the model and its tools. F9 (2026-11-10): a job that comes to wait on a person, fails or lands is a notice: the factory's on_change carries a Change, so beside each job's change it says one line about the job and how many jobs wait. The server publishes notices on its event stream and posts each to a chat webhook as {"text": ...} (Slack, Mattermost), kept in a file only the server's user reads and shown to clients as its host and last four characters; Settings sets it, forgets it and sends a test. bastion-tray is a program of its own on tao and tray-icon, because a tray needs the native event loop iced keeps to itself: it reaches the local server through bastion-client, shows each notice as the system's notification, and its menu says how many jobs wait and run, lists the waiting jobs to approve, pauses or opens intake and opens the window; on macOS the waiting count shows beside a white castle. One runs at a time by a lock, the window starts it with its output in tray.log, and it outlives the window. The window shows the waiting count in its title and on Jobs. The app has an icon: the gh-pages castle with its gate lit (assets/icon). Exit (2026-11-10), live on DeepSeek: a job's plan came to wait, the system notification showed it, the menu bar read the castle and 2, matching the server's two waiting jobs, and the plan was approved and went on to build. The window started the tray once the tray's output was logged; the first start left nothing to say why it stopped. Not run live: the chat webhook (no webhook was set; a test posts a notice to a local listener). The count is in the title and the tray, not a Dock badge, and Open Bastion starts a second window if one is open. F10 (2026-11-10): an installed Bastion keeps its stack in share/bastion beside its binaries (following the link it was run by) and its data in the user's data folder. scripts/package.sh builds a platform's server-and-CLI tarball with its stack (the compose file running Bastion's own worker from its binary), and its desktop: Bastion.app, signed ad hoc until a Developer ID signs it, or a Linux tarball with a desktop entry. A v* tag runs the release workflow on macOS arm64 and Linux x86_64 and arm64, signs SHA256SUMS with the release minisign key and publishes the release. install.sh, served from the site, installs the server and CLI, or with --desktop the app or the window and the tray, checks each file's sum and the sums' signature when minisign is there, and with --service runs the server as a systemd user service on a Linux VM or VPS. bastion update installs the newest release once its signature, built in, and its sum check. scripts/brew.sh writes the tacoda/homebrew-bastion formula and cask from a release's sums. Exit (2026-11-10): v0.1.0 released from all three builds; its signature verified against scripts/minisign.pub; install.sh into a scratch prefix installed the server and the CLI, which ran, and bastion update read v0.1.0 as the newest; --desktop installed Bastion.app, whose ad hoc signature verified, with the CLI linked into it; the tap took the formula and, once fixed (a cask cannot conflict with a formula, and depends_on macos takes :ventura), the cask; the site went out with the install commands and served install.sh. The macOS 14 runner had been retired, so releases run on macos-latest. Not run: --service on a VM, an update across two releases, a Linux install, and the Homebrew installs on this machine. F11 (2026-11-10): a workspace names its VM image, python, node or browser; browser is Microsoft's Playwright image (v1.65.0, noble) registered with the sandbox worker, with Node, ffmpeg and Chromium, Firefox and WebKit under /ms-playwright but not the playwright npm package. Every VM now boots with loopback up: an offline VM had `lo` down, so nothing could serve on 127.0.0.1, and with no `ip` in the images the factory brings it up with Perl's ioctl, a failure logged and harmless. A browser VM gets PLAYWRIGHT_BROWSERS_PATH, which the image's own environment does not pass, and its turns are told how to take a screenshot offline and that a local server must start in the same command as the browser. After each turn the factory copies each image or recording under /work/artifacts, up to 50 MB each, to the host once, keeps it on the job, and serves it at `GET /jobs/{id}/media/{name}`; the job page lists the kept files to open. Exit (2026-11-10), live on DeepSeek: the browser image took about 9 minutes to pull on first boot; with loopback down a local request timed out, and up it answered. On a `demo-browser` workspace a short-pipeline job added a paragraph, served the site with node and screenshotted it with the headless shell; home.png (1280 by 800, showing the new paragraph) was kept on the job and served by the media route. With its setup installing playwright@1.65.0 (in the setup VM, with the network), a second job wrote a Playwright script that served the site in its own process, recorded Chromium clicking a new Back to top link, and kept visit.webm (25 KB) and after.png; both played back right from the media route, and the setup's node_modules stayed out of the diff. Not looked at live: the job page's Kept files section. |
