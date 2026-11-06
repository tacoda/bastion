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

## A6

| Spike | Result | Date | Notes |
|---|---|---|---|
| N9 | partial | 2026-10-31 | `judge::evaluate` lives in namespace `bastion` and answers `{ "status": "error", "code": "provider_unavailable" }`: the stack has the `judge` worker but no `judge-<provider>` worker, so it has no model to grade with. Bastion grades with a harness turn that has no tools instead, and reads its last `GRADE: n` line. `harness::metrics` with `root_session_id` gives each session's `turns`, `function_calls`, `function_call_errors` and `cost_usd`, which the eval tallies. The stock `eval` worker analyzes finished harness sessions; it does not compare two configurations. `worktree::create` takes a commit as `base_ref`, so an eval pins its snapshot as a commit. |
| N11 | pass | 2026-10-31 | The stack's `configuration` worker (namespace `default`) can carry Bastion's harness and factory defaults (D26). `configuration::ensure` seeds an id once and writes `config/<id>.yaml`; a later `ensure` answers `"action": "preserved"` and keeps the stored value. A hand edit to that file reaches the next `configuration::get` with no restart. `configuration::on-updated` announces changes. `config/` is git-ignored, so the stack's own files hold the owner's choices. Spike: `crates/bastion-spikes/examples/n11_config.rs`. |

## S

| Spike | Result | Date | Notes |
|---|---|---|---|
| N12 | pass | 2026-11-06 | An `axum` 0.8 server (`n12_server`) answers `/ping` and streams `/events` as server-sent events; both refuse a request without the bearer token (401). `tower-http`'s `CorsLayer` allows the window's origins (`http://localhost:1420` in dev, `tauri://localhost` built), and a preflight from any other origin gets no allow header. In the `cargo tauri dev` window, the page read the stream with `fetch` from Rust (`web-sys`, so the token goes in a header) after the CSP gained `connect-src ... http://127.0.0.1:*`; without it the CSP blocks the call. The server was killed after 13 events and started again, and the page reconnected with `Last-Event-ID: 13`. Two stacks run side by side on this machine: a copy of `worker-compose.yaml` on ports 49170 and 49171, run from its own directory, keeps its own `data/`, so its `bastion.db` has no `jobs` table while the first holds 15 jobs. Both work in namespace `bastion`, so a server needs only the engine URL and a data directory per target. Not checked: a built app (origin `tauri://localhost`), and whether two stacks' VMs collide when both run turns. Spikes: `n12_server.rs`, `n12_stacks.rs`, and the page panel `src/spike_n12.rs` (removed after). |

## K

| Spike | Result | Date | Notes |
|---|---|---|---|
| N13 | pass | 2026-11-06 | A provider key can live in the stack's `secrets` vault and reach `llm-router` with no key in the stack's environment (D34). `secrets::detect` found `ANTHROPIC_API_KEY` only in the login shell; `secrets::import` with `source: login_shell`, `store: vault` and `consumers: ["llm-router"]` stored it (the worker reads the value itself, so it never passes through the caller) and returned only a masked hint and `secret://ANTHROPIC_API_KEY`. `configuration::set` on `bastion-llm-router` with `providers.anthropic.api_key = "secret://ANTHROPIC_API_KEY"` turned `router::provider::list` for Anthropic from `credential_source: none` to `secret`, `configured: true`, default model `claude-sonnet-5-5`. A job with the plan stage on `claude-haiku-5-5` then ran its plan turn and reached Approve. The router's entry was `null` before, so a set replaces the whole value: Bastion must read, change one provider and write back. `providers.openai.api_url` reaches another host: pointed at a local recorder ending in `/v1/chat/completions`, `provider-openai` sent Chat Completions (`messages`, `tools`, `stream`, `max_completion_tokens`, `reasoning_effort`, `prompt_cache_key`) with the key as a bearer token, and also sent `/v1/embeddings` to the same base. So one compatible host can stand in for OpenAI, but not beside it, and a host that lacks embeddings or rejects `reasoning_effort` or `prompt_cache_key` would fail; K3's worker stays. The function list carries no request schemas, so payloads were found from the workers' errors. Spike: `n13_keys.rs`. |
