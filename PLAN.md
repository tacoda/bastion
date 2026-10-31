# Bastion plan

Bastion is a harness and a factory for agent work, on the iii engine. A
request goes in, a reviewed change comes out, and nothing merges itself. What
Bastion adds is governance you can measure: constraints and capabilities built
into the harness's hooks, a way to promote and demote each one, a
tamper-evident audit record, and A/B evals that show what a change to the
harness does. It is one app: a Tauri 2 window with a Leptos frontend and a Rust
core, against a local engine that the app starts. A remote engine on the
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
| D11 | One owner, no sign-in. The actor on every record is `git config user.name`. The owner's accounts with other services (forge, tracker, providers) are settings, kept in `secrets` (A7). | One person on one machine needs no accounts. |
| D12 | Local is the default and only mode for now. The engine address is a profile field, never a constant. | The remote box (§10) becomes a new profile kind, not a rewrite. |
| D13 | The app starts the stack with `iii compose --up -f worker-compose.yaml`, and attaches to a stack that already runs. On quit, the owner chooses whether it keeps running. | The owner's choice. No terminal. |
| D14 | Every agent turn runs in an `iii-sandbox` VM on its job's worktree. A credential never enters the VM. Pushes and PRs happen on the host. | wipp's lesson: a sprite that saw a credential is tainted. |
| D15 | Merge is a person's decision, or `hold` at the lowest. Every other checkpoint has a dial: `ask`, `hold` (default after 30 min) or `auto`. | wipp's oversight model. |
| D16 | A comment on an artifact goes to the agent verbatim, as its next brief. A fix loop stops after a set count, or sooner when a refusal repeats word for word. | wipp's bounded loops. Paraphrased feedback loses the point. |
| D17 | Every job runs a pipeline. The smallest pipeline is plan, approve, build, review, land, and it is the default until A8 adds the rest. | The factory is the unit. A free-form task is a short pipeline, not a second mode. |
| D18 | All factory logic lives in the app, in `crates/bastion-core`, apart from the view and from iii (§6.1). The core takes an event and the current state, and returns the new state and a list of effects. It does no I/O, has no `async`, and does not depend on `iii-sdk`, Tauri or Leptos. | The owner's choice. The rules are tested as plain functions, with no engine and no window. |
| D19 | The seams in §6.2 are the only ways across layers, and Cargo dependencies enforce them. A CI check fails if `bastion-core` or `bastion-protocol` gains a dependency other than `serde`, or if `iii-sdk` is imported outside `src-tauri/src/engine/`. | The owner wants clean seams. A seam that only a reviewer guards erodes. |
| D20 | Every constraint and capability is enforced in a harness hook, and its ladder position is data. A turn's behavior changes only through those hooks and that data. | An eval can then name what changed. A rule kept only in a prompt cannot be measured. |
| D21 | The audit record is a hash chain with one writer. Each entry carries the hash of the one before it, and verification walks the chain. | Any change to a past entry breaks the chain, so the record is tamper-evident. It cannot stop a change; it shows one. |
| D22 | An eval compares two harness configurations on the same requests and the same workspace snapshots, and reports the difference with the hook decisions behind it. | A promotion or demotion comes with evidence, not a guess. |
| D23 | Forge, tracker, CI, model providers and notifications are adapters behind seams, chosen in Settings. A new product is a new adapter, not a core change. | The owner keeps the tools they already use. |
