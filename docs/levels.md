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
