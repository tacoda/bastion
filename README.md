# Bastion

Bastion is a software factory you can govern. A request goes in, agents work
it through a workflow, and a reviewed change comes out. A workflow is a graph
of steps that you see on a canvas. Each step is a prompt, an agent, a skill
or a shell script, each with one text you edit, and each waits for
your approval or runs on its own, as you set it. Bastion ships the common
workflows with their text filled in, and every word of them is yours to
change. Bastion is a server and its clients: `bastion-server` runs the factory on
the [iii](https://iii.dev) engine, and the desktop app (on
[iced](https://iced.rs)) and the `bastion` command line talk to it over HTTP.
It's early. [VISION.md](VISION.md) says where it is going, and
[DECISIONS.md](DECISIONS.md) holds the goals and the decisions behind it.

## Install

On an Apple Silicon Mac or on Linux, install the server and the CLI, and add
`--desktop` for the app:

```sh
curl -fsSL https://tacoda.github.io/bastion/install.sh | sh -s -- --desktop
```

On a Mac, Homebrew works too: `brew install --cask tacoda/bastion/bastion`.
The installer checks each download against the release's signed checksums,
and `bastion update` moves you to the newest release. On a Linux VM, add
`--service` to run the server as a systemd user service.

## Run it from source

```sh
cargo build -p bastion-server
cargo run -p bastion-desktop
```

The client starts a local server if none runs on port 49190, and the server
starts the stack in `worker-compose.yaml`. Go to **Engine** to see Doctor and
the stack, then **Jobs** and **New request** to start work.

## What it is not

Not an editor: the only code you see in Bastion is the diff a job proposes.
Not a web app. Not Windows, because sandbox VMs need Apple Silicon or Linux
with `/dev/kvm`. A server listens on loopback unless you give it a TLS
certificate, and then several people can share it, each with a token the
server issued them.

## What you need

- Rust, from `rust-toolchain.toml` (rustup picks it up).
- `iii` 0.25.0 on your `PATH`.
- A model provider key, set in **Settings**, which keeps it in the stack's
  vault. A key in the environment works too.
- A `GH_TOKEN` in the stack's vault for the workflows that open pull
  requests: their shell steps ask for the network and that secret. A workflow
  that merges on this machine works without it.

If something is missing, Doctor on the Engine page names it and says how to
fix it. A failed required check stops the start.

## How it fits together

| Part | Where | Does |
|---|---|---|
| Core | `crates/bastion-core` | The factory's rules, as pure functions. |
| Protocol | `crates/bastion-protocol` | The API's types and routes, shared by the server and its clients. |
| Server | `crates/bastion-server` | The HTTP API and its events, the factory, the store, the gate, the audit and the stack. |
| Engine adapter | `crates/bastion-server/src/engine/` | The only server code that imports `iii-sdk`. |
| Client library | `crates/bastion-client` | What every client uses to reach a server: the server list, the local server's start, the calls and the event stream. |
| Desktop client | `crates/bastion-desktop` | The pages, on iced. |
| Command line | `crates/bastion-cli` | `bastion`: list and follow jobs, submit a request, approve, send back, stop, update. |
| Workers | `workers/` | Bastion's own iii workers. |

`scripts/check-seams.sh` fails the build when code crosses a seam (DECISIONS.md
D19).

What the engine was found to do, and on which versions, is in
`docs/engine-findings.md`.

## Checks

```sh
cargo fmt --all -- --check
scripts/check-seams.sh
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs the same on macOS and Linux. The live checks need a running stack:
`cargo test -p bastion-server -- --ignored --nocapture`.
