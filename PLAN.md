# Bastion plan

Bastion is a native software factory on the iii engine. A request goes in, a
reviewed change comes out, and nothing merges itself. It is one app: a Tauri 2
window with a Leptos frontend and a Rust core. Agents run on iii workers,
against a local engine that the app starts. A remote engine on the owner's own
box comes later (§10). Written 2026-10-30, revised the same day.

The owner works at the level of the factory. The first view is the **floor**:
every job, the stage it is in, and the decisions waiting on the owner. Agents
and their transcripts sit one level down, for when a job needs a closer look.
Antigravity starts at the agent; Bastion starts at the job. wipp is the model
for the pipeline and its checkpoints.

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

## 2. Non-goals

- Not an editor or a file browser. The diff artifact is the only view of code.
  "Open in editor" hands a worktree path to the owner's own editor.
- Not an agent chat. The owner talks to a job through its artifacts and
  checkpoints, not through a prompt box per agent.
- Not a browser app, and no HTTP API.
- Not multi-user. One owner, on their own machine. No sign-in.
- Not a new agent runtime. Turns run on the stock `harness`, `llm-router` and
  `iii-sandbox` workers.
- Not hosted. wipp runs on Fly for a GitHub org. Bastion runs on the owner's
  machine.
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
| D10 | Live updates are push. The core observes `bastion::*::changed` and emits a Tauri event `{kind, id, revision}`, and the page reads the resource again. | wipp polled. Push needs no timers and no stale views. |
| D11 | One owner, no sign-in. The actor on every record is `git config user.name`. | One person on one machine needs no accounts. |
| D12 | Local is the default and only mode for now. The engine address is a profile field, never a constant. | The remote box (§10) becomes a new profile kind, not a rewrite. |
| D13 | The app starts the stack with `iii compose --up -f worker-compose.yaml`, and attaches to a stack that already runs. On quit, the owner chooses whether it keeps running. | The owner's choice. No terminal. |
| D14 | Every agent turn runs in an `iii-sandbox` VM on its job's worktree. A credential never enters the VM. Pushes and PRs happen on the host. | wipp's lesson: a sprite that saw a credential is tainted. |
| D15 | Merge is a person's decision, or `hold` at the lowest. Every other checkpoint has a dial: `ask`, `hold` (default after 30 min) or `auto`. | wipp's oversight model. |
| D16 | A comment on an artifact goes to the agent verbatim, as its next brief. A fix loop stops after a set count, or sooner when a refusal repeats word for word. | wipp's bounded loops. Paraphrased feedback loses the point. |
| D17 | Every job runs a pipeline. The smallest pipeline is plan, approve, build, review, land, and it is the default until A5 adds the rest. | The factory is the unit. A free-form task is a short pipeline, not a second mode. |

## 4. The repository

```
Cargo.toml  index.html  Trunk.toml  styles.css
src/                  bastion-ui: the Leptos frontend (WASM)
src-tauri/            bastion: Tauri, the Rust core
  src/engine.rs       the iii-sdk link
  src/commands/       one file per resource (D9)
  src/stack.rs        iii compose as a child process
  src/doctor.rs
crates/
  bastion-core/       shared types: job, stage, artifact, pipeline, checkpoint
  bastion-spikes/     spikes and live checks, as examples
workers/
  bastion-factory/    jobs, stages, artifacts, comments, checkpoints
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
| Jobs, stages, artifacts, comments, checkpoints | worker | `bastion-factory` | A3 |
| Running a pipeline | worker | `workflow` (stock), if A3's spike passes | A3 |
| Worktrees and local land | worker | `worktree` (stock) | A3 |
| Agent turns | worker | `harness`, `llm-router`, `iii-sandbox` (stock) | A3 |
| Transcripts | worker | `session-manager` (stock) | A3 |
| Diffs | worker: computed in the job's worktree | `bastion-factory` | A4 |
| Approvals | worker | `approval-gate` (stock) | A4 |
| Push, PRs, CI watch, merge | worker | `github` (stock) | A4 |
| Browser | worker | `bastion-browser` | A6 |
| Notifications and tray | app | — | A7 |

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

## 7. Phases

Each phase uses paired pacing and stops at its exit evidence, run live after
its last change. A phase that rests on an unproven engine feature starts with
a spike, and a failed spike stops the phase with its fallback named. Sizes: S,
M, L.

### A0 — Spikes (M)

**Goal:** prove that Rust on `iii-sdk` 0.25.0 and Tauri can do what the rest of
the plan assumes.

- **N1:** a Rust worker registers a function, triggers one, observes a
  `changed` function, and calls `database::query` and `database::transaction`.
- **N2:** it enqueues with `TriggerAction::Enqueue` on `bastion-steps`, and
  sends with `Void`.
- **N3:** it binds `harness::turn-completed` for one session, and a
  `harness::hook::pre-trigger` that rewrites `sandbox_id`.
- **N4:** it reads a `sandbox::fs::read` content channel.
- **N5:** compose starts a Rust binary through `scripts.run`, and restarts it.
- **N6:** a Tauri 2 window shows a Leptos page in the dark purple theme. An
  `invoke` round trip works, and the core emits an event the page receives.

**Exit:** N1 to N6 pass, and N3 passes three runs in a row. Each result goes in
`docs/levels.md` with the version of each part.

### A1 — The shell and the stack (M)

**Goal:** the app starts, checks, watches and stops the stack.

- The shell: a sidebar with Floor, Inbox, Engine and Settings, a skip link,
  and the dark theme.
- The engine link, with reconnect and backoff. The `this machine` profile, in
  TOML in the OS config directory, with no secret (D12).
- Doctor: `iii` and its version, the pins in `worker-compose.yaml`, provider
  keys, `gh` login, and sandbox support. Each failed check names its fix.
- The stack routine: Start runs Doctor and stops on a failed required check,
  then runs `iii compose --up`. Stop runs `iii kill --compose`. A stack that
  already runs is attached, not started twice. On quit, the owner chooses.
- CI on macOS and Linux: `cargo fmt --check`, `cargo clippy -- -D warnings`,
  `cargo test`.
- Replace the template `README.md`.

**Exit, live:** from a stopped machine, the app starts the stack and shows the
link as up. Doctor names a missing `ANTHROPIC_API_KEY` with its fix. After a
quit with the stack kept running, the next start attaches. CI passes.

### A2 — The engine console (S)

**Goal:** the owner sees the engine without `ade`.

- Workers with their versions and state, from `compose::status`.
- Functions and triggers, from `engine::functions::list`.
- Queues and their depth.
- Live logs for one worker, from `compose::logs`.

**Exit, live:** stop one container from the console, see its state change by
push, start it again, and read its logs. Remove `ade` from the stack if the
owner no longer needs it.

### A3 — The floor (L)

**Goal:** a request becomes a job that runs the short pipeline (D17), and the
owner watches every job from the floor.

- **Spike first:** the stock `workflow` worker (0.1.9) runs a two-stage
  template with a person's checkpoint between them, and the run survives an
  engine restart. If it fails, `bastion-factory` runs stages from
  `bastion-steps` instead, as wipp does.
- `bastion-factory`: jobs, stages, artifacts, comments and checkpoints, as rows
  in `database` (D8), with `bastion::factory::changed` for push (D10).
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
Restart the stack, and the two other jobs carry on from where they were.

### A4 — Review and landing (M)

**Goal:** a job's work lands only after a person decides.

- The diff artifact: `bastion-factory` computes it in the job's worktree
  against its base. The page renders it per file, unified or side by side,
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

### A5 — The full pipeline (L)

**Goal:** a job can run wipp's whole factory.

- The full pipeline, after wipp: scope, clarify, plan, approve; acceptance
  tests first, and the baseline must fail; one build turn per plan item, tests
  first; checks; refactor; review; ship; CI fix; merge; a briefing of what
  changed and why.
- Split a job into parts that depend on each other. The floor shows parts
  under their job.
- Read-only stages get a read-only grant. A changed tree in one is reverted.
- The owner picks a pipeline per request, and sets the dial per checkpoint.

**Exit, live:** a request in this repository runs the full pipeline to a
merged change. Each stage leaves an artifact, the baseline tests fail before
the build and pass after it, and each checkpoint obeys its dial.

### A6 — The browser agent (M)

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

### A7 — Notifications and tray (M)

**Goal:** the owner learns that a job needs them without watching the window.

- Notify on `needs_you`, `blocked`, `failed` and `landed`. A click opens the
  inbox item or the job.
- The tray shows the inbox count and the stack state, with start and stop.
- After a reconnect, the core reads every active job again.
- A setting turns each kind of notification off.

**Exit, live:** a checkpoint raises a notification within 2 s. A click opens
it, the approval moves the job on, and the tray count goes from 1 to 0.

### A8 — Packaging and updates (M)

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
| The gate's own audit log stopped answering, and every call was refused. | A3: the audit path has a timeout test, and its failure shows in the console. |
| iii upgrades broke pins and config names. | A8: the app refuses a stack it does not know. |

## 10. Later: the owner's box

The app will reach a stack on a box the owner controls, after A8. Nothing here
starts until the owner asks.

- **Transport:** an SSH tunnel to the engine port on the box's loopback, with
  the owner's own SSH config and agent. The app stores no SSH key.
- **Profile:** a new `kind = "remote"`, with `ssh_host` and the far port (D12).
- **Spike first:** N1 passes through the tunnel.
- Features in workers need nothing new. The app's core features (Doctor, the
  stack) run on the box over SSH.
- Diffs come from `bastion-factory` on the box, so review needs no file sync.
- The title bar names the profile.
- **Exit, live:** from the laptop, a job on the box lands. The tunnel is killed
  mid-job, the app reconnects, and the job finishes once.

## 11. Risks

| Risk | What settles it |
|---|---|
| N3 passed two of three runs, and the cause of the failure is unknown. | A0's exit needs three passes in a row. |
| `workflow` 0.1.9 is young. | A3's spike, with `bastion-steps` as the fallback. |
| CDP screencasts may be hard to turn into a playable recording. | A6's spike. The fallback is a frame sequence shown as a slideshow. |
| A large diff may be slow to render in the webview. | A4 renders one file at a time, and collapses files over a set size. |
| App checks have no driver on macOS. | Linux CI runs `tauri-driver`. |

## 12. Progress

| Phase | State | Evidence |
|---|---|---|
| A0 | in progress | N1, N2, N4, N5 pass (2026-10-30). N3 passed two of three runs. N6 is built in the app at the root, and its live run has not been recorded. |
| A1 to A8 | — | — |
| Later (§10) | on hold | — |
