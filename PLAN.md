# Bastion plan

Bastion is a native software factory on the iii engine. A request goes in, a
reviewed change comes out, and nothing merges itself. It is one app: a Tauri 2
window with a Leptos frontend and a Rust core. Agents run on iii workers,
against a local engine that the app starts. A remote engine on the owner's own
box comes later (§10). Written 2026-10-30, revised the same day.

The owner works at the level of the factory. The first view is the **floor**:
every job, the stage it is in, and the decisions waiting on the owner. Agents
and their transcripts sit one level down, for when a job needs a closer look.
Antigravity starts at the agent; Bastion starts at the job.

Bastion reaches feature parity with wipp (`~/Code/wipp`, as of commit
`f7c7e02`): every feature wipp has, except the ones tied to hosting and to
more than one user (§7.1). Parity is at the level of features, not code: wipp
runs Claude Code in sprites.dev VMs, and Bastion runs `harness` turns in
`iii-sandbox` VMs.

Bastion is not an editor. The owner keeps their own editor. The one view of
code inside Bastion is the diff a job proposes.

## 1. Goals

1. One native app on macOS and Linux runs Bastion against a local `iii` engine.
   No terminal is needed after install.
2. The floor shows every job, its stage and its state, and the inbox shows
   every decision waiting on the owner. Both update by push.
3. A job moves through a **pipeline** of stages. Agents do the work in each
   stage, several jobs at once, each in its own worktree and sandbox VM.
4. Every stage leaves an **artifact** a person can judge: a plan, a test
   result, a diff, a screenshot, a recording or a briefing. A comment on an
   artifact goes back to the agent as its next instruction.
5. An agent can drive a browser, and its screenshots and recordings are
   artifacts.
6. Nothing merges itself. A person decides every merge.
7. A job that needs the owner raises a native notification within 2 s.
8. No Bastion process serves HTTP. The engine listens on loopback only.
9. Feature parity with wipp, with the exceptions §7.1 names.

## 2. Non-goals

- Not an editor or a file browser. The diff artifact is the only view of code.
  "Open in editor" hands a worktree path to the owner's own editor.
- Not an agent chat. The owner talks to a job through its artifacts and
  checkpoints, not through a prompt box per agent.
- Not a browser app, and no HTTP API.
- Not multi-user. One owner, on their own machine. No sign-in. This drops
  wipp's accounts, GitHub sign-in, org check and roles (§7.1).
- Not a new agent runtime. Turns run on the stock `harness`, `llm-router` and
  `iii-sandbox` workers.
- Not hosted. wipp runs on Fly for a GitHub org, behind Caddy. Bastion runs
  on the owner's machine, so the Fly deploy is not ported (§7.1).
- Not a remote client yet. The owner's box is §10.
- Not Windows. Sandbox VMs need Apple Silicon or Linux with `/dev/kvm`.

## 3. Fixed decisions

| # | Decision | Why |
|---|---|---|
| D1 | Rust for all Bastion code. TypeScript or JavaScript only where no Rust path exists, and each such file is named in §4 with its reason. | One language. |
| D2 | Tauri 2, the stable line. `cargo search` shows 3.0.0-alpha.5 on 2026-10-30, so 3 waits. | A native app with a Rust core. |
| D3 | The frontend is Leptos 0.8, client-side rendered to WASM and built by Trunk. | Rust pages in Tauri's webview. |
| D4 | One dark theme, purple as the main color (`--primary: #7c3aed`, `--primary-hover: #6d28d9`, `--link: #c4b5fd`). No light theme. Tailwind 4 runs as Trunk's standalone binary. | The owner's choice. No Node toolchain. |
| D5 | One Cargo workspace: the frontend crate at the root, the app in `src-tauri/`, shared code in `crates/`, Bastion workers in `workers/` (§4). | One build, one lockfile, one CI. |
| D6 | Every Bastion worker is a Rust binary on `iii-sdk` 0.25.0. Compose starts it through `scripts.run`. | The same engine, with no second runtime. |
| D7 | A stock registry worker comes first. Bastion writes a worker only for what no pinned worker does, and §5 records each choice. | Less code to own. The stack already pins `harness`, `iii-sandbox`, `worktree`, `approval-gate`, `workflow`, `session-manager`, `github` and `storage`. |
| D8 | Bastion's records (jobs, stages, artifacts, comments, decisions) live in the `database` worker as typed columns. Each row carries a `rev`, and a stale write is refused. | wipp lost a live job in the `state` worker on restart, and a last write once overrode a Stop. |
| D9 | The pages reach the engine only through typed Tauri commands. A command maps to one engine function or one core routine, and no command takes a function id. | One narrow surface, listed in one file. |
| D10 | Live updates are push. The engine adapter observes engine events (a turn completed, a CI run changed) by push. After each change it persists, the app emits a Tauri event `{kind, id, revision}`, and the page reads the resource again. | wipp polled. Push needs no timers and no stale views. |
| D11 | One owner, no sign-in. The actor on every record is `git config user.name`. | One person on one machine needs no accounts. |
| D12 | Local is the default and only mode for now. The engine address is a profile field, never a constant. | The remote box (§10) becomes a new profile kind, not a rewrite. |
| D13 | The app starts the stack with `iii compose --up -f worker-compose.yaml`, and attaches to a stack that already runs. On quit, the owner chooses whether it keeps running. | The owner's choice. No terminal. |
| D14 | Every agent turn runs in an `iii-sandbox` VM on its job's worktree. A credential never enters the VM. Pushes and PRs happen on the host. | wipp's lesson: a sprite that saw a credential is tainted. |
| D15 | Merge is a person's decision, or `hold` at the lowest. Every other checkpoint has a dial: `ask`, `hold` (default after 30 min) or `auto`. | wipp's oversight model. |
| D16 | A comment on an artifact goes to the agent verbatim, as its next brief. A fix loop stops after a set count, or sooner when a refusal repeats word for word. | wipp's bounded loops. Paraphrased feedback loses the point. |
| D17 | Every job runs a pipeline. The smallest pipeline is plan, approve, build, review, land, and it is the default until A5 adds the rest. | The factory is the unit. A free-form task is a short pipeline, not a second mode. |
| D18 | All factory logic lives in the app, in `crates/bastion-core`, apart from the view and from iii (§6.1). The core takes an event and the current state, and returns the new state and a list of effects. It does no I/O, has no `async`, and does not depend on `iii-sdk`, Tauri or Leptos. | The owner's choice. The rules are tested as plain functions, with no engine and no window. |
| D19 | The seams in §6.2 are the only ways across layers, and Cargo dependencies enforce them. A CI check fails if `bastion-core` or `bastion-protocol` gains a dependency other than `serde`, or if `iii-sdk` is imported outside `src-tauri/src/engine/`. | The owner wants clean seams. A seam that only a reviewer guards erodes. |

## 4. The repository

```
Cargo.toml  index.html  Trunk.toml  styles.css
src/                  bastion-ui: the Leptos frontend (WASM)
src-tauri/            bastion: Tauri, the Rust core
  src/engine/         the engine adapter: every iii call, and effects run
  src/commands/       one file per resource (D9)
  src/stack.rs        iii compose as a child process
  src/doctor.rs
crates/
  bastion-protocol/   the typed commands the view calls (D9)
  bastion-core/       the factory logic (D18): jobs, pipelines, stages,
                      checkpoints, the dial, fix loops, briefs
  bastion-spikes/     spikes and live checks, as examples
workers/
  bastion-browser/    a browser driven over CDP
worker-compose.yaml   the stack the app starts
PLAN.md  docs/
```

`cargo tauri dev` runs the app.

**JavaScript that may remain (D1):** none is planned.

## 5. Where each feature lives

**Put it in a worker when** it keeps state, runs while the app is closed, or a
turn or another worker needs it. The app on a laptop then reaches it on the box
later (§10).

**Put it in the app's core when** it is about this machine or this window: it
must run before the engine is up, it only shapes what the page shows, or it
talks to the OS.

| Feature | Home | Worker | Phase |
|---|---|---|---|
| Stack start, stop, attach, logs | app | — | A1 |
| Doctor | app: it runs before the engine | — | A1 |
| Profiles | app | — | A1 |
| Engine console: workers, functions, triggers, queues | app view over the engine | engine built-ins | A2 |
| Factory logic: jobs, pipelines, stages, checkpoints, the dial, fix loops, briefs | app: `bastion-core` (D18) | — | A3 |
| Running effects and resuming jobs | app: the engine adapter | — | A3 |
| Records | worker | `database` (stock) | A3 |
| Worktrees and local land | worker | `worktree` (stock) | A3 |
| Agent turns | worker | `harness`, `llm-router`, `iii-sandbox` (stock) | A3 |
| Transcripts | worker | `session-manager` (stock) | A3 |
| Diffs | app: the adapter reads them from the job's worktree | `worktree` or `ide` (stock) | A4 |
| Approvals | worker | `approval-gate` (stock) | A4 |
| Push, PRs, CI watch, merge | worker | `github` (stock) | A4 |
| Browser | worker | `bastion-browser` | A9 |
| Notifications and tray | app | — | A10 |

The factory logic is the one exception to the worker rule above. The owner
chose to keep it in the app (D18), and the cost is that jobs advance only while
the app runs. A10 lets the app keep running in the tray with its window closed.
Because the core does no I/O, moving it into a worker later means writing a
second adapter, not changing the core.

`ade` is iii's own console, on `127.0.0.1:3114`. It stays in the stack until
A2's console covers what the owner uses in it, and then it leaves.

## 6. Concepts

From the top down:

- **Floor:** every job in flight, grouped by stage, with its state.
- **Inbox:** every checkpoint waiting on the owner, oldest first.
- **Workspace:** a repository the owner registers with Bastion.
- **Job:** one request against one workspace. It owns one worktree, one
  sandbox VM and one pipeline run. States: `queued`, `running`, `needs_you`,
  `blocked`, `landed`, `closed`, `failed`, `stopped`.
- **Pipeline:** the stages a job moves through. It is data, so a job can run a
  short one or the full one.
- **Stage:** one step of a pipeline. An agent stage runs one or more turns; a
  person stage is a checkpoint.
- **Checkpoint:** a point where a person decides, with a dial (D15).
- **Artifact:** a typed output of a stage: plan, test result, diff,
  screenshot, recording, briefing. Bytes go to `storage`; the row goes to
  `database`.
- **Comment:** the owner's note on an artifact, or on one line of a diff. It
  becomes the stage's next brief (D16).
- **Turn:** one agent run inside a stage, with its transcript. The lowest
  level the app shows.

### 6.1 Three layers

| Layer | Where | Knows | Does not know |
|---|---|---|---|
| **Core** | `crates/bastion-core` | The factory's rules: which stage comes next, when a checkpoint waits, what the dial does, when a fix loop stops, what a brief says. | iii, Tauri, Leptos, the clock, the disk. |
| **Engine adapter** | `src-tauri/src/engine/` | How to run each effect on iii, and how to turn each engine event into a core event. It persists every new state with its `rev` (D8). | Any factory rule. |
| **View** | `src/` | How to draw a resource, and which typed command to call (D9). | iii and the factory's rules. |

One loop joins them. An event arrives: the owner's decision through a command,
or an engine event such as a turn that completed. The adapter loads the job,
calls the core, writes the new state, runs the effects the core returned, and
emits the change to the page (D10). An effect is data, such as "start a turn
with this brief" or "open a PR". The core never waits on one.

On start, the adapter reads every active job and resumes it. Read commands ask
the core for derived facts, such as which actions a checkpoint allows, so the
view never computes a rule itself.

### 6.2 Seams

Each seam is named, has one owner, and has a fake that tests use. Crate
dependencies enforce the seams, so code that crosses one does not compile
(D19).

| Seam | Shape | Owner | Fake in tests |
|---|---|---|---|
| **Event in, decision out** | `bastion_core::step(state, event) -> (state, Vec<Effect>)`. `Event` and `Effect` are enums. The time comes in on the event. | `bastion-core` | None needed: the core is pure, so tests call `step` directly. |
| **Engine** | A trait in the adapter: `call`, `observe`, `register`. The only code that imports `iii-sdk`. | `src-tauri/src/engine/` | An in-memory engine that records calls and replays events. |
| **Store** | A trait in the adapter: load a job, save a job with its `rev`, list active jobs. Backed by `database` through the Engine seam. | `src-tauri/src/engine/` | An in-memory store that refuses a stale `rev`. |
| **Machine** | A trait in the app: run a program, read a file, test a path, name the platform. Doctor and the stack routine reach the OS only through it. | `src-tauri/src/machine.rs` | A machine with canned runs, files and platform. |
| **Commands** | The typed Tauri commands and their request and response types, in `crates/bastion-protocol`. The view and the app both depend on it. | `bastion-protocol` | The view's tests use canned responses. |

Allowed dependencies:

```
bastion-ui (view)  ──>  bastion-protocol
bastion (app)      ──>  bastion-protocol, bastion-core, iii-sdk, tauri
bastion-core       ──>  serde only
bastion-protocol   ──>  serde only
```

`bastion-core` and `bastion-protocol` never depend on each other's internals.
The app maps between them, so the core's types can change without changing
the page.

## 7. Phases

Each phase uses paired pacing and stops at its exit evidence, run live after
its last change. A phase that rests on an unproven engine feature starts with
a spike, and a failed spike stops the phase with its fallback named. Sizes: S,
M, L.

### 7.1 Parity with wipp

Each wipp feature, and the phase that brings it to Bastion. A phase that lists
a wipp feature is not done until the feature works as it does in wipp, unless
this table names the difference.

| wipp feature | Bastion | Phase |
|---|---|---|
| Jobs page | The floor | A3 |
| New request | New request | A3 |
| Job page: needs, panels, turn | The job view and the inbox | A3 |
| Job page: proof | The evidence of each check, per stage | A5 |
| Diff | The diff artifact | A4 |
| Every stage in `stages.py` (shape, acceptance tests, build, ship, side paths) | The full pipeline | A5 |
| Split jobs into dependent parts | Split | A5 |
| Oversight dial: `ask`, `hold`, `auto` | The dial (D15) | A4 |
| Oversight presets: high, medium, low, off | Presets in Settings | A7 |
| The gate, refusals, retry with feedback, allow once | The gate in `bastion-core`, bound as a `pre-trigger` hook | A6 |
| Constraints: board, detail, forms, three tiers | Constraints pages | A6 |
| Capabilities: grants per stage | Capabilities page | A6 |
| Repository hooks per stage | Repository hooks | A6 |
| Audit: hash chain, verification, export | Audit page | A6 |
| Repos, and onboarding a repository with no settings | Workspaces | A7 |
| Sessions | Sessions page | A7 |
| Sprites, and the warm pool | Sandboxes page, on `iii-sandbox` | A7 |
| Costs | Costs page | A7 |
| Settings, with history, versions and restore | Settings page | A7 |
| Model and provider per stage, and override per job | Models in Settings, override in the job view | A7 |
| Stop, escape, override, split, pause intake, destroy | Job and factory controls | A7 |
| The vault | The `secrets` worker | A7 |
| Learn, and proposals from what surprised a turn | Learn stage and Proposals page | A8 |
| Evals: grade and backtest | Evals page | A8 |
| Verification: escape rate against a threshold | Verification page | A8 |
| Accounts, GitHub sign-in, org check, roles | Not ported: one owner (D11) | — |
| Fly deploy, Caddy, the loopback boundary script | Not ported: a local app. §10 covers the owner's box. | — |
| Claude Code in a sprite, with a broker of MCP tools | Replaced by `harness` turns in `iii-sandbox` (D14) | A3 |

Bastion also has what wipp does not: the native window, push updates (wipp
polls), notifications, the browser agent, and comments on artifacts.

### 7.2 The phases

#### A0 — Spikes (M)

**Goal:** prove that Rust on `iii-sdk` 0.25.0 and Tauri can do what the rest of
the plan assumes.

- **N1:** a Rust worker registers a function, triggers one, observes a
  `changed` function, and calls `database::query` and `database::transaction`.
- **N2:** dropped by D18. The factory runs in the app and needs no queue.
- **N3:** it binds `harness::turn-completed` for one session, and a
  `harness::hook::pre-trigger` that rewrites `sandbox_id`.
- **N4:** it reads a `sandbox::fs::read` content channel.
- **N5:** compose starts a Rust binary through `scripts.run`, and restarts it.
- **N6:** a Tauri 2 window shows a Leptos page in the dark purple theme. An
  `invoke` round trip works, and the core emits an event the page receives.

**Exit:** N1 and N3 to N6 pass, and N3 passes three runs in a row. Each result goes in
`docs/levels.md` with the version of each part.

#### A1 — The shell and the stack (M)

**Goal:** the app starts, checks, watches and stops the stack.

- The shell: a sidebar with Floor, Inbox, Engine and Settings, a skip link,
  and the dark theme.
- The seams first (§6.2): `crates/bastion-protocol`, the Engine trait with its
  in-memory fake, and the D19 dependency check in CI. `iii-sdk` moves behind
  the Engine seam, and today's `engine_status` command moves into
  `bastion-protocol`.
- The engine link, with reconnect and backoff. The `this machine` profile, in
  TOML in the OS config directory, with no secret (D12).
- Doctor: `iii` and its version, the pins in `worker-compose.yaml`, sandbox
  support and `gh` login run with the engine down. Model providers need the
  engine: `router::provider::list` says which have a credential, without the
  secret. Each failed check names its fix.
- The stack routine: Start runs Doctor and stops on a failed required check,
  then runs `iii compose --up`. Stop runs `iii kill --compose`. A stack that
  already runs is attached, not started twice. On quit, the owner chooses.
- CI on macOS and Linux: `cargo fmt --check`, `cargo clippy -- -D warnings`,
  `cargo test`.
- Replace the template `README.md`.

**Exit, live:** from a stopped machine, the app starts the stack and shows the
link as up. Doctor names a missing `ANTHROPIC_API_KEY` with its fix. After a
quit with the stack kept running, the next start attaches. CI passes.

#### A2 — The engine console (S)

**Goal:** the owner sees the engine without `ade`.

- Workers with their versions and state, from `compose::status`.
- Functions and triggers, from `engine::functions::list`.
- Queues and their depth.
- Live logs for one worker, from `compose::logs`.

**Exit, live:** stop one container from the console, see its state change by
push, start it again, and read its logs. Remove `ade` from the stack if the
owner no longer needs it.

#### A3 — The floor (L)

**Goal:** a request becomes a job that runs the short pipeline (D17), and the
owner watches every job from the floor.

- `bastion-core`, tests first: the job, the short pipeline, its stages and
  checkpoints, as `step(state, event) -> (state, effects)` (D18).
- The Store seam over `database`, with its in-memory fake: jobs, stages,
  artifacts, comments and checkpoints as rows with a `rev` (D8).
- The adapter loop (§6.1): load, step, save, run effects, emit. On start it
  resumes every active job.
- Register a workspace: a local repository path and its default branch.
- New request: a workspace and the request text. The job gets a worktree from
  `worktree` and a VM from `iii-sandbox`.
- Agent stages run turns on `harness`, with the model from `llm-router`. The
  workspace's charter (`CLAUDE.md`, `AGENTS.md`) goes into every turn. Every
  turn has a time cap.
- The floor: every job by stage, with its state and age. Up to
  `max_concurrent_sandboxes` (8) jobs run at once, and the rest queue.
- The inbox: each checkpoint with its artifact and its actions.
- The job view: the stage timeline, each stage's artifacts, and Stop. A turn's
  transcript from `session-manager` opens from its stage.
- An agent can stop with a question. The job goes to `needs_you`, and the
  answer starts the next turn.

**Exit, live:** submit three requests in two workspaces. Each job reaches the
approve checkpoint with a plan artifact, and the inbox shows three items. A
comment on one plan changes the next plan. Stop one job, and its VM is gone.
Quit and reopen the app, and the two other jobs carry on from where they were.
Every core transition has a unit test, and the adapter's tests run on the fake
engine and the fake store.

#### A4 — Review and landing (M)

**Goal:** a job's work lands only after a person decides.

- The diff artifact: the adapter reads it from the job's worktree against its
  base. The page renders it per file, unified or side by side,
  with syntax colors, and shows only the changed files.
- Comments on a diff line, sent verbatim (D16).
- The checkpoint dial (D15), per checkpoint, with merge never below `hold`.
- Land locally with `worktree::land`, or push and open a PR through `github`.
  Watch CI, and a failure starts a bounded fix turn (D16).
- Merge after the owner's decision.
- "Open in editor" passes the worktree path to the owner's editor.

**Exit, live:** a job's diff gets a line comment, and the next diff answers it.
The job is approved and lands locally. Another opens a PR, a failing CI run is
fixed by the agent, and the owner merges it.

#### A5 — The full pipeline (L)

**Goal:** a job can run wipp's whole factory.

- Every stage wipp has, with its person checkpoints:
  - Shape: provision, prepare, scope, discover, clarify (person), plan,
    approve (person).
  - Acceptance tests: define, your checks (person), specify, baseline (the
    tests must fail first), falsify.
  - Build: implement (one turn per plan item, tests first), checks, refactor
    with the owner's pick, acceptance, review with the owner's pick.
  - Ship: ship (commit, push, PR), CI, CI fix, prove, comprehend (the
    briefing), UAT (person), merge (person), deploy (watch CD), learn,
    teardown.
  - Side paths: onboard, revive.
- Split a job into parts that depend on each other. The floor shows parts
  under their job.
- Read-only stages get a read-only grant. A changed tree in one is reverted.
- The owner picks a pipeline per request, and sets the dial per checkpoint.

**Exit, live:** a request in this repository runs the full pipeline to a
merged change. Each stage leaves an artifact, the baseline tests fail before
the build and pass after it, and each checkpoint obeys its dial.

#### A6 — Governance (L)

**Goal:** every agent action is checked against the rules and recorded, as in
wipp.

- The gate: every tool call in a turn goes through a `pre-trigger` hook the
  app binds (N3). The rule lives in `bastion-core`. A call the gate cannot
  judge is refused.
- A refused call stops the turn and reaches the inbox. The owner retries with
  feedback, allows that exact call once, or stops the job.
- Constraints in three tiers: delivery, workflow, and the workspace's charter.
  Each turn is told the ones that apply. The Constraints pages show the board,
  one constraint's detail, and the forms to add and change one.
- Capabilities: the grant of each stage, shown and changed per stage.
- Repository hooks: which of the workspace's hooks run at which stage, and
  their results.
- The audit record: every decision and every write-like call, in a hash chain
  with one owner. Verify and export from the Audit page. The audit path has a
  timeout test, and a stalled audit shows in the console.

**Exit, live:** a turn's write outside its grant is refused and reaches the
inbox. "Allow once" lets exactly that call through, and the next one is
refused again. A constraint added on the page is told to the next turn. Audit
verification passes over a whole job.

#### A7 — Operations (L)

**Goal:** the owner runs the factory day to day with the pages wipp has for it.

- Workspaces, with onboarding for a repository that has no settings.
- Sessions: every agent session with its transcript.
- Sandboxes: every VM with its job and age, and Stop. A warm pool keeps one VM
  ready per workspace.
- Costs: spend per job, stage, model and day.
- Settings: the factory, pipelines, workspaces and oversight presets, with
  history, versions and restore.
- Models: a model and provider per stage, from what `llm-router` offers, and an
  override per job.
- Controls: stop, escape, override, split, pause intake and destroy.
- Provider keys live in the `secrets` worker.

**Exit, live:** onboard a new repository and land a job in it. The Costs page
shows that job's spend by stage. Change a setting, restore the version before
it, and the next job uses the restored value.

#### A8 — Learning and verification (M)

**Goal:** the factory learns from its jobs and measures itself, as wipp does.

- Learn: the stages that report surprises (specify, implement, prove) feed the
  learn stage, which proposes constraints.
- Proposals: accept or reject each one. An accepted proposal changes the
  constraint set.
- Evals: grade a job, and backtest a constraint against past jobs.
- Verification: the share of shipped jobs with an escape over a window, against
  a threshold the owner sets, and what reviews find that no gate does.

**Exit, live:** a job's surprise becomes a proposal, the owner accepts it, and
the next job is told the new constraint. The Verification page shows the
escape rate for the jobs so far.

#### A9 — The browser agent (M)

**Goal:** an agent checks its work in a real browser, and the owner sees what
it saw.

- **Spike first:** `bastion-browser` drives Chrome over CDP with
  `chromiumoxide`. A screenshot and a screencast reach `storage`.
- Browser tools for turns: open, click, type, read the page, screenshot, and
  record.
- Screenshots and recordings are artifacts in the stage that made them.
- A pipeline can require a browser check before review.

**Exit, live:** a job changes a page in a test app, and its browser check
records the page. The owner comments on the screenshot, and the next turn
fixes what the comment names.

#### A10 — Notifications and tray (M)

**Goal:** the owner learns that a job needs them without watching the window.

- Notify on `needs_you`, `blocked`, `failed` and `landed`. A click opens the
  inbox item or the job.
- The tray shows the inbox count and the stack state, with start and stop.
- After a reconnect, the core reads every active job again.
- A setting turns each kind of notification off.

**Exit, live:** a checkpoint raises a notification within 2 s. A click opens
it, the approval moves the job on, and the tray count goes from 1 to 0.

#### A11 — Packaging and updates (M)

**Goal:** the app installs and updates itself on another machine.

- A signed and notarized macOS build for Apple Silicon, and an AppImage and a
  `.deb` for Linux.
- `tauri-plugin-updater` with a signed manifest from GitHub releases.
- The app checks each Bastion worker's version, and refuses a stack it does not
  know, with the fix.
- A release workflow in CI, from a tag.

**Exit, live:** on a clean Mac, install from the release and start the stack
from the app. Publish a newer release, and the app updates itself.

## 8. Verification rules

- Each exit runs live, after the phase's last change.
- Rust passes `cargo fmt --check`, `cargo clippy -- -D warnings` and
  `cargo test` across the workspace.
- App checks run through `tauri-driver` in Linux CI. It has no macOS support,
  so macOS gets the Rust tests and a manual smoke test.
- Each changed file passes the CodeScene review and the pre-commit safeguard.
- A live check that spends money says so first, with its rough cost.

## 9. Lessons from wipp

wipp (`~/Code/wipp`) is the model for the pipeline. These lessons carry over.

| Lesson | Where it lands |
|---|---|
| The `state` worker lost a live job on restart. | D8: rows in `database`. |
| A last write overrode Stop. | D8: `rev` on every row. |
| A turn that never ended stalled its job forever. | A3: every turn has a time cap. |
| A credential in a VM taints the VM. | D14: pushes happen on the host. |
| Feedback paraphrased by the factory lost the point. | D16: comments go verbatim. |
| A fix loop with no bound ran until it was stopped. | D16: a count, and a stop on a repeated refusal. |
| The gate's own audit log stopped answering, and every call was refused. | A6: the audit path has a timeout test, and its failure shows in the console. |
| iii upgrades broke pins and config names. | A11: the app refuses a stack it does not know. |

## 10. Later: the owner's box

The app will reach a stack on a box the owner controls, after A11. Nothing here
starts until the owner asks.

- **Transport:** an SSH tunnel to the engine port on the box's loopback, with
  the owner's own SSH config and agent. The app stores no SSH key.
- **Profile:** a new `kind = "remote"`, with `ssh_host` and the far port (D12).
- **Spike first:** N1 passes through the tunnel.
- Features in workers need nothing new. The app's core features (Doctor, the
  stack) run on the box over SSH.
- Diffs come from the worktree on the box through the engine, so review needs
  no file sync.
- The factory still runs in the app (D18), so jobs on the box advance only
  while the laptop's app is connected. If that is too limiting, a second
  adapter hosts `bastion-core` in a worker on the box, with no core change.
- The title bar names the profile.
- **Exit, live:** from the laptop, a job on the box lands. The tunnel is killed
  mid-job, the app reconnects, and the job finishes once.

## 11. Risks

| Risk | What settles it |
|---|---|
| N3 failed once under the old name, and the cause is unknown. | It passed three runs in a row in A0. If a turn hook fails again, the spike prints the transcript. |
| Jobs stop when the app quits (D18). | The adapter resumes every active job on start, and A10 keeps the app running in the tray. |
| CDP screencasts may be hard to turn into a playable recording. | A9's spike. The fallback is a frame sequence shown as a slideshow. |
| A large diff may be slow to render in the webview. | A4 renders one file at a time, and collapses files over a set size. |
| App checks have no driver on macOS. | Linux CI runs `tauri-driver`. |

## 12. Progress

| Phase | State | Evidence |
|---|---|---|
| A0 | done | N1 and N3 to N6 pass (2026-10-30). N2 is dropped (D18). Details in `docs/levels.md`. |
| A1 | done | All six steps are in: the seams (424c69b), profiles (c62f8e2), Doctor (ea62b16, 763a79a), the stack routine (ac36f24), the shell (8f6e6b5), and CI and the README (0351c35). 19 tests pass, and CI passes on macOS and Linux. Live exit (2026-10-30), driven in the window: from a stopped machine, Start ran Doctor and brought up all 24 containers, and the page showed the link up with 378 functions by push. Doctor named the missing `ANTHROPIC_API_KEY` with its fix. Closing the window asked the quit question, and "Keep it running" quit the app with the stack up. The next start attached to the same compose daemon (pid 89588) and started nothing. |
| A2 | done | The console on the Engine page: containers with versions, state, stop, start and live logs; functions and trigger types with a filter; queues with depth, delivered, failed and dead letters. One `console` read through the Engine seam, 23 tests. Live exit (2026-10-30): `judge` stopped and started from the window, a stop made from the CLI reached the page by push with no reload, and its logs streamed. `ade` was removed with `compose::remove` (23 containers), so nothing in the stack serves HTTP. |
| A3 | done | `bastion-core` (9caf363), the Store (93b5cb8), spike N7 (c640769), the factory loop (88f45fb), and the floor, inbox and job panel (778016d), with fixes from the exit. 28 app tests and 15 core tests pass. Live exit (2026-10-30), driven in the window: two workspaces added through the form; three requests in them reached the approve checkpoint with plan artifacts, three plan turns on gpt-5-mini; a comment on the `subtract` plan (add a docstring, rename the test) came back in the next plan; `multiply` was stopped and its VM went away; the app was quit with the stack kept running and reopened, and both other jobs were still at approve; `divide` was then approved, ran its build turn, and reached review with its artifact. |
| A4 | next | — |
| A5 to A11 | — | — |
| Later (§10) | on hold | — |

**What A1 found:**

- `iii-sdk` 0.25.0 reconnects by itself at a fixed delay, so the app adds no
  backoff. It reads `get_connection_state()` every 500 ms and pushes changes,
  because the SDK has no listener for it.
- `router::provider::list` says which providers have a credential, without the
  secret. So the provider check runs once the engine is up, and the machine
  checks run before it.
- `iii compose --up` is the compose daemon itself, in the foreground. The app
  starts it detached in its own process group, with its output in
  `compose.log` in the app's log directory. Run from a terminal, it stops when
  that terminal job stops, and so does the stack.
- The stack inherits the app's environment. A provider key set only in an
  interactive shell does not reach an app started some other way.
- `iii trigger` prints its answer and then does not exit.

**What A2 found:**

- `observe` watches only functions. `engine::functions-available` is a
  trigger type, so the app binds it with the deprecated `register_trigger`, as
  N3 does. It fires when a worker's functions join or leave, which is how a
  container's state reaches the page.
- compose reports `owned: false` for a container whose process is gone, so
  Start shows for any container that is not ready.
- Trunk watched the whole checkout, and the stack writes logs into it, so each
  log line rebuilt and reloaded the page. `Trunk.toml` now watches only what
  the view is built from.
- `bastion-steps` is still declared in `worker-compose.yaml`, though D18 left
  the factory with no queue.

**What A3 found so far:**

- `sandbox::create` cannot mount a host folder, so a job's branch goes into its
  VM as a git bundle and its commits come back the same way (N7).
- `harness::status` gives a finished turn's `result` and `result_error`, which
  is all the core needs, and it lets resume catch a turn that ended while the
  app was closed.
- `worktree::create` answers with `worktree_id`, and `worktree::land` reports
  only on its `worktree::landed` and `worktree::land-blocked` triggers.
- Each turn runs on gpt-5-mini through OpenAI until A7, because the stack has
  no Anthropic credential yet.
- `harness::status` truncates `result` and encodes it as a JSON string, so a
  turn's report comes from its last assistant message in `session::messages`.
- The bundle folder in the app's cache must exist before `git bundle create`.
- The factory starts after the page loads, so it announces itself with an
  empty `job-changed`, and every page reads again.
- Page reads overlap when pushes come fast, so each page keeps only its latest
  read. An artifact is written before the page hears of a change, so a panel
  that mounts on that change sees it.
