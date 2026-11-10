#!/usr/bin/env bash
# The seam check (PLAN.md D19). Fails when a dependency crosses a seam.
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0
deps() { cargo tree -p "$1" --depth 1 -e normal --prefix none | tail -n +2 | awk '{print $1}' | sort -u; }

# The pure crates depend on serde only; the core also hashes the audit chain (D21).
for crate in bastion-protocol bastion-core; do
  allowed=serde; [ "$crate" = bastion-core ] && allowed="serde|sha2"
  extra=$(deps "$crate" | grep -vxE "$allowed" || true)
  if [ -n "$extra" ]; then echo "FAIL $crate depends on: $extra"; fail=1; fi
done

# A client reaches a server only through its API (D27, D37, D38).
for client in bastion-client bastion-desktop bastion-cli; do
  for banned in bastion-core bastion-server iii-sdk; do
    if deps "$client" | grep -qx "$banned"; then echo "FAIL $client depends on $banned"; fail=1; fi
  done
done

# In the server, only the engine adapter imports iii-sdk.
leaks=$(grep -rl 'iii_sdk' crates/bastion-server/src | grep -v '^crates/bastion-server/src/engine/' || true)
if [ -n "$leaks" ]; then echo "FAIL iii_sdk outside crates/bastion-server/src/engine/: $leaks"; fail=1; fi

[ "$fail" -eq 0 ] && echo "seams ok"
exit "$fail"
