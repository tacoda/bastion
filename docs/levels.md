# Levels

What each spike proved, with the versions it ran on. PLAN.md A0 lists the
spikes.

Versions: `iii` 0.25.0, `iii-sdk` 0.25.0, Tauri 2.12.5, Leptos 0.8.22,
Trunk 0.22.1, rustc 1.92.0, macOS on Apple Silicon.

## A0

| Spike | Result | Date | Notes |
|---|---|---|---|
| N1 | pass | 2026-10-30 | A Rust function answers, an engine function answers a Rust caller (379 functions), an observer gets each call by push, and `database::query` and `database::transaction` work, with rows keyed by column name. The first run failed only because the stack still carried the old `floyd` namespace. |
| N2 | dropped | 2026-10-30 | Dropped by PLAN.md D18: the factory needs no queue. `Void` delivered; `Enqueue` failed for the namespace reason. |
| N3 | pass | 2026-10-30 | Three runs in a row: Rust binds `harness::turn-completed`, and a `pre-trigger` hook rewrites `sandbox_id` on `sandbox::fs::read`. 5.7 s, 3.5 s, 3.5 s. The earlier failure, under the old name, did not recur, and its cause is unknown. |
| N4 | pass | 2026-10-30 | A 2 MB binary file left the VM whole through its channel, in 197 ms. |
| N5 | pass | 2026-10-30 | Compose started the `bastion-spikes` binary through `scripts.run` and restarted it: pid 66455, then 66457, in 1.4 s. |
| N6 | pass | 2026-10-30 | `cargo tauri dev` opened the window. `iii trigger -n bastion bastion::app::changed` reached the page as the `changed` event, and the page called the `seen` command back: `PAGE SAW {"id":"n6","kind":"job","revision":1}`. `iii trigger` printed `{}` and then did not exit. |

A0's exit is met: N1 and N3 to N6 pass, and N3 passed three runs in a row.
N6's engine-status line and theme were not checked by eye.

## A3

| Spike | Result | Date | Notes |
|---|---|---|---|
| N7 | pass | 2026-10-30 | Two runs, 6.7 s and 5.9 s: `worktree::create` made a worktree of a scratch repo, its branch went into a fresh VM as a git bundle (`sandbox::fs::write` with `content_b64`), one `harness` turn on gpt-5-mini committed there through `sandbox::exec` pinned by the `pre-trigger` hook, and the 377-byte bundle came back through the content channel and fast-forwarded the worktree. `content_b64` is for small files; a large repo needs a channel upload. The spike's cleanup sent the wrong id field to `worktree::remove`, which left one scratch worktree registered. |

## A5

| Spike | Result | Date | Notes |
|---|---|---|---|
| N8 | pass | 2026-10-31 | A `harness::hook::pre-trigger` binding answers `{ "decision": "deny", "reason": … }` or `{ "decision": "hold", … }`. A denied call's reason reaches the turn verbatim as its result. A held call parks the turn (status stays `running`), and the envelope carries `session_id`, `turn_id` and `call.id`. `harness::function::resolve` takes `session_id`, `turn_id`, `function_call_id`, `action` (`deliver` or `execute`), `content`, `details` and `is_error`; an unknown field such as `mode` is ignored and the default `deliver` sends an empty result. The held arguments arrive without the hook's `sandbox_id` mutation, so on approval the app runs the call itself, pinned to the job's VM, and delivers the result. `harness::stop` exists and answers `{ "stopping": … }`. One run: the model gave up after the deny until told to make the second call anyway. |
