#!/usr/bin/env bash
# Build one platform's release files into dist/ (PLAN.md F10):
#   bastion-<version>-<target>.tar.gz          the server and the CLI, with the stack
#   bastion-desktop-<version>-<target>.zip     macOS: Bastion.app, which holds the server too
#   bastion-desktop-<version>-<target>.tar.gz  Linux: the window and the tray, with the server
# Usage: scripts/package.sh <version> [target]. The target defaults to this machine's.
set -euo pipefail
cd "$(dirname "$0")/.."

version="${1:?usage: scripts/package.sh <version> [target]}"
target="${2:-$(rustc -vV | sed -n 's/^host: //p')}"
out="dist"
rel="target/$target/release"

# The version a release build carries, which `bastion update` compares.
export BASTION_VERSION="$version"
cargo build --release --locked --target "$target" \
  -p bastion-server -p bastion-cli -p bastion-openai-compat -p bastion-desktop -p bastion-tray

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$out"

# The stack an installed server runs: the compose file with Bastion's own
# worker run from its binary instead of `cargo run`, the seed, the worker.
stack() {
  local share="$1/share/bastion"
  mkdir -p "$share/seed" "$share/workers/bastion-openai-compat"
  sed 's|run: cargo run -q -p bastion-openai-compat|run: ./workers/bastion-openai-compat/bastion-openai-compat|' \
    worker-compose.yaml > "$share/worker-compose.yaml"
  grep -q 'run: ./workers/bastion-openai-compat/bastion-openai-compat' "$share/worker-compose.yaml" \
    || { echo "the compose file's worker line did not change" >&2; exit 1; }
  cp seed/governance.sql "$share/seed/"
  cp "$rel/bastion-openai-compat" "$share/workers/bastion-openai-compat/"
}

# The server and the CLI.
server="$stage/bastion-$version"
mkdir -p "$server/bin"
cp "$rel/bastion-server" "$rel/bastion" "$server/bin/"
stack "$server"
tar -C "$stage" -czf "$out/bastion-$version-$target.tar.gz" "bastion-$version"

case "$target" in
  *apple-darwin)
    app="$stage/Bastion.app/Contents"
    mkdir -p "$app/MacOS" "$app/Resources"
    cp "$rel/bastion-desktop" "$rel/bastion-tray" "$rel/bastion-server" "$rel/bastion" "$app/MacOS/"
    cp assets/icon/bastion.icns "$app/Resources/"
    sed "s/@VERSION@/$version/g" scripts/Info.plist > "$app/Info.plist"
    stack "$app"
    # Ad hoc, until a Developer ID signs it: Apple Silicon runs nothing unsigned,
    # and a bundle is signed whole.
    codesign --force --deep --sign - "$stage/Bastion.app"
    (cd "$stage" && zip -qry "$OLDPWD/$out/bastion-desktop-$version-$target.zip" Bastion.app)
    ;;
  *linux*)
    desk="$stage/bastion-desktop-$version"
    mkdir -p "$desk/bin" "$desk/share/applications" "$desk/share/icons/hicolor/256x256/apps"
    cp "$rel/bastion-desktop" "$rel/bastion-tray" "$rel/bastion-server" "$rel/bastion" "$desk/bin/"
    cp scripts/bastion.desktop "$desk/share/applications/"
    cp assets/icon/bastion-256.png "$desk/share/icons/hicolor/256x256/apps/bastion.png"
    stack "$desk"
    tar -C "$stage" -czf "$out/bastion-desktop-$version-$target.tar.gz" "bastion-desktop-$version"
    ;;
esac

ls -1 "$out"
