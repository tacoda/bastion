# Bastion plan

Bastion is a harness and a factory for agent work, on the iii engine. The
factory runs workflows: graphs of steps a person can see and govern, and any
workflow can run on it (D35). A request goes in, a reviewed change comes out,
and nothing merges itself. What
Bastion adds is governance you can measure: constraints and capabilities built
into the harness's hooks, a way to promote and demote each one, a
tamper-evident audit record, and A/B evals that show what a change to the
harness does. It is one app: a native Rust client (iced from phase G, D37; a Tauri 2
window with a Leptos frontend until then) and a Rust core, against a local
engine that the app starts. A remote engine on the
owner's own box comes later (§10). Written 2026-10-30, revised the same day.

It is general. The owner sets up their own accounts and settings so Bastion
uses the processes, products, tools and policies they already use: their forge,
their tracker, their CI, their model providers, their pipeline and their rules.

**The words** (used the same way everywhere in Bastion):

- **Charter:** a project's own rules, written by people and kept in the
  project: its `CLAUDE.md` or `AGENTS.md`, and the constraints and capabilities
  it declares. Each workspace has a charter, or runs without one on the owner's
  defaults.
- **Harness:** what runs one agent turn and applies the charter to it. Hooks
  check each call against the constraints and capabilities in force. Bastion is
  a harness.
- **Factory:** the pipeline of stages and checkpoints that turns a request into
  a landed change, with turns in the harness at its agent stages. Bastion is a
  factory.
- **Constraint:** a rule about what a turn must or must not do, on a ladder:
  told (in the brief), checked (a hook records breaches), enforced (a hook
  refuses the call).
- **Capability:** a function a turn may call, on a ladder: denied, ask (the
  call waits for the owner), allowed.
- **Promote, demote:** move one constraint or capability one rung, as a
  recorded decision the owner can back with an eval.

The **job** is the main idea: one request, like a ticket. The app reads like
GitHub Actions and like wipp's Jobs page. The first view lists every job, with
the ones waiting on the owner first. A job's page shows its pipeline as steps,
each with its state, its artifacts and its agent turns, and a panel at the top
when the job needs the owner. Antigravity starts at the agent; Bastion starts
at the job.

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
2. The Jobs page lists every job with its state and stage, the ones waiting
   on the owner first. The job page shows each step of its pipeline, as a run
   of GitHub Actions does. Both update by push.
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
10. Every constraint and capability is a hook decision, so an eval of two
    harness configurations shows a difference in behavior the owner can act on.
11. The audit record detects any change to a past entry.
12. Forge, tracker, CI, model providers, notifications, pipeline and policies
    are the owner's settings, not built in.

## 2. Non-goals

- Not an editor or a file browser. The diff artifact is the only view of code.
  "Open in editor" hands a worktree path to the owner's own editor.
- Not an agent chat. The owner talks to a job through its artifacts and
  checkpoints, not through a prompt box per agent.
- Not a browser app yet. The page runs in the native window and talks to its
  Bastion servers over HTTP (D28).
- No GitHub sign-in. People sign in to a shared Bastion server with a token
  the server issues (D31), so wipp's GitHub sign-in and org check are not
  ported (§7.1).
- Not a new agent runtime. Turns run on the stock `harness`, `llm-router` and
  `iii-sandbox` workers.
- No managed hosting. Bastion runs locally by default; a shared Bastion
  server runs wherever its admin puts it (D32). wipp's Fly deploy is not
  ported (§7.1).
- Not Windows. Sandbox VMs need Apple Silicon or Linux with `/dev/kvm`.

## 3. Fixed decisions

| # | Decision | Why |
|---|---|---|
| D1 | Rust for all Bastion code. TypeScript or JavaScript only where no Rust path exists, and each such file is named in §4 with its reason. | One language. |
| D2 | Replaced by D37 (2026-11-09), at G4. Tauri 2, the stable line. `cargo search` shows 3.0.0-alpha.5 on 2026-10-30, so 3 waits. | A native app with a Rust core. |
| D3 | Replaced by D37 (2026-11-09), at G4. The frontend is Leptos 0.8, client-side rendered to WASM and built by Trunk. | Rust pages in Tauri's webview. |
| D4 | One dark theme, purple as the main color (`--primary: #7c3aed`, `--primary-hover: #6d28d9`, `--link: #c4b5fd`). No light theme. Tailwind 4 runs as Trunk's standalone binary. | The owner's choice. No Node toolchain. |
| D5 | One Cargo workspace: the frontend crate at the root, the app in `src-tauri/`, shared code in `crates/`, Bastion workers in `workers/` (§4). | One build, one lockfile, one CI. |
| D6 | Every Bastion worker is a Rust binary on `iii-sdk` 0.25.0. Compose starts it through `scripts.run`. | The same engine, with no second runtime. |
| D7 | A stock registry worker comes first. Bastion writes a worker only for what no pinned worker does, and §5 records each choice. | Less code to own. The stack already pins `harness`, `iii-sandbox`, `worktree`, `approval-gate`, `workflow`, `session-manager`, `github` and `storage`. |
| D8 | Bastion's records (jobs, stages, artifacts, comments, decisions) live in the `database` worker as typed columns. Each row carries a `rev`, and a stale write is refused. | wipp lost a live job in the `state` worker on restart, and a last write once overrode a Stop. |
| D9 | The pages reach the engine only through typed Tauri commands. A command maps to one engine function or one core routine, and no command takes a function id. | One narrow surface, listed in one file. |
| D10 | Live updates are push. The engine adapter observes engine events (a turn completed, a CI run changed) by push. After each change it persists, the app emits a Tauri event `{kind, id, revision}`, and the page reads the resource again. | wipp polled. Push needs no timers and no stale views. |
| D11 | Replaced by D31 (2026-11-06). One owner, no sign-in. The actor on every record is `git config user.name`. The owner's accounts with other services (forge, tracker, providers) are settings, kept in `secrets` (A7). | One person on one machine needs no accounts. |
| D12 | Local is the default and only mode for now. The engine address is a profile field, never a constant. | The remote box (§10) becomes a new profile kind, not a rewrite. |
| D13 | The app starts the stack with `iii compose --up -f worker-compose.yaml`, and attaches to a stack that already runs. On quit, the owner chooses whether it keeps running. | The owner's choice. No terminal. |
| D14 | Every agent turn runs in an `iii-sandbox` VM on its job's worktree. A credential never enters the VM. Pushes and PRs happen on the host. | wipp's lesson: a sprite that saw a credential is tainted. |
| D15 | Replaced by D33 (2026-11-06). Merge is a person's decision, or `hold` at the lowest. Every other checkpoint has a dial: `ask`, `hold` (default after 30 min) or `auto`. | wipp's oversight model. |
| D16 | A comment on an artifact goes to the agent verbatim, as its next brief. A fix loop stops after a set count, or sooner when a refusal repeats word for word. | wipp's bounded loops. Paraphrased feedback loses the point. |
| D17 | Every job runs a pipeline. The smallest pipeline is plan, approve, build, review, land, and it is the default until A8 adds the rest. | The factory is the unit. A free-form task is a short pipeline, not a second mode. |
| D18 | All factory logic lives in the app, in `crates/bastion-core`, apart from the view and from iii (§6.1). The core takes an event and the current state, and returns the new state and a list of effects. It does no I/O, has no `async`, and does not depend on `iii-sdk`, Tauri or Leptos. | The owner's choice. The rules are tested as plain functions, with no engine and no window. |
| D19 | The seams in §6.2 are the only ways across layers, and Cargo dependencies enforce them. A CI check fails if `bastion-protocol` gains a dependency other than `serde`, if `bastion-core` gains one other than `serde` and `sha2` (for the audit chain, D21), or if `iii-sdk` is imported outside `src-tauri/src/engine/`. | The owner wants clean seams. A seam that only a reviewer guards erodes. |
| D20 | Every constraint and capability is enforced in a harness hook, and its ladder position is data. A turn's behavior changes only through those hooks and that data. | An eval can then name what changed. A rule kept only in a prompt cannot be measured. |
| D21 | The audit record is a hash chain with one writer. Each entry carries an HMAC-SHA256 of its fields and the entry before it, keyed with a secret kept in the macOS Keychain, not in the record, and verification walks the chain. | Any change to a past entry breaks the chain, and whoever can edit the database cannot recompute it without the Keychain key. It still cannot stop a change, or detect entries cut from the end; a remote append-only copy (§10) closes that, later. |
| D22 | An eval compares two harness configurations on the same requests and the same workspace snapshots, and reports the difference with the hook decisions behind it. | A promotion or demotion comes with evidence, not a guess. |
| D23 | Forge, tracker, CI, model providers and notifications are adapters behind seams, chosen in Settings. A new product is a new adapter, not a core change. | The owner keeps the tools they already use. |
| D25 | Every constraint and capability belongs to one layer. The charter is the repository's own, read from it (`.claude/settings.json` today). The harness runs an agent's turn and binds its calls. The factory moves a job through its pipeline and binds review, land, pull requests and merges. The Constraints and Capabilities pages each show the three. | The owner's words: a charter belongs to a repo, and the harness and factory are Bastion's, built on iii. A rule is easier to judge when its place is plain. |
| D26 | The owner changes the harness and the factory as data, with no code change. Their rules, grants and settings live in Bastion's database, served by the stack's `database` worker: the owner edits them on the pages, or directly in `data/bastion.db`, and the gate reads them again before each turn. The first-run defaults are a SQL file the stack carries (`seed/governance.sql`), not constants in Bastion's code. | The owner's choice (2026-10-31): the owner updates their own settings and configuration while the factory runs. Spike N11 showed the `configuration` worker could carry them as YAML; the owner chose the database, which keeps the rules, their history and the audit in one place. |
| D27 | Bastion is a server and its clients. `bastion-server`, one Rust binary on `axum` and `tokio`, is the source of truth: it holds the factory (`bastion-core` and its adapter), the store, the gate, the audit chain, the forge, and the stack on its machine. Business logic lives in the server; iii workers are the implementation under it, reached only through the Engine seam. A client shows state, sends the owner's actions and follows updates, and keeps no business state. Replaces D18's "in the app", D13's app-owned stack, and §10's second adapter. | The owner's choice (2026-11-06). Jobs advance with no app open, and one server serves the app on this machine or another. D18 kept the core pure, so it moves without change. |
| D28 | The API is HTTP with JSON. Each route is one action on one resource, its types live in `bastion-protocol`, and every request carries the Bastion server's bearer token. The view calls the server from the webview. Replaces D9's Tauri commands, except the few that belong to the window (S3). | One surface for every client, typed once. |
| D29 | Push is server-sent events. `GET /events` streams D10's `{kind, id, revision}` with an event id. A client that reconnects with `Last-Event-ID` gets what it missed, or a `resync` when it is too far behind, and then reads again. The view reads the stream with `fetch`, so the token goes in a header. WebSockets wait until a client must stream to the server, such as a live terminal. | Push is one-way today. SSE is plain HTTP, so it passes the same token, proxy and SSH tunnel as the API. |
| D30 | The app keeps a list of Bastion servers the owner added, each a name, a base URL and a token kept in the Keychain. The server the app starts on this machine is `local`. A Bastion server holds its own workspaces, one per repository, each with its own setup, checks and tests. Each job runs on one Bastion server: New request picks the server, `local` by default, then one of its workspaces, and the job page names the server. Every server stays connected, and the Jobs page lists the jobs of all of them. A page about one server (Engine, Settings, Constraints, Capabilities, Audit Log, Evals) has a server picker at its top, `local` by default; the sidebar names no server. "Bastion server" is the one word for it: not target, not profile. Replaces D12's profiles. | The owner's choice (2026-11-06): use several backends at once, chosen job by job. |
| D31 | A Bastion server has people, each a name and a role. An admin manages the server: its settings, workspaces, constraints, capabilities, stack and people. A member submits requests and acts on their own jobs. A viewer reads everything and changes nothing. A person signs in with a token the server issued them, shown once and kept only as its hash. Every record names its actor: who requested a job, who decided each checkpoint, who wrote each audit entry. A local server's first start makes the person who started it its admin, so one person on one machine sees nothing new. Replaces D11. | The owner's choice (2026-11-06): local by default, with shared Bastion servers that several people sign in to. Tokens need no outside service, and revoking one is deleting it. |
| D32 | A Bastion server listens on loopback unless its admin gives it an address and a TLS certificate and key; it refuses a non-loopback address without them. | A token sent over plain HTTP can be read by anyone on the network. |
| D33 | Every step of a pipeline is lights-on (it waits for a person to approve its result or send it back with feedback) or dark (it goes on at once). Every step starts lights-on, and each can be set on its own, merge included. A workflow's level follows from its steps: lights-on when every step has a person's gate, dim when at least one step up to the merge has one, and dark when none up to the merge has one (no human oversight of what merges). No step waits on a timer. The checkpoint stages fold into the steps they judge, and merge is a step that merges. Oversight decides only human approval and feedback: every step's result is kept, and every decision is logged, whatever the dial. A person's decisions are in the audit log with their name, and a dark step's decision is there too, named as the dial's. Replaces D15. | The owner's choice (2026-11-06): the factory runs the whole pipeline, and the amount of oversight is the variable. A step is governed by a person or runs alone; a timer that approves by itself governs nothing. |
| D34 | Model provider keys are configuration, not environment. Bastion stores each key in the stack's `secrets` worker (store `vault`, readable only by `llm-router`) and points the router's provider entry at it as `secret://NAME`; the router resolves it at each request and reports whether a provider has a key without returning it. Providers with their own worker (Anthropic, OpenAI, DeepSeek, xAI, Z.AI for GLM, Kimi, OpenRouter) take their key that way; other OpenAI-compatible hosts (Together, Fireworks, Qwen through DashScope, a local Ollama) go through a Bastion provider worker that serves any number of them. | The owner's choice (2026-11-06): a key for any provider, with a spread wide enough that most people can use it, and no dependency on `ANTHROPIC_API_KEY` in the environment. |
| D35 | A pipeline is a workflow: a graph of steps that a person edits on a canvas. Each node is a step: what runs (a skill, the preferred building block; a prompt; a command such as a bash command, a script or a task runner; or a Bastion stage), its oversight dial (D33), a note that the node expands to show and that the step's agent reads, and the results it declares, such as documents, kept in the database. A link sends one step's results to another. Links carry no conditions: a step's entry criteria are the results of the steps linked into it, and its exit criteria are the results it declares, and a code check, not a model, holds a step that lacks either. Bastion ships a few workflows, and people make their own for themselves and others to use. For now only an admin makes or changes a workflow; members and viewers use them. | The owner's choice (2026-11-06). |
| D36 | Claude takes a key only: an Anthropic API key, or a cloud provider's credentials. Bastion offers no sign-in with a Claude subscription and does not run the `provider-claude-code` worker. | Anthropic's terms (code.claude.com, Legal and compliance) keep Claude subscription sign-in for Claude Code and Anthropic's own apps, and do not permit a third-party product to route requests through a Free, Pro, Max or Ultra plan. Checked 2026-11-06. |
| D37 | The client is a native Rust app on `iced` 0.14, drawn on the GPU through `wgpu`. It is a new client of the Bastion server and nothing more: it reaches a server only through D28's HTTP API and D29's events, with the types and the route table from `bastion-protocol`, and it holds no factory logic. It looks the same as the Leptos pages: the same sidebar, pages, layout and D4's theme, written as one Rust style module. The window's own jobs (the server list in the Keychain, starting the `local` server, quitting) move with it. Replaces D2 and D3 at G4, when Tauri, Leptos, Trunk and Tailwind leave the repository. | The owner's choice (2026-11-09): the Tauri and Leptos app is sluggish. Spike N14 measured GTK 4 and iced on the same screens, and the owner chose iced: it started in about half GTK's time, held the display's full frame rate where GTK held 60 fps and hitched on the diff, and ships as one binary with no libraries to bundle. |

## 4. The repository

```
Cargo.toml  index.html  Trunk.toml  styles.css
src/                  bastion-ui: the Leptos frontend (WASM), until G4
src-tauri/            bastion: Tauri, the window and its commands, until G4
  src/commands/       one file per resource (D9, until S3)
  src/profile.rs      profiles (D12, until S4)
crates/
  bastion-desktop/    the native client (D37, G): pages, the server list,
                      the HTTP client and the event stream
  bastion-server/     the server (D27): the factory and what it runs on
    src/engine/       the engine adapter: every iii call
    src/factory/      effects run, turns, the acceptance steps
    src/store.rs      records in `database`, with numbered migrations
    src/stack.rs      iii compose as a child process
    src/doctor.rs
    src/rt.rs         the process-wide tokio runtime
  bastion-protocol/   the typed commands the view calls (D9)
  bastion-core/       the factory logic (D18): jobs, pipelines, stages,
                      checkpoints, the dial, fix loops, briefs
  bastion-spikes/     spikes and live checks, as examples
workers/
  bastion-browser/    a browser driven over CDP
worker-compose.yaml   the stack the app starts
PLAN.md  docs/
```

`cargo tauri dev` runs the app. `cargo run -p bastion-desktop` runs the native client (D37), which replaces it at G4.

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
| Diffs | app: the adapter reads them from the job's VM | `iii-sandbox` (stock) | A4 |
| Constraints and capabilities: the gate's rules and the ladders | app: `bastion-core`, bound as hooks | `harness` hooks | A5 |
| Calls held at `ask` | worker | `approval-gate` (stock) | A5 |
| The audit chain | app: one writer, rows in `database` | `database` (stock) | A5 |
| A/B evals | app: `bastion-core` compares; runs are jobs | `eval`, `judge` (stock) | A6 |
| Accounts and their secrets | worker | `secrets` (stock) | A7 |
| Forge: push, PRs, CI watch, merge | worker, one adapter per forge | `github` (stock) | A7 |
| Issue trackers as request sources | one adapter per tracker | — | A7 |
| Browser | worker | `bastion-browser` | A11 |
| Notifications and tray | app | — | A12 |

The factory logic is the one exception to the worker rule above. It lives in
`bastion-server` (D27), which reaches the workers through the Engine seam. In
this table, "app" now means the server for everything but the window,
profiles and notifications: S1 moved the code, and S3 moves the app onto the
server's API.

`ade` is iii's own console, on `127.0.0.1:3114`. It stays in the stack until
A2's console covers what the owner uses in it, and then it leaves.

## 6. Concepts

From the top down:

- **Jobs page:** every job, in three groups: waiting for you, in progress,
  and finished. Filter by workspace and search by text.
- **Job page:** one job as a run. The request, its state, the pipeline as
  steps with their artifacts and turns, and the owner's actions when it waits.
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
| **Engine** | A trait in the adapter: `call`, `observe`, `register`. The only code that imports `iii-sdk`. | `crates/bastion-server/src/engine/` | An in-memory engine that records calls and replays events. |
| **Store** | A trait in the adapter: load a job, save a job with its `rev`, list active jobs. Backed by `database` through the Engine seam. | `crates/bastion-server/src/store.rs` | An in-memory store that refuses a stale `rev`. |
| **Machine** | A trait in the server: run a program, read a file, test a path, name the platform. Doctor and the stack routine reach the OS only through it. | `crates/bastion-server/src/machine.rs` | A machine with canned runs, files and platform. |
| **Commands** | The typed Tauri commands and their request and response types, in `crates/bastion-protocol`. The view and the app both depend on it. | `bastion-protocol` | The view's tests use canned responses. |

Allowed dependencies:

```
bastion-ui (view)  ──>  bastion-protocol
bastion (app)      ──>  bastion-protocol, bastion-core, bastion-server, tauri
bastion-server     ──>  bastion-protocol, bastion-core, iii-sdk, tokio (never tauri)
bastion-core       ──>  serde, sha2 (the audit chain, D21)
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
| Jobs page: waiting, in progress, finished, filters | The Jobs page | A3 |
| New request | New request | A3 |
| Job page: needs, panels, turn, timeline | The job page | A3 |
| Job page: proof | The evidence of each check, per stage | A8 |
| Diff | The diff artifact | A4 |
| Every stage in `stages.py` (shape, acceptance tests, build, ship, side paths) | The full pipeline | A8 |
| Split jobs into dependent parts | Split | A8 |
| Oversight dial: `lights_on`, `dark` per step; a workflow is lights-on, dim or dark | The dial (D33) | O1 |
| Oversight presets: high, medium, low, off | Presets in Settings | A7 |
| The gate, refusals, retry with feedback, allow once | The gate in `bastion-core`, bound as a `pre-trigger` hook | A5 |
| Constraints: board, detail, forms, three tiers | Constraints pages, each constraint on a ladder | A5 |
| Capabilities: grants per stage | Capabilities page, each grant on a ladder | A5 |
| Repository hooks per stage | Repository hooks | A5 |
| Audit: hash chain, verification, export | Audit page | A5 |
| Repos, and onboarding a repository with no settings | Workspaces | A9 |
| Sessions | Sessions page | A9 |
| Sprites, and the warm pool | Sandboxes page, on `iii-sandbox` | A9 |
| Costs | Costs page | A9 |
| Settings, with history, versions and restore | Settings page | A7 |
| Model and provider per stage, and override per job | Models in Settings, override in the job view | A7 |
| Stop, escape, override, split, pause intake, destroy | Job and factory controls | A9 |
| The vault | The `secrets` worker | A7 |
| Learn, and proposals from what surprised a turn | Learn stage and Proposals page | A10 |
| Evals: grade and backtest | A/B evals of harness configurations | A6 |
| Verification: escape rate against a threshold | Verification page | A10 |
| Accounts, GitHub sign-in, org check, roles | The owner's own accounts with other services (A7); people with server-issued tokens and roles, no GitHub sign-in or org check (D31) | A7, P |
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

- The shell: a sidebar with Jobs, Engine and Settings, a skip link,
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

#### A3 — Jobs (L)

**Goal:** a request becomes a job that runs the short pipeline (D17), and the
owner follows every job from the Jobs page and the job page.

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
- The Jobs page: waiting for you, in progress and finished, with a workspace
  filter and search, and a New request page. Up to
  `max_concurrent_sandboxes` (8) jobs run at once, and the rest queue.
- The job page: the request and its badges, a needs-you panel with the
  actions the core allows, and the pipeline as steps. Each step shows its state
  and opens to its artifacts and its turn's transcript. Workspaces live in
  Settings.
- The job view: the stage timeline, each stage's artifacts, and Stop. A turn's
  transcript from `session-manager` opens from its stage.
- An agent can stop with a question. The job goes to `needs_you`, and the
  answer starts the next turn.

**Exit, live:** submit three requests in two workspaces. Each job reaches the
approve checkpoint with a plan artifact, and the Jobs page shows three
waiting for you. A
comment on one plan changes the next plan. Stop one job, and its VM is gone.
Quit and reopen the app, and the two other jobs carry on from where they were.
Every core transition has a unit test, and the adapter's tests run on the fake
engine and the fake store.

#### A4 — Review and landing (M)

**Goal:** a job's work lands only after a person decides.

- The diff: captured from the VM when a build finishes, kept as the review's
  artifact, and shown per file with line numbers.
- Comments on a diff line, sent verbatim (D16).
- The checkpoint dial (D15), per checkpoint, with merge never below `hold`.
- Land locally with `worktree::land`.
- "Open in editor" passes the worktree path to the owner's editor.

**Exit, live:** a job's diff gets a line comment, and the next diff answers it.
The job is approved and lands locally. A held checkpoint approves itself when
its hold runs out. Pushing and pull requests move to A7, where the forge is a
setting.

#### A5 — Constraints, capabilities and the audit (L)

**Goal:** the harness enforces the charter through hooks, and every decision is
on record (D20, D21).

- The gate: every tool call in a turn goes through a `pre-trigger` hook the
  app binds (N3). The rule lives in `bastion-core`. A call the gate cannot
  judge is refused.
- Constraints, each on its ladder (told, checked, enforced). A constraint comes
  from the workspace's charter or from the owner, and the Constraints pages
  show the board, one constraint's detail and history, and the forms to add,
  promote and demote one.
- Capabilities, each on its ladder (denied, ask, allowed), per stage. A call at
  `ask` waits on the owner on its job page: retry with feedback, allow that
  exact call once, or stop the job.
- A project with no charter runs on the owner's defaults, and the job page says
  so.
- Repository hooks: which of the workspace's hooks run at which stage, and
  their results.
- The audit record: every decision, every promotion and demotion, and every
  write-like call, in a hash chain with one owner. Verify and export from the
  Audit page. The audit path has a timeout test, and a stalled audit shows in
  the console.

**Exit, live:** a turn's write outside its grant waits on the job page. "Allow
once" lets exactly that call through, and the next one waits again. A
constraint promoted from told to enforced turns a passing call into a refused
one. Audit verification passes over a whole job, and fails after one byte of
the record is changed.

#### A6 — A/B evals (L)

**Goal:** the owner sees what a change to the harness does before keeping it
(D22).

- An eval runs a set of requests against fixed workspace snapshots under two
  harness configurations, A and B. A configuration is the set of constraints
  and capabilities with their ladder positions, the model per stage, and the
  pipeline.
- Each run records its outcome per request: landed or not, turns, refusals,
  checkpoint decisions, time and cost, plus a judge's grade.
- The Evals page shows the difference between A and B per request and in
  total, and links each difference to the hook decisions behind it.
- A promotion or demotion can be proposed with its eval attached, and the
  audit records the eval the owner acted on.

**Exit, live:** an eval of three requests under A (a constraint told) and B
(the same constraint enforced) shows a difference in refusals, and the
difference links to the refused calls.

#### A7 — Settings and integrations (L)

**Goal:** the owner runs Bastion with the processes, products, tools and
policies they already use (D23).

- Accounts, each kept in the `secrets` worker: a forge (GitHub, GitLab, or
  local git only), an issue tracker (GitHub Issues, Jira, Linear, or none) as a
  source of requests, model providers with their keys, and CI.
- Push and pull requests through the chosen forge, CI watched, a failing run
  starting a bounded fix turn (D16), and merge after the owner's decision.
- A request can start from a tracker issue, and its job links back to it.
- Models: a model and provider per stage, from what `llm-router` offers, and an
  override per job.
- Settings for the factory, pipelines, workspaces and oversight presets (high,
  medium, low, off), with history, versions and restore.
- Each integration is one adapter behind a seam, so a new forge or tracker is a
  new adapter, not a change to the core.

**Exit, live:** with a GitHub account set, a job opens a pull request, a
failing CI run is fixed by the agent, and the owner merges it. A job started
from a tracker issue links back to it. Change a setting, restore the version
before it, and the next job uses the restored value.

#### S — Bastion as a server (L)

**Goal:** the factory runs in `bastion-server`, and the app is a client of one
or more servers (D27 to D30). It runs between A8 steps 3 and 4, so the rest of
A8 lands in the server.

**Steps:**

- **S0, spike N12.** An `axum` server with one route and an event stream,
  called from the Leptos page in the Tauri window: CORS for the window's
  origin, the bearer token on `fetch` and on the stream, and a reconnect with
  `Last-Event-ID` after the server restarts. Then a second iii stack on this
  machine, on its own ports and data directory, for the exit's second Bastion server.
  Fallback if the webview cannot stream: the app's Rust side holds the
  connection and passes events to the page as Tauri events. If two stacks
  cannot run side by side, the second Bastion server waits for a box (§10).
- **S1, the split.** A `crates/bastion-server` crate takes the engine
  adapter, the store, the factory, the gate, the forge, evals, settings, the
  audit log, charter, spec, Doctor, the stack and the console from
  `src-tauri`. Tests move with their code. For this step the app links the
  server as a library, so nothing changes in the window. §4, §5, §6.2 and
  `scripts/check-seams.sh` follow.
- **S2, the API.** A route for each of today's commands, with its types in
  `bastion-protocol`, the token, and `GET /events`. The binary runs headless
  (`bastion-server --data <dir> --port <port>`): it starts or attaches to the
  stack, resumes jobs, and serves. Route tests run on the fakes.
- **S3, the view on the API.** The pages call the window's Bastion server over HTTP
  and follow its events. The app keeps only what belongs to the window: the
  quit question, "Open in editor", and later notifications and the tray. It
  starts the local server as a child process and no longer links it.
- **S4, servers.** The list of servers in Settings, `local` started by the
  app, a server picker on New request that defaults to `local`, the server's
  name on the job page, and the Jobs page listing every server's jobs.

**Exit, live:** with two servers connected (`local`, and a second stack on
this machine), a job runs on each, picked on New request. The app quits
and both jobs go on; it reopens and shows where each one is. The server is
killed mid-job, the window says so, and after a restart it reconnects and
reads again, and the job finishes once.

#### P — People on a shared Bastion server (L)

**Goal:** several people sign in to one Bastion server, each with a role, and
every record says who did what (D31, D32). It runs between A8 steps 5 and 6,
so split and the owner's choices are built per person.

**Steps:**

- **P1, people in the store.** Migration 2 adds `people` (name, role) and
  `tokens` (the token's SHA-256, its person, a label, when it was made and
  last used). A server's first start makes its admin, named by `--owner` or
  `git config user.name`, with the token in `data/token`.
- **P2, a person on every request.** The token resolves to a person, and each
  route checks the role: a viewer may only read, a member may submit and act
  on their own jobs, an admin may do everything. A refusal is 403 with why.
- **P3, the actor on every record.** A job keeps who requested it, each
  checkpoint who decided it, and each audit entry its actor. The Jobs page
  shows who asked.
- **P4, managing people.** Routes to list people, add one, change a role, and
  issue or revoke a token (shown once). A People section in Settings for
  admins. The page reads who you are on each server and hides what your role
  cannot do.
- **P5, serving beyond loopback.** `--listen <address>` with `--tls-cert` and
  `--tls-key`; without them the server refuses any address but loopback.

**Exit, live:** on a shared Bastion server, an admin adds a member and a
viewer. The member's request runs to its first checkpoint and the member
approves it; the viewer sees the job but cannot approve it, and the server
refuses the viewer's attempt with 403; the member cannot change a
constraint, and the admin can. The audit log names each of them.

#### K — Model providers in configuration (M)

**Goal:** an admin adds a key for any provider in Settings, and turns run on
it, with no provider key in the environment (D34).

**Steps:**

- **K1, spike N13.** Prove the path on a live stack: `secrets::set` with
  store `vault` and consumer `llm-router`, `configuration::set` on the
  router's entry with `providers.<id>.api_key = secret://NAME`, and
  `router::provider::list` reporting the credential as from a secret. Find
  whether `providers.openai.api_url` reaches a compatible host.
- **K2, keys in Settings.** A Model providers section: set or remove a
  provider's key, see whether each has one. Add `provider-deepseek`,
  `provider-xai` and `provider-zai` to the stack. Doctor stops requiring
  `ANTHROPIC_API_KEY` and warns only when no provider has a key.
- **K3, OpenAI-compatible hosts.** Deferred (the owner, 2026-11-06) until
  after O, W and A8, then built in full (2026-11-09). `workers/bastion-openai-compat`, a Rust
  worker on `iii-sdk` that registers one router provider per host the admin
  lists (a name, a base URL, a key), each speaking Chat Completions:
  Together, Fireworks, Qwen through DashScope, a local Ollama.
- **K4, models from every provider.** The model per stage picks from every
  provider that has a key.

**Exit, live:** with no provider key in the stack's environment, an admin
adds an Anthropic key and a DeepSeek key in Settings, and a job's plan
turn runs on each. Claude Code sign-in was part of this exit until D36
removed it. K3 is not
part of this exit.

#### O — Oversight per step (L)

**Goal:** the factory runs the whole pipeline, and a dial on each step says
how much a person oversees it (D33).

**Steps:**

- **O1, the core.** `Dial` is `lights_on` or `dark` (stored `ask` and
  `hold` read as `lights_on`, `auto` as `dark`). `Dials` holds one per step,
  each `lights_on` until set, and its level is lights-on, dim or dark as
  D33 derives it. The hold timer goes. When a step ends and its dial is
  lights-on, the job waits on its result: approve goes on, and a note sends
  the step back with it. The checkpoint stages leave the pipelines and fold
  into the step each judged: approve into plan, clarify into discover, your
  checks into define, review into build (short) and pick findings into code
  review (full), pick refactors into refactor, and UAT into comprehend.
  Their picks (findings, refactors, answers) stay what a person sends at
  that step. Merge becomes a step that merges, and the short pipeline's
  land is its merge. Assumptions, the owner's to overturn: a step that acts
  outside the job (ship, land, merge, deploy) waits before it acts, not
  after; a failing step's automatic fix loop (`revise`) stays automatic,
  and the dial judges only the finished result; under dark, discover goes
  on with its stated assumptions instead of waiting for answers. A stored
  job waiting at an old checkpoint waits on the step that checkpoint
  judged, and stored settings map each old dial to the steps it covered.
- **O2, everything logged.** Every step's result stays as its artifact
  whatever the dial, and every decision goes in the audit log: a person's
  with their name, a dial's as the dial's.
- **O3, the dials in Settings.** One dial per step, with three presets:
  lights-on (every step), dim (only the steps that act outside the job)
  and dark (no step). The page shows the workflow's level; the versions
  keep the dials.

**Exit, live:** a request runs the full pipeline with every dial at
lights-on and waits after each step; the same request with every dial dark
runs to a merged change alone; the audit log names each person's decision
and each dial's.

#### W — Workflows on a canvas (L)

**Goal:** a person picks a workflow for a request, and an admin edits
workflows as an interactive flow chart on a canvas (D35).

**Steps:**

- **W1, the model.** A workflow is a graph, kept on the Bastion server
  with its versions. A node is a step (stage, dial, note, the results it
  declares, its place on the canvas); a link joins two nodes, and the later
  one gets the earlier one's results. The graph has no cycle, each stage at
  most once, and one node that lands the change (land or merge) with only
  deploy and learn after it. Only an agent step declares results. A job
  keeps the version it started on and runs the nodes in link order, ties
  broken by the canvas, skipping every stage the graph does not have.
  Before a step: each result linked into it exists, or the job waits on a
  person. After a turn: its report gives each declared result as a
  `RESULT <name>:` section, kept as a document; a missing one is one more
  try with the reason, then a wait on a person. The built-in workflows
  (short, full, full without acceptance tests) are chains with no results
  and the settings' dials; they cannot be changed or deleted, only copied.
- **W1b, step kinds.** A node is a skill, a prompt, a command or a
  built-in step (a Bastion stage). A skill is the preferred building block,
  and the palette offers it first: an agent turn that reads and follows
  `.claude/skills/<name>/SKILL.md` in the repository. A prompt is an agent
  turn whose instructions are the node's note. A command is one shell line
  run in the job's VM in the repository, with no agent: a bash command, a
  script or a task runner (`make test`, `./scripts/lint.sh`, `npm run
  build`); exit 0 passes, and its output is its result. Each node has its
  own id, so a workflow can hold several skills, prompts or commands, and a
  job tracks the node it is at.
- **W2, the canvas.** A Workflows page draws each workflow's nodes and
  links. An admin (D31) drags a node, links two by dragging from one to the
  other, removes a node or a link, adds a step from a palette, and expands
  a node to set its dial, note and results; saving makes a new version.
  Everyone else sees it read-only.
- **W3, picking one.** New request picks a workflow, the server's default
  first. The job page draws the job's workflow with each step's state, and
  each result as its document.

- **W4, seeded workflows.** The seed (D26) carries eleven workflows to start
  from, each a copy an admin can change: research, bug fix, dependency
  upgrade and test coverage for engineers; weekly status, decision record and
  onboarding guide for managers; incident response, incident postmortem,
  runbook and infrastructure change for platform and SRE teams. Each reads with plan, a
  command or a prompt, writes with build, and waits for a person at land, so
  each runs dim. A document lands under `docs/`. A seeded workflow the owner
  deletes is not seeded again.

**Exit, live:** an admin copies a default workflow, removes a step, gives
plan a `design-notes` result linked into build, and saves; a member runs a
request on the copy; the job skips the removed step, the design notes are
kept as a document and reach the build's brief; a viewer sees the workflow
but cannot edit it.

#### A8 — The full pipeline (L)

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
- Split a job into parts that depend on each other. The Jobs page shows parts
  under their job.
- Read-only stages get a read-only grant. A changed tree in one is reverted.
- The owner picks a pipeline per request, and sets the dial per checkpoint.

**Steps**, from wipp's stage graph (`stages.py` at `f7c7e02`):

1. **The graph in the core.** Every stage, its kind (turn, checkpoint or the
   factory's own step), its next stage per outcome, and whether it is
   read-only. The eight checkpoints of wipp's `oversight.py` (questions,
   approve, checks, refused, refactor, review, uat, merge), each with its own
   dial; merge never goes below hold. Two pipelines: the short one, and the
   full one with acceptance tests when the workspace has checks to run. The
   bounds of wipp's `REVISIONS`, and a revise loop that stops when a refusal
   repeats word for word. Pure, with tests. The app still runs the short
   pipeline.
2. **Shape.** Scope, discover, clarify and plan, live. Discovery's tasks feed
   implement. Read-only turns: the tree is snapshotted before the turn and put
   back after it if it changed.
3. **Acceptance tests.** Define, your checks, specify, baseline and falsify.
   The baseline runs each check before the build, and a green check must go
   red once against broken code. The approved files are fingerprinted, and
   verify fails on a changed one.
4. **Build.** Implement one turn per task, checks, verify, refactor with the
   owner's pick, acceptance, and review with the owner's pick.
5. **Ship.** Ship, CI, CI fix, prove, comprehend, UAT, merge, deploy, learn
   and teardown, then revive. Onboarding moved to A9 (2026-11-06).
6. **Split.** Scope's split verdict makes parts that follow each other. The
   Jobs page shows the parts under their job.
7. **The owner's choices, then the exit.** A pipeline per request and each
   stage's proof on the job page. The dial per checkpoint became phase O
   (D33).

**Exit, live:** a request in this repository runs the full pipeline to a
merged change. Each stage leaves an artifact, the baseline tests fail before
the build and pass after it, and each checkpoint obeys its dial.

#### G — A native client (L)

**Goal:** the owner works in a native app that looks the same as today's and
is much faster, on the same Bastion servers (D37).

Added 2026-11-09. It runs next, before A9. From G1 on, no new work goes into
the Leptos pages: a page still to build, such as the rest of W, is built in
the new client. A12 and A13 are about the new client, not Tauri.

**Steps:**

- **G0, spike N14: pick the toolkit by numbers.** Done (2026-11-09): the
  Jobs page with 200 jobs, the Workflows canvas and a 2,000-line diff, built
  on GTK 4 and on `iced`, measured in `docs/levels.md`. The owner chose
  iced and dropped the baseline against today's app.
- **G1, the shell.** The `bastion-desktop` crate: the window, the sidebar,
  D4's theme as one style module, the server list in the Keychain with `local` first
  (D30), starting the `local` server when nothing listens, an HTTP client
  driven by the route table in `bastion-protocol`, and one event stream per
  server with `Last-Event-ID` (D29). The server picker for the pages about
  one server, and the quit choice (D13).
- **G2, the pages.** Each page, with the same layout and controls:
  - Jobs, New request (with the workflow picker) and the job page, with
    its steps, its workflow drawing, its results and the diff with line
    comments. The diff draws only its visible lines, as N14 found 2,000
    text widgets slow to start.
  - Workflows: the canvas, the palette, the step detail, copy and delete,
    read-only for a person who is not an admin.
  - Constraints, Capabilities and the Audit Log.
  - Evals.
  - Engine: the console, its containers, functions and logs.
  - Settings: servers, people, model providers, OpenAI-compatible hosts,
    the factory, oversight, history, workspaces and accounts.
- **G3, parity.** Screenshot every page in both clients against the same
  server, side by side. A difference is a defect unless the owner keeps it.
  Each UI test moves with its page: what a page decides (a job's parts, a
  draft workflow, a host list) stays a plain function with its test, and the
  widgets only show it.
- **G4, cutover.** `src/`, `src-tauri/`, `index.html`, `Trunk.toml` and
  `styles.css` go. CI loses the wasm clippy and the `bastion-ui` tests and
  gains `bastion-desktop`'s. `scripts/check-seams.sh` allows the client
  only `bastion-protocol` among Bastion's crates. README says how to run it.

**Exit, live:** from the new client, the owner adds a server, submits a
request, approves each step of a short workflow, edits a workflow on the
canvas, and sets a provider key. Every page matches its G3 screenshot, and
N14's measures, taken again on the finished client, show first paint under
500 ms, a change painted within two frames, and the canvas and the diff at
the display's frame rate.

#### A9 — Operations (M)

**Goal:** the owner runs the factory day to day with the pages wipp has for it.

- Workspaces, with onboarding for a repository that has no charter: an
  onboard turn drafts the workspace's settings (its checks command among
  them), moved here from A8 (2026-11-06).
- Sessions: every agent session with its transcript.
- Sandboxes: every VM with its job and age, and Stop. A warm pool keeps one VM
  ready per workspace.
- Costs: spend per job, stage, model and day.
- Controls: stop, escape, override, split, pause intake and destroy.

**Exit, live:** onboard a new repository and land a job in it. The Costs page
shows that job's spend by stage.

#### A10 — Learning and verification (M)

**Goal:** the factory learns from its jobs and measures itself, as wipp does.

- Learn: the stages that report surprises (specify, implement, prove) feed the
  learn stage, which proposes constraints.
- Proposals: each comes with an A/B eval (A6). Accept or reject it; an
  accepted proposal enters its ladder at the bottom.
- Verification: the share of shipped jobs with an escape over a window, against
  a threshold the owner sets, and what reviews find that no gate does.

**Exit, live:** a job's surprise becomes a proposal with its eval, the owner
accepts it, and the next job is told the new constraint. The Verification page
shows the escape rate for the jobs so far.

#### A11 — The browser agent (M)

**Goal:** an agent checks its work in a real browser, and the owner sees what
it saw.

- **Spike first:** `bastion-browser` drives Chrome over CDP with
  `chromiumoxide`. A screenshot and a screencast reach `storage`.
- Browser tools for turns: open, click, type, read the page, screenshot, and
  record. Each is a capability on its ladder (A5).
- Screenshots and recordings are artifacts in the stage that made them.
- A pipeline can require a browser check before review.

**Exit, live:** a job changes a page in a test app, and its browser check
records the page. The owner comments on the screenshot, and the next turn
fixes what the comment names.

#### A12 — Notifications and tray (M)

**Goal:** the owner learns that a job needs them without watching the window.

- Notify on `needs_you`, `blocked`, `failed` and `landed`. A click opens the
  job. A notification integration (A7) can send the same to a chat tool.
- The tray shows how many jobs wait on the owner and the stack state, with
  start and stop.
- After a reconnect, the core reads every active job again.
- A setting turns each kind of notification off.

**Exit, live:** a checkpoint raises a notification within 2 s. A click opens
it, the approval moves the job on, and the tray count goes from 1 to 0.

#### A13 — Packaging and updates (M)

**Goal:** the app installs and updates itself on another machine.

- A signed and notarized macOS build for Apple Silicon, and an AppImage and a
  `.deb` for Linux.
- An updater for the native client (D37) with a signed manifest from GitHub
  releases.
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
| The gate's own audit log stopped answering, and every call was refused. | A5: the audit path has a timeout test, and its failure shows in the console. |
| iii upgrades broke pins and config names. | A13: the app refuses a stack it does not know. |

## 10. Later: the owner's box

The app will reach a stack on a box the owner controls, after A13. Nothing here
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
| Jobs stop when the app quits (D18). | The adapter resumes every active job on start, and A12 keeps the app running in the tray. |
| CDP screencasts may be hard to turn into a playable recording. | A11's spike. The fallback is a frame sequence shown as a slideshow. |
| A large diff may be slow to render in the webview. | A4 renders one file at a time, and collapses files over a set size. |
| App checks have no driver on macOS. | Linux CI runs `tauri-driver`. Under D37, a page's decisions are plain functions with tests, and G3's screenshots check the look. |
| `iced` is before 1.0, and a minor version can break its API. | Pin it exactly, and move versions as a step of its own. |
| `iced` 0.14 has no screen-reader support: no crate in it uses `accesskit`, so the labels and the skip link the Leptos pages had have no equivalent. Text input (IME) works. | The owner chose iced knowing this (2026-11-09). Move to an iced release with `accesskit` when one ships, as its own step. |
| A rewrite of 4,674 lines of pages can drift from the old look and behavior. | G3 compares every page side by side, and the Leptos client stays until G4. |

## 12. Progress

| Phase | State | Evidence |
|---|---|---|
| A0 | done | N1 and N3 to N6 pass (2026-10-30). N2 is dropped (D18). Details in `docs/levels.md`. |
| A1 | done | All six steps are in: the seams (424c69b), profiles (c62f8e2), Doctor (ea62b16, 763a79a), the stack routine (ac36f24), the shell (8f6e6b5), and CI and the README (0351c35). 19 tests pass, and CI passes on macOS and Linux. Live exit (2026-10-30), driven in the window: from a stopped machine, Start ran Doctor and brought up all 24 containers, and the page showed the link up with 378 functions by push. Doctor named the missing `ANTHROPIC_API_KEY` with its fix. Closing the window asked the quit question, and "Keep it running" quit the app with the stack up. The next start attached to the same compose daemon (pid 89588) and started nothing. |
| A2 | done | The console on the Engine page: containers with versions, state, stop, start and live logs; functions and trigger types with a filter; queues with depth, delivered, failed and dead letters. One `console` read through the Engine seam, 23 tests. Live exit (2026-10-30): `judge` stopped and started from the window, a stop made from the CLI reached the page by push with no reload, and its logs streamed. `ade` was removed with `compose::remove` (23 containers), so nothing in the stack serves HTTP. |
| A3 | done | `bastion-core` (9caf363), the Store (93b5cb8), spike N7 (c640769), the factory loop (88f45fb), and the first views (778016d), with fixes from the exit. 28 app tests and 15 core tests pass. Live exit (2026-10-30), driven in the window: two workspaces added through the form; three requests in them reached the approve checkpoint with plan artifacts, three plan turns on gpt-5-mini; a comment on the `subtract` plan (add a docstring, rename the test) came back in the next plan; `multiply` was stopped and its VM went away; the app was quit with the stack kept running and reopened, and both other jobs were still at approve; `divide` was then approved, ran its build turn, and reached review with its artifact. The owner then moved the views from a floor and an inbox to a Jobs page and a job page, after GitHub Actions and wipp (2026-10-30): the Jobs page groups waiting for you, in progress and finished, with a workspace filter and search; the job page shows the badges, a waiting panel with the artifact it judges, and the pipeline as steps whose states come from `bastion_core::steps`; workspaces moved to Settings. Checked in the window on the live jobs. |
| A4 | done | The diff, captured from the VM after each build and shown per file with line comments (e5da6d7). The checkpoint dial: ask, hold and auto, review never below hold, holds resumed after a restart (0bdb745). Turn guide and uncommitted-work flag (4b46dca). Landing fixes: the bundle names the branch, a failed land waits on the owner with its VM, and the land passes `target_branch` (75adbe2, f2898c6). 24 core tests, 29 app tests. Live exit (2026-10-31), driven in the window: on `divide`, line comments on `calc.py:5` and `test_calc.py:3` came back verbatim and each next diff answered them; on `subtract`, the owner approved the plan and the review, the land failed once on `target_checked_out_dirty` (an untracked `__pycache__` in the target) and waited, and Approve retried it: `demo-a` main got `0e30e2f`, both tests pass, and the VM is gone. Not run live: a hold expiring (30 min); the core and loop tests cover it. |
| A5 | done | The gate, the ladders and the audit chain in `bastion-core` (def679b); the governance tables, the hook and the owner's commands (a509bb1); held calls and the Governance page (b4f2704). First live exit on `j4825765c` (ee4435e): a plan's write held, Allow once ran exactly it, Refuse reached the agent, `keep-tests` passed a `git rm` at checked and refused it at enforced, and verification failed after one byte of entry 20 changed. Then, from what that exit found: the review page freeze (513da6c); a keyed chain, its writer's state with a 5 s limit per write, and export (d0d3a27); charter constraints and repository hooks from `.claude/settings.json` (37a4b8e); a `tests-kept` constraint on the review diff (c7e992b); Stop during a busy turn and a single send-back (2b9afae); the add form and each constraint's history (4855dca). 45 core tests, 49 app tests, 2 view tests. Second live exit (2026-10-31) on `j3e0ca30c`, with a charter in `demo-a` (deny `Bash(git push:*)`, a PreToolUse hook that refuses `rm`, a Stop hook that runs pytest): the job page showed the charter, its rule and every hook run; the hook passed 12 calls with exit 0 and refused `rm -f /work/repo/notes.txt` with exit 2 and its own message; the Stop hook ran pytest after each build; with `tests-kept` enforced, a build that dropped `test_add` went back to build with the rule, and at checked the same finding waited for the owner. On the Governance page, the form added `no-curl` (entry 333), History listed `tests-kept`'s promotion and demotion, the chain verified 332 and then 335 entries under the Keychain key, and Export wrote 335 entries and `{"verified":true}`. Not run live: the `git push` deny rule (the agent read it in its brief and never made the call; the core and gate tests cover it), and a stalled audit (the unit test covers it). |
| A6 | done | Spike N9: `judge::evaluate` has no provider worker in this stack, so a run is graded by a harness turn with no tools; `harness::metrics` meters each session (docs/levels.md). Build (38519c5): `Dials::trial` in the core (a trial approves its own plan and stops at review with its diff, so nothing lands), the core's `eval` module (arms, tally from the audit, totals, differences, grade), an arm's rungs applied per trial job in the gate, every turn of every job metered into `turns`, the eval tables, and the Evals page. 51 core tests, 58 app tests. Live exit (2026-10-31), eval `ee8eaaddd` on `demo-a` from `d6227b2`: three requests that each ask for a `wc` count, A with `no-wc` told and B with it enforced. All six runs reached review and were graded. In total B differs from A: refused 0 -> 5, review findings 2 -> 0, turns 39 -> 28, calls 33 -> 17, cost $0.0344 -> $0.0207; grades 7 and 7. Each request lists B's refused calls with their audit entries (#375, #405, #363, #452, ...) and links to the trial job, which shows it stopped at review with Land pending. Apply B set `no-wc` to enforced and the audit recorded `ee8eaaddd arm b: Told -> Enforced` (#487) and the eval the owner acted on (#488). Not varied yet: the model per stage and the pipeline, since there is one of each until A7 and A8. |
| A7 | done | Built (3605253): the GitHub forge (push `bastion/<job>`, open or reuse the pull request, poll its checks, a bounded CI fix turn with the failing log, merge on Approve), issues as requests that link back and close on merge, versioned settings with Restore, oversight presets, a model per stage from `llm-router`, and the Accounts list; the governance pages split into Constraints, Capabilities and Audit Log with Lucide icons (dbe1cf7). 57 core tests, 66 app tests. Live exit (2026-10-31) on `ianparo/bastion` (private, CI runs pytest and needs a CHANGELOG.md entry): job `j7eb804a7` started from issue #1 and links to it; it opened pull request #2 (`bastion/j7eb804a7`, body `Closes #1`); CI failed on the changelog, Bastion sent the fix turn with the CI log (ci_fixes 1), the fixed change was pushed to the same pull request, and CI passed. Settings: forge set to GitHub (v1), the medium preset set both dials to hold (v2), Restore of v1 wrote v3 with the dials back at ask, and the next job (`ja0b04eb6`) waited at approve with no hold timer. Approve at land then merged pull request #2 into `main` (2beb204) and issue #1 closed. Not run live: a fix turn that fixes CI by itself (see below). |
| A8 | done | Spike N11 (20a9b71): the `configuration` worker can carry config as YAML, but the owner chose the database (D26). The Constraints and Capabilities pages each show charter, harness and factory sections (D25, 062cd3e), checked in the window. A7's exit was checked again: pull request #2 merged (2beb204), issue #1 closed, the job's branch deleted, CI green on `main`. D26 (7e22bc3): the defaults moved into `seed/governance.sql`, and the gate reads the database again before each turn. 58 core tests, 67 app tests. Step 1, the graph in the core: every full-pipeline stage with its kind, the full pipeline with and without acceptance tests, a dial per checkpoint with wipp's presets and merge held at least, steps the factory runs (`RunStep`, `StepPassed`, `StepFailed`), the revise loop with wipp's limits and the stop on a repeated failure, and one implement turn per task. Checkpoints stay their own stages (the owner's choice, 2026-11-06), and the agent's review turn is `code_review`. 75 core tests, 67 app tests (2881686). Step 2, shape: scope and discover briefs; discovery's reply read into tasks and questions, and clarify passed when there are none; the clarify checkpoint shows discovery's report and takes the answers as a note; read-only turns tag the VM's `HEAD` first and are reset to it after, so a commit made in one goes too; prepare passes at once (no setup steps until A9); `job_submit` takes a pipeline. The store gained numbered migrations on `user_version`: migration 1 rebuilds `jobs`, `artifacts` and `capabilities` with a stage check from `Stage::ALL` and the new job columns, and adds `tasks` and `revisions` tables (D8). The rebuild keeps the table's name, because with foreign keys on (which the worker may set; not checked), renaming a new table into place fails at commit; both were tried on a copy of `data/bastion.db`, and the generated migration kept its 15 jobs and 83 artifacts. Not yet run through the `database` worker itself. Scope's split verdict is not read until step 6. 79 core tests, 69 app tests (0971e08). Step 3, acceptance tests: define, your checks (approve keeps every criterion, a note redefines, up to 3 times), specify, baseline, falsify, and verify and acceptance as steps the factory runs. The app reads specify's JSON (`src-tauri/src/spec.rs`, so the core stays on `serde` and `sha2`, D19); a reply that does not read is `Outcome::Unusable` and gets one more try. The baseline commits the spec's files, runs each check in the VM (exit 0 is green; 126 and 127 did not run), puts back tracked files the checks wrote, and passes only when every check fails. Checks that pass already go to falsify once per spec: the turn breaks the product code from a tag made before it, the checks run against it, the tree resets to the tag, and a check that stayed green, or a test the turn changed, sends the spec back. A passing baseline tags the tree `bastion-spec`; verify waits on the owner when a spec file differs from the tag, or a check cannot run, and sends an unmet criterion back to implement. Not ported from wipp: layers, Gherkin, lint on the tests, property tests, critical models and strength; they need repository settings Bastion does not have. The tag lives in the VM, so revive (step 5) needs another way. 86 core tests, 73 app tests (288c863). Paused for S; steps 4 to 7 land in the server. Step 4, build: a workspace has a checks command (Settings), and the checks step runs it in the VM, sending a failure back to implement with its output and waiting on the owner when it cannot run (exit 126 or 127); provisioning tags the VM `bastion-base`, and the refactor and code review turns diff against it; each replies with a `REFACTORS:` or `FINDINGS:` list, and with nothing listed the job passes the pick; the pick pages show the report, and the findings pick the diff; the owner picks by note, as the core already sends a pick to implement and back. Not run live: the build stages need ship (step 5) to finish a job. 90 core, 79 server tests (5c18ddf). Step 5a, ship to learn: ship brings the VM's commits back to the worktree, then with the GitHub forge pushes and opens or reuses the pull request, and with the local forge goes on, passing CI, which has nothing to watch; a CI failure goes through CI fix and ship again; prove ends with a `VERDICT:` line, yes going on, no sending the work back to implement once, anything else waiting on the owner; comprehend writes the briefing UAT and merge show; merge merges the pull request, or with the local forge lands into the workspace's branch here; learn ends the job. Deploy passes, because nothing watches CD yet. Not run live. 92 core, 80 server tests (ebcc75f). Step 5b, revive: after each turn that writes code (build, implement, CI fix, specify) its commits come back to the worktree; when the sandbox answers `sandbox not found` at a turn's start, a step or a land, the core gets `VmLost`, and the job gets a new VM on the same worktree, which re-enters its stage; a VM lost more than twice in a row fails the job. Provisioning tags `bastion-base` at the fork from the workspace's branch, so a revived VM still diffs against where the job began. Not kept across a revive: work the turn left uncommitted, and the `bastion-spec` tag, so verify after a revive waits on the owner. Onboarding moved to A9. 93 core, 81 server tests. Step 6, split: scope ends its report with `PARTS:`; two or more parts keep the job on the first, and each later part becomes a queued job of the same request, requester and workflow, with its `parent`, the part it waits `after`, and its `part`, which every brief names. Admission holds a part until the part before it lands, and a part is not split again. The Jobs page lists parts under their job, and the job page names the part and its parent. Not run live: the full pipeline needs its grants (the exit). A part whose predecessor fails or stops stays queued until a person stops it. 111 core, 103 server, 10 view tests. Step 7: New request picks a pipeline or workflow (W3), and each step's result is its artifact on the job page (O2); the dial per checkpoint became O. The full pipeline's grants are seeded (15d58da): its read-only turns may read and run commands and not write, and implement, specify, CI fix and falsify may use the whole sandbox. Exit, live (2026-11-07), on `bastion-gh` (`ianparo/bastion`) with the GitHub forge and every step lights-on: `jb1cf52a9`, "Add a modulo(a, b) function to calc.py…", ran all 20 steps with a person's approval at each, each leaving an artifact; scope named one part; the baseline's checks failed before the build and verify passed after it; ship opened pull request #3, its CI passed, and merge merged it (6c602f2). Found on the way and fixed: discover runs before specify, so its task asked for the tests specify had written, and implement rewrote the approved test file each time, so verify stopped (`j0781345c`, `je9da2a54`); implement's brief now says the approved tests are fixed (f6d7c0e), and, as the model ignored that, the gate refuses a write to an approved test once the baseline passes (an enforced rule per spec file, 76e4c1b). Open: plan, told the `commit-work` rule, tried to write a test file and was held on its `ask` grant (refused with a note); an agent ran git outside `/work/repo` once; the protection lives in memory, so a server restart after the baseline drops it, though verify still catches a change. |
| P | done | Added 2026-11-06 (D31, D32). Runs before A8 step 6. P1 to P3: `bastion_core::people` holds the roles and what each may do; `people` and `tokens` tables keep each person and each token's SHA-256, never the token; a server's first start makes its owner (`--owner`, else `git config user.name`) an admin, and the token in `data/token` is the owner's even before the database opens. Every request resolves its token to a person, and `access` checks the role against what the route does: reads for all, submitting for members and admins, acting on a job for its requester or an admin, everything else for admins; a refusal is 403 with who and why. A job keeps its requester, the Jobs page shows it, and every successful change goes in the audit log as `act` with who made it. `GET /me` says who you are. 94 core, 86 server tests (d6b453a). P4: routes to list people, add one, change a role, issue a token (returned once, kept as its hash) and list or revoke tokens, with listing tokens kept to admins; the server strips from each job the actions the asker may not take, so the job page shows only what they may do; Settings shows who you are on the Bastion server, and an admin gets a People section with roles, Add, Issue a token and Revoke. Not run live. 94 core, 87 server tests. P5: `--listen IP` with `--tls-cert` and `--tls-key` (PEM) serves HTTPS through rustls on ring, already in the tree for reqwest; any address but loopback without both is refused at start, and the page's CSP allows `https:`. A handshake that fails or takes over 10 s is logged and dropped. Live on a self-signed certificate: HTTPS with the owner's token gives `/me`, no token gives 401, plain HTTP and an untrusted certificate fail at the handshake, and the server goes on serving. 94 core, 90 server tests. Exit, live, on `local`: the admin added mia (member) and vic (viewer) and issued their tokens; mia's request ran to `approve` and mia approved it; vic saw the job with no actions, and the server refused vic's approve and submit with 403; mia's constraint move got 403, and the admin moved `no-wc` down and back up. The audit log now keeps every 403 as `refused` with who tried, next to each `act`, so it names all three. 94 core, 91 server tests. Next: K. |
| K | done | Added 2026-11-06 (D34). K1, spike N13, passes (docs/levels.md): an Anthropic key imported from the login shell into the `secrets` vault, readable only by `llm-router`, and referenced as `secret://ANTHROPIC_API_KEY` in the router's entry, ran a plan turn with no key in the stack's environment. `providers.openai.api_url` reaches one Chat Completions host in place of OpenAI, embeddings included, so K3 still needs its own worker. K2: `GET /providers` lists each provider and where its key comes from (`secret`, `env` or `none`), never the key; `PUT` and `DELETE /providers/{id}/key` keep a key in the vault as `<ID>_API_KEY`, readable only by `llm-router`, and set or clear `secret://` in the router's entry, keeping the rest of the entry; an unknown provider stores nothing. Settings has a Model providers section with a field and Set per provider, and Remove for a key set there; Accounts no longer lists providers. Doctor passes when any provider has a key and fails only when none has. The stack gained `provider-deepseek` 0.1.24, `provider-xai` 1.3.23, `provider-zai` 0.5.18, `provider-kimi` 1.1.19 and `provider-openrouter` 0.1.16, each the newest in the train of the pinned router; the registry has no Google, Mistral, Groq, Together, Fireworks, Ollama or Qwen worker. Live: all seven providers ready, Anthropic's vault key kept across a stack restart, a dummy xAI key set and removed through the API. 94 core, 95 server tests. Then the subscriptions: `provider-openai-codex` 0.4.30 and `provider-claude-code` 0.1.25 (0.2.x needs iii 0.26, and this stack runs 0.25). `llm-router` reports both as having no credential, so the server asks each itself and lists it as `signed_in`: Codex by its `auth::status`, Claude Code by whether its catalog has models. OpenAI Codex signs in with a ChatGPT plan from Settings, by a device code the page polls until it ends (`/providers/openai-codex/sign-in`, `/sign-in/poll`, `/sign-out`), and the worker reads its models again after. Claude Code reads the sign-in of `claude` on the stack's machine (Keychain), so Settings reports it and says how to sign in there. A key for either is refused. The stage model pickers offer every provider with a key or a sign-in. Live: Claude Code signed in through the Keychain with 12 models offered; the Codex sign-in returned a link and a code. 97 server tests. A plan turn on `claude-code/claude-haiku-5-5` reached Approve. K3 is deferred until after O, W and A8, built in full then; K4 is met by the stage pickers offering every provider with a key or a sign-in. Found live: a provider reads its models only at start, so a new key offered no models; setting or removing a key now asks `provider::<id>::refresh_models`. Exit, live (2026-11-06): with no Anthropic or DeepSeek key in the stack's environment, both keys went to the vault through `secrets::import` from the login shell (the same vault entry and router reference the Settings route writes, which a dummy xAI key exercised end to end), and plan turns on `claude-haiku-5-5` (Anthropic), `deepseek-flash` (DeepSeek) and `claude-code/claude-haiku-5-5` (Claude Code through the Keychain) each reached Approve. Not shown: a key typed into the Settings page itself, and a Codex sign-in, as the owner has no Codex account. OpenAI and Z.AI keys are still in the stack's environment. 97 server tests. K is done except K3, deferred. Then D36 (2026-11-06): the Claude Code sign-in is removed, with its worker, its Settings row and its server check, because Anthropic's terms do not permit it in a third-party product. K3 (2026-11-09): `workers/bastion-openai-compat`, a Rust worker on `iii-sdk` 0.25.0 that compose starts with `cargo run`, makes each host the admin lists (an id, a name, a full `/chat/completions` URL) a router provider of its own. It writes llm-router 1.4.41's provider contract by hand, as no router crate pins iii-sdk 0.25: `provider::<id>::{stream,abort,refresh_models,on_router_ready}`, a `router::provider::register` with its token kept in `state`, a catalog through `router::models::reconcile` with ids `<id>/<model>`, and frames on the router's channel. A host is declared `credential_optional`, so a local host needs no key (the router reports it `configured`, and the server now offers a provider's models when it is `ready`, not only when it has a key); a host's key is set under Model providers like any other. Settings has an OpenAI-compatible hosts section (`GET` and `PUT /providers/hosts`, admin only). A host may not take a provider's id. The router has no call to drop a provider, so a removed host is retired: its catalog empties, it is declared needing a key, the server leaves it out of Model providers, and its id can be added again. Live, against a local Chat Completions host in the scratchpad: the host registered with 1 model and no `Authorization` header; a job's plan turn on `localfake/echo-1` reached Approve with the host's text as its artifact; a dummy key set through the API reached the host as a bearer token and stopped after Remove; a host added and removed in the Settings page lost its models, and was added again. Not shown: compose starting the worker (it ran by hand beside the running stack), a real Together, Fireworks, DashScope or Ollama host, and a tool call round trip on a live host (the mapping is tested). Open: the host answers whole (`stream: false`), so a turn's text arrives as one delta, with a ping every 25 s while it waits; every model gets a 128,000 context window; images and an orphaned tool call are dropped. 7 worker, 107 server, 11 UI tests. K is done. |
| O | done | Added 2026-11-06 (D33). Takes over A8 step 7's dial per checkpoint. O1 (9c52028): a step is lights-on or dark; the hold timer is gone; a workflow is lights-on when every step has a person's gate, dim when one up to the merge does, dark when none does; the checkpoints fold into the steps they judged; ship, land, merge and deploy wait before they act; stored jobs and settings map onto the steps. O2 (b176981): a step the factory runs keeps what it reported as its artifact; every decision is a `decide` record, a person's with their name and the step, a dark step's as its dial's. O3 (22331e0): Settings shows each pipeline's steps, each a lights-on or dark button, and the level they make; `PUT /dials/{stage}` sets one; the presets are the three levels. Found live and fixed: `turns` still checked for the short pipeline's stages, so full-pipeline turns kept no usage (migration 2); dark decisions logged out of order. Exit, live (2026-11-06), on the short pipeline of `demo-local` with the local forge: with every step lights-on, `j459f405e` waited at plan, build and land and landed after three approvals; with every step dark, `j5ba1f381` and `j2ab6822e` landed with no person, `multiply`, `subtract` and `divide` are in `calc.py`; the audit log has `plan: approved by Ian Johnson` and the like for the first, and `plan`, `build`, `land: approved by its dark dial`, in order, for the last. Jobs kept at approve and review before O wait on plan and build. Not shown: the full pipeline, as this server grants `sandbox::*` only at plan and build, so scope's calls were refused; the full pipeline's grants are A8's. `eval_arms` still checks for the short stages, so an eval of a capability at a full stage would be refused. 203 tests. |
| W | in progress | Added 2026-11-06 (D35). Builds on O's steps and dials. W1 (a0cf9e9): a workflow is a graph of steps linked by results, kept as versions, with the built-in pipelines as chains. W1b (ef68e86): a step is a skill, a prompt, a command or a Bastion stage. W2 (ebc266d): the Workflows page draws each workflow on a canvas; an admin drags a step, links two from a step's right edge, adds and removes steps and links, opens a step to set what it runs, its dial, note and results, and saves a version; a built-in can only be copied. W3 (917681c): New request picks a workflow, and the job page draws the job's workflow version with each step's state and each result as a document; a workflow job's current step is its node. W4 (0bcbb3d, 85a1bcf): eleven seeded workflows. Live (2026-11-06), in the page against the local server: the database seeded the eleven; Short was copied as `Short with design notes`, plan given a `design-notes` result and a note, its link to build removed and drawn again (which found and fixed a bug: the link's preview took the pointer's release), and version 2 saved; `j73499b5a` ran on it from New request, plan kept `design-notes`, build's brief carried it as `<result name="design-notes">`, and the job page showed it as a document. Not shown: removing a step, because Short has no step to spare and the full pipelines need A8's grants; a viewer's read-only view, because `local` has only its owner; the land, which waits on this server's GitHub forge and `demo-local` has no `origin` remote. The window's webview stayed blank to screen capture, so the page was driven in Chrome with the window's server. |
| S | in progress | Added 2026-11-06 (D27 to D30). Runs before A8 step 4. S0, spike N12, passes (docs/levels.md): the page follows a server's events over `fetch` with the token, reconnects with `Last-Event-ID` after a server restart, and a second stack runs beside the first with its own data. The window's CSP gained `connect-src` for `127.0.0.1`. S1, the split: `crates/bastion-server` holds the engine adapter, the store, the factory, the gate, the forge, evals, settings, the audit log, charter, spec, Doctor, the stack and the console; the app links it as a library and keeps its commands, profiles and the window. `tauri::async_runtime` became `bastion_server::rt`, one `tokio` runtime; the fakes sit behind a `fakes` feature. `scripts/check-seams.sh` now fails if the server depends on Tauri or the page on the server. 86 core, 64 server, 9 app tests (2181a86). S2, the API: each command's body moved into `bastion_server::api` (de6861c), and the Tauri commands call it until S3. A `Server` holds the engine, the machine, the stack, a data directory, the factory, and a `Bus` of numbered changes (`Change` in `bastion-protocol`, the latest 256 kept for a reconnect). `bastion_server::http` serves one route per function behind the bearer token, with CORS for the window's origins: 400 for a refused action, 409 for the stack's typed refusal, 503 while the factory waits for the engine, 401 without the token. `GET /events` streams the bus as server-sent events, replays after `Last-Event-ID`, and sends `resync` when it cannot. The `bastion-server` binary takes `--checkout`, `--data`, `--port` and `--engine`, keeps its token in `data/token` (mode 0600), starts or attaches to the stack, and serves on loopback. Live (2026-11-06) against the second stack: the server started the stack, opened the store on the third try once `database` registered, ran migration 1 through the worker (`user_version` 1, the seed's 5 constraints and 3 grants), refused a request with no token, answered `/engine`, `/jobs` and `/constraints`, pushed a change on `/events` when a preset was set, answered a stale `Last-Event-ID` with `resync`, refused a bad preset with 400, and stopped the stack through `/stack/stop`. 86 core, 77 server, 3 app tests (62ea1a5). S3, the view on the API: the page sends each server command over HTTP by a route table in `bastion-protocol` (a server test sends every row and fails on a 404 or 405), with the token from the window's `target` command, and follows `GET /events` with `fetch`, reconnecting with `Last-Event-ID`; a resync, a reconnect or the window's focus makes every page read again, the engine status included. The app keeps three commands (`profile`, `target`, `quit`), no longer links the server, and starts `bastion-server` in its own process group, or attaches to the one on port 49190. The page decides a quit: it asks only while the stack runs, and the server outlives the app. Live (2026-11-06), on the owner's stack after backing up `data/bastion.db`: the app started the local server, migration 1 ran through the `database` worker and kept the 15 jobs and 83 artifacts; the Jobs page showed them and the Engine page the link up; with the server killed the Link card said `Down: http://127.0.0.1:49190 does not answer.`, and after a restart the open page came back up with no reload; a relaunched app attached to the running server instead of starting another. Not confirmed: the quit question (after its close button, screenshots of the window came back blank), and ⌘Q quits without asking, as Tauri's Quit menu item exits with a code; a stale error stays on the stack card after an outage. The sidebar's server label is gone (D30). 86 core, 78 server, 3 app, 3 view, 1 protocol tests (8004bb8). S4, Bastion servers, built: the app keeps the servers the owner added in `servers.toml`, each token in the Keychain (`keyring`), with `local` always first; the page follows every server's events and calls any of them (`call_on`). The Jobs page lists every server's jobs, each row naming its server, and the filter offers each server and workspace that has jobs. New request picks a Bastion server, `local` by default, then one of its workspaces; opening a job makes its server the current one, and the job page names it. The pages about one server have a Bastion server picker at their top, and Settings lists the servers with add and remove. The `profile` command is gone from the page; `profiles.toml` only tells the app how to start `local`. 86 core, 78 server, 5 app, 3 view, 1 protocol tests (fe68813). Exit, live (2026-11-06), with `local` and a second Bastion server on a second stack on this machine: Two was added in Settings, its token went to the Keychain and not to `servers.toml`; New request on `local` with `demo-local` started `j7df1a6c2`, and New request with Two picked showed only Two's workspace and started `jcbf97e51`; the Jobs page listed both under their servers, and each reached Approve with one plan. Closing the window with the stack running asked the quit question (S3's open item), Keep it running quit the app with both servers and both stacks up, and the reopened app listed both jobs where they were. Killing only Two's server left its stack running, and the restarted server attached to it. Found, and open: the Jobs page does not say when a server cannot be reached, and keeps showing its last read; stopping the background task that ran Two's server took its stack down too, and the job waiting at Approve lost its VM (`sandbox not found` at stop), so a job that waits across a stack restart cannot go on; whether a plan turn survives a kill mid-turn is not shown, because the plan finished too fast to tell; the add-server form keeps its values after Add. Then the Jobs page's unreachable server: the page now marks a server whose event stream is down, names it above the list, keeps that server's jobs as last read and marks them unreachable, and still says why a reachable server's jobs did not read; checked live by killing only Two's server, which put up the note, and restarting it, which cleared the note and brought Two's job back with no reload. Next: A8 step 4. |
| G | in progress | Added 2026-11-09 (D37). Runs next, before A9. G0, spike N14 (docs/levels.md): GTK 4 and `iced` measured on the same three screens; the owner chose iced and dropped the baseline. G1 done: `crates/bastion-desktop` on `iced` 0.14.0 is the window, the sidebar with the Lucide icons, D4's theme as `theme.rs`, and the server picker on the pages about one server. It starts or attaches to `local` and reads the added servers from the files and Keychain entries the Tauri app keeps, so both clients share them until G4. A command goes over HTTP by `bastion-protocol`'s route table, and each server's `GET /events` stream is an iced subscription that reconnects with `Last-Event-ID`. Live: the window attached to the running `local` server, listed `local` up and the added server `Two` down (nothing listens on 49191), showed a push from a workflow copy, and showed `local` down and up again across a server restart. Closing the window while the stack ran asked the quit question; with the stack stopped, it quit at once. The client's theme is a custom iced `Theme` from D4's tokens, so a widget with no style of its own reads light on dark, and its face is Helvetica Neue, as iced draws no bold from macOS's variable system face. `scripts/check-seams.sh` allows the client no Bastion crate but `bastion-protocol`. Found: iced 0.14 has no screen-reader support (§11). 9 client tests. G2 (2026-11-09), first pages: Jobs (every server's jobs, waiting first, the workspace filter and search, a split job's parts under it, a note per server that cannot be read), New request (server, workspace and workflow pickers, the request, and a start from a GitHub issue), and the job page (its badges, the failure, the wait with what Approve does, the report, the diff with a comment per line, the held calls, the charter, the workflow drawn on an iced canvas with each step's state, the results, and the steps with their artifacts and latest turn). A page reads again on each server push. The diff draws every line for now. Live, once the stack ran again: the Jobs page listed 34 jobs, three waiting; a waiting job opened with its report, the note and Approve; `j73499b5a` showed its diff by line number, its workflow with each step's mark and its `design-notes` result, and its land artifact. Found live: a page kept the scroll of the one before it, so each page now starts at its top; and reading an added server's token from the Keychain took 7.5 s, which held the first page, so the window now shows `local` at once and adds the others when their tokens are read. Then Constraints (the charter's rules, the harness and factory ladders with history, promote and demote, and the form to add one), Capabilities (the charter's allows, the grants per stage, the factory's own), and the Audit Log (the chain's check, the writer's state, every entry, and Export), each seen live. Found live: the Keychain asks the person before a newly built `bastion-desktop` reads an added server's token, as its item trusts the Tauri app, so each rebuild asks again; and the Audit Log showed the audit writer failing with `database::query` not found since the stack stopped, and no entry for that stop, so a stop that takes the database down is not recorded (a server problem, left open). CodeScene does not count `draw`'s arguments in `pages/drawing.rs`, as iced's `canvas::Program` fixes them. 21 client tests. |
| A9 to A13 | — | — |
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
- Each turn runs on gpt-5-mini through OpenAI until A9, because the stack has
  no Anthropic credential yet.
- `harness::status` truncates `result` and encodes it as a JSON string, so a
  turn's report comes from its last assistant message in `session::messages`.
- The bundle folder in the app's cache must exist before `git bundle create`.
- The factory starts after the page loads, so it announces itself with an
  empty `job-changed`, and every page reads again.
- Page reads overlap when pushes come fast, so each page keeps only its latest
  read. An artifact is written before the page hears of a change, so a panel
  that mounts on that change sees it.

**What A4 found:**

- An agent's report can claim work its commits do not hold. The diff from the
  VM is the ground truth, and work left uncommitted is listed at review.
- gpt-5-mini sometimes ended a turn by announcing its next step, ran tests
  through `bash -lc` (which hung the VM call), and passed no timeout, so one
  turn stalled for about 25 minutes. `harness` has no way to stop a turn. The
  turn guide now covers all three, and A5's constraints should enforce them:
  commit every change, keep existing tests, leave a clean tree.
- `git bundle create … HEAD` names no branch, so the pull found nothing. The
  bundle names the branch now.
- `worktree::land` needs `target_branch`, refuses a dirty target, and reports
  through `worktree::land-blocked` with a `reason`.
- A hidden webview can miss pushes, so the window reloads every page when it
  regains focus.

**What A5 found:**

- A `pre-trigger` hook answers `continue`, `deny` with a `reason`, or `hold`.
  The deny reason reaches the turn as the call's result, word for word (N8).
- A held call arrives without the hook's `sandbox_id` pin. On approval the app
  runs the call itself in the job's VM and sends the result back with
  `harness::function::resolve` (`action: "deliver"`).
- `harness::stop` exists, so the turn cap can stop a turn, which A4 thought it
  could not.
- `bastion-core` takes `sha2` for the audit chain. The hash is part of the
  rule, so it stays pure and tested with the rest (D19 updated).
- The first rule matched a plain substring, so `keep-tests` ("git rm") never
  bound `git -C /work/repo rm`. A rule's words now bind in order with any
  words between them (`mentions` in `bastion-core`).
- A rule on a command line cannot see a deleted test. One build rewrote
  `test_calc.py` with `sandbox::fs::write` and dropped `test_add`, and no rule
  bound that call; the owner caught it in the diff. Keeping tests needs a rule
  on the diff at review, not on a call.
- A refusal at plan does not carry into build. The owner refused `RISKS.txt`
  at plan, and the build wrote it anyway, because writes are allowed at build.
- A closed page's push handler read its disposed `StoredValue` and panicked,
  so open pages stopped updating on push. `ipc::listen` now stops a handler
  when its page closes.
- Typing a comma into the review note froze the webview at 100% CPU. A
  sample showed the main thread answering `AXVisibleCharacterRange`, which
  WebKit measures across the whole page; macOS text checking asks for it after
  punctuation. `spellcheck="false"` on the note boxes fixed it. The layout was
  not the cause, though the page did scroll sideways, which is also fixed.
- `iii-sandbox` passes the shell's whole environment, secrets included, into
  worker VMs such as `opengantry` as `--env` flags. A9 starts the stack with
  only the variables it needs.
- A rule on a call cannot see a test deleted by rewriting its file. A
  constraint on the review diff (`tests-kept`) can. An enforced finding sends
  the work back once: live, eight builds in a row restored `test_add` under a
  new name until the job was stopped.
- Every hook run pushes a change, and the job page rebuilt its header on each
  one, so a click on Stop during a busy turn was lost. Stop now sits outside
  the part that re-renders.
- The repository's charter is read at each turn start. A job that has not
  started a turn since the app started shows no charter yet.
- `sandbox::exec` takes `cmd` and `args` as well as `argv`; the gate reads
  all three, or a rule on a command line sees nothing.
- An agent that reads a deny rule in its brief does not try the call, so the
  gate never sees it. A rule told is often a rule obeyed.
- The page shifts under a click when it reloads, and a mis-aimed click
  promoted a grant during the exit. The audit showed the promotion and the
  demotion that undid it. Two-step confirmation on a promotion is worth
  considering in A7.
- Trunk's file watcher died after the disk filled, and a timestamp-only touch
  does not trigger a rebuild. Restart `cargo tauri dev` after a full disk.

**What A6 found:**

- Every rule goes into the brief at every rung, and the agent obeys a rule it
  is told, so told against enforced shows no difference for a rule that
  forbids what the request asks. An eval of the gate itself needs a rule the
  brief advises rather than forbids. Whether a told rule belongs in the brief
  at all is a design question for A10.
- The stack's `judge` worker has no provider worker, so it cannot grade. A
  turn with no tools grades instead. Every run graded 7, so a 0 to 10 grade
  from one turn is coarse; a rubric with named criteria is the next step.
- Under A, two of three builds dropped a test (`tests-kept` found them at
  review); under B none did. Three requests is too few to say B caused that.
- A trial job is an ordinary job, so it shows on the Jobs page, and the
  factory's admission limit counts it.

**What A7 found:**

- The fix turn after CI's first failure changed nothing that fixed it, though
  its brief held the whole failing log, "CHANGELOG.md has no entry" included,
  between a step's environment dump and its timestamps. It took a note from
  the owner to add the line. The log is now cleaned to the failing lines
  before it goes into the brief; whether gpt-5-mini then acts on it is not
  yet measured.
- Approve at land merged pull request #2 (2beb204), the branch was deleted,
  issue #1 closed, and the workspace's `main` pulled the merge.
- The agent asks before acting on an owner's note, and once ignored part of
  one (it moved the tests into `tests/` after being told to keep them at the
  root). `origin/main` does not exist in the VM: the branch it starts from is
  `@{u}`.
- The webview freeze in A5 came back with spellcheck off. A sample showed the
  same `AXVisibleCharacterRange` walk; the trigger is the test driver, which
  reads that attribute after scrolls and typing, not macOS text checking.
  Driving with keys (Page Down, End) avoided it. Whether VoiceOver does the
  same has not been checked.
- A settings change pushed nothing, so the other sections of the page kept
  the old values; each settings command now pushes a change.
- The `Approve` and `Review` labels on the Jobs list are badges, not buttons,
  and read as buttons.
