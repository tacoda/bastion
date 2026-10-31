#!/usr/bin/env bash
# The seam check (PLAN.md D19, §6.2). Fails when a dependency crosses a seam.
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0
deps() { cargo tree -p "$1" --depth 1 -e normal --prefix none | tail -n +2 | awk '{print $1}' | sort -u; }

# The pure crates depend on serde only; the core also hashes the audit chain (D21).
for crate in bastion-protocol bastion-core; do
  allowed=serde; [ "$crate" = bastion-core ] && allowed="serde|sha2"
  cargo metadata --no-deps --format-version 1 | grep -q "\"name\":\"$crate\"" || continue
  extra=$(deps "$crate" | grep -vxE "$allowed" || true)
  if [ -n "$extra" ]; then echo "FAIL $crate depends on: $extra"; fail=1; fi
done

# The view reaches the app only through bastion-protocol.
for banned in bastion-core iii-sdk tauri; do
  if deps bastion-ui | grep -qx "$banned"; then echo "FAIL bastion-ui depends on $banned"; fail=1; fi
done

# Only the engine adapter imports iii-sdk.
leaks=$(grep -rl 'iii_sdk' src-tauri/src src | grep -v '^src-tauri/src/engine/' || true)
if [ -n "$leaks" ]; then echo "FAIL iii_sdk outside src-tauri/src/engine/: $leaks"; fail=1; fi

[ "$fail" -eq 0 ] && echo "seams ok"
exit "$fail"
