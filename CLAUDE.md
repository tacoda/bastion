# CLAUDE.md

## The project

Bastion is a server and its clients. [PLAN.md](PLAN.md) holds the decisions
(cited in code as `PLAN.md D<n>`) and the phases; read §3 and §5 before a
change.

- `crates/bastion-core`: the factory's rules, pure. It may depend only on
  `serde` and `sha2`.
- `crates/bastion-protocol`: the API's types and routes. Only `serde`.
- `crates/bastion-server`: the HTTP API, events, the factory adapter, the
  store, the gate, the audit, the stack. Only `src/engine/` imports `iii-sdk`.
- `crates/bastion-desktop`: the iced client. It reaches a server only through
  the API; never through `bastion-core` or `iii-sdk`.
- `workers/`: Bastion's own iii workers.

## Before a commit

Run `pre_commit_code_health_safeguard` and commit only when it passes. After
changing code, run `code_health_review` on the changed files, and if Code
Health drops, fix it and confirm with `code_health_score`.

## Before a push

Run every CI step, not only the tests:

```sh
cargo fmt --all -- --check
scripts/check-seams.sh
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
