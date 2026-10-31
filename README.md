# Bastion

Bastion is a software factory that runs on your own machine. A request goes in,
agents work it through a pipeline of stages, and a reviewed change comes out.
Nothing merges itself. It's a native app (Tauri 2, Leptos, Rust) on the
[iii](https://iii.dev) engine, and it's early: the app shell, Doctor and the
stack controls work, and the factory itself starts in phase A3 of
[PLAN.md](PLAN.md).

```sh
cargo tauri dev
```

That opens the app. Go to **Engine**, press **Start**, and Bastion runs Doctor,
then brings up the stack in `worker-compose.yaml`. If the stack already runs,
the app attaches to it instead of starting a second one.

## What it is not

Not an editor: the only code you see in Bastion is the diff a job proposes. Not
a web app, and it serves no HTTP. Not multi-user: one owner, no sign-in. Not
Windows, because sandbox VMs need Apple Silicon or Linux with `/dev/kvm`.

## What you need

- Rust, from `rust-toolchain.toml` (rustup picks it up), and the
  `wasm32-unknown-unknown` target.
- `cargo install tauri-cli trunk`.
- `iii` 0.25.0 on your `PATH`.
- A model provider key, such as `ANTHROPIC_API_KEY` or `OPENAI_API_KEY`, in the
  environment you start Bastion from. The stack inherits it.
- `gh`, signed in, for pushes and pull requests. Landing locally works
  without it.

If something is missing, Doctor on the Engine page names it and says how to
fix it. A failed required check stops the start.

## How it fits together

| Part | Where | Does |
|---|---|---|
| View | `src/` | Draws the pages. Calls typed commands only. |
| Protocol | `crates/bastion-protocol` | The commands and events between the view and the app. |
| App | `src-tauri/` | Profiles, Doctor, the stack, and the engine adapter. |
| Engine adapter | `src-tauri/src/engine/` | The only code that imports `iii-sdk`. |
| Spikes | `crates/bastion-spikes` | The A0 checks, results in `docs/levels.md`. |

Each seam has a fake for tests, and `scripts/check-seams.sh` fails the build
when code crosses one (PLAN.md §6.2).

Your profile lives in `profiles.toml` in the OS config directory
(`~/Library/Application Support/dev.tacoda.bastion/` on macOS). It holds the
engine address and the checkout, and never a secret.

## Checks

```sh
cargo fmt --all -- --check
scripts/check-seams.sh
cargo clippy --workspace --exclude bastion-ui --all-targets -- -D warnings
cargo clippy -p bastion-ui --target wasm32-unknown-unknown -- -D warnings
cargo test --workspace --exclude bastion-ui
```

CI runs the same on macOS and Linux. Two live checks need a running stack:
`cargo test -p bastion -- --ignored --nocapture`.

## Next

[PLAN.md](PLAN.md) has the phases, A0 to A11, and the map to wipp's features.
Start with §6 if you want to change code: it says which layer owns what.
