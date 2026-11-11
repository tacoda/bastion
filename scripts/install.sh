#!/bin/sh
# Install Bastion from its GitHub release:
#   curl -fsSL https://tacoda.github.io/bastion/install.sh | sh
#   curl -fsSL https://tacoda.github.io/bastion/install.sh | sh -s -- --desktop
# Options:
#   --desktop        the window and the tray too (macOS: Bastion.app in ~/Applications)
#   --service        Linux: run the server as a systemd user service, for a VM or VPS
#   --version vX.Y.Z a release other than the newest
#   --prefix DIR     where bin/ and lib/ go (default ~/.local)
# Every file is checked against the release's SHA256SUMS; the sums' minisign
# signature is checked too when minisign is installed.
set -eu

REPO="tacoda/bastion"
# The release key's public half; a release is signed with its secret half.
PUBKEY="${BASTION_MINISIGN_PUBKEY:-RWRxPyyTFnBoVkMwrQGM3Z/h9W1jdGi9Vcqv0k2QVJyJzegnI7UaTVoS}"

desktop=no service=no version="" prefix="${HOME}/.local"
while [ $# -gt 0 ]; do
  case "$1" in
    --desktop) desktop=yes ;;
    --service) service=yes ;;
    --version) version="$2"; shift ;;
    --prefix) prefix="$2"; shift ;;
    *) echo "bastion: unknown option $1" >&2; exit 2 ;;
  esac
  shift
done

say() { printf 'bastion: %s\n' "$*"; }
die() { printf 'bastion: %s\n' "$*" >&2; exit 1; }

# The release target for this machine. Sandbox VMs need Apple Silicon or
# Linux with KVM, so those are the builds there are.
case "$(uname -s)/$(uname -m)" in
  Darwin/arm64) target=aarch64-apple-darwin ;;
  Linux/x86_64) target=x86_64-unknown-linux-gnu ;;
  Linux/aarch64 | Linux/arm64) target=aarch64-unknown-linux-gnu ;;
  *) die "no build for $(uname -s) on $(uname -m): Bastion runs on Apple Silicon Macs and on Linux" ;;
esac

if [ -z "$version" ]; then
  version="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
    | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)"
  [ -n "$version" ] || die "could not read the newest release of $REPO"
fi
v="${version#v}"
base="https://github.com/$REPO/releases/download/$version"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fetch() { curl -fsSL "$base/$1" -o "$work/$1" || die "could not download $1 from $version"; }

sums() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$@"; else shasum -a 256 "$@"; fi
}

# Download $1 and check it against the signed sums.
verified() {
  fetch "$1"
  expected="$(grep " $1\$" "$work/SHA256SUMS" | cut -d ' ' -f 1)"
  [ -n "$expected" ] || die "$1 is not in the release's SHA256SUMS"
  actual="$(cd "$work" && sums "$1" | cut -d ' ' -f 1)"
  [ "$expected" = "$actual" ] || die "$1 does not match its checksum; nothing was installed"
}

fetch SHA256SUMS
fetch SHA256SUMS.minisig
if command -v minisign >/dev/null 2>&1; then
  minisign -Vm "$work/SHA256SUMS" -x "$work/SHA256SUMS.minisig" -P "$PUBKEY" >/dev/null \
    || die "the release's signature does not check; nothing was installed"
  say "the release's signature checks"
else
  say "minisign is not installed, so only the checksums were checked (brew or apt install minisign)"
fi

mkdir -p "$prefix/bin" "$prefix/lib"
link() { ln -sf "$1" "$prefix/bin/$(basename "$1")"; }

if [ "$desktop" = yes ] && [ "$target" = aarch64-apple-darwin ]; then
  # The app holds the server and the CLI too.
  verified "bastion-desktop-$v-$target.zip"
  mkdir -p "$HOME/Applications"
  rm -rf "$HOME/Applications/Bastion.app"
  (cd "$HOME/Applications" && unzip -q "$work/bastion-desktop-$v-$target.zip")
  bin="$HOME/Applications/Bastion.app/Contents/MacOS"
  link "$bin/bastion"; link "$bin/bastion-server"
  say "installed Bastion.app in ~/Applications"
elif [ "$desktop" = yes ]; then
  verified "bastion-desktop-$v-$target.tar.gz"
  rm -rf "$prefix/lib/bastion-desktop-$v"
  tar -xzf "$work/bastion-desktop-$v-$target.tar.gz" -C "$prefix/lib"
  bin="$prefix/lib/bastion-desktop-$v/bin"
  for b in bastion bastion-server bastion-desktop bastion-tray; do link "$bin/$b"; done
  apps="${XDG_DATA_HOME:-$HOME/.local/share}"
  mkdir -p "$apps/applications" "$apps/icons/hicolor/256x256/apps"
  sed "s|^Exec=.*|Exec=$bin/bastion-desktop|" \
    "$prefix/lib/bastion-desktop-$v/share/applications/bastion.desktop" > "$apps/applications/bastion.desktop"
  cp "$prefix/lib/bastion-desktop-$v/share/icons/hicolor/256x256/apps/bastion.png" "$apps/icons/hicolor/256x256/apps/"
  say "installed the window and the tray"
else
  verified "bastion-$v-$target.tar.gz"
  rm -rf "$prefix/lib/bastion-$v"
  tar -xzf "$work/bastion-$v-$target.tar.gz" -C "$prefix/lib"
  bin="$prefix/lib/bastion-$v/bin"
  link "$bin/bastion"; link "$bin/bastion-server"
  say "installed the server and the CLI"
fi

if [ "$service" = yes ]; then
  [ "$(uname -s)" = Linux ] || die "--service is for Linux; on macOS the window starts the server"
  share="$(cd "$bin/../share/bastion" && pwd)"
  data="${XDG_DATA_HOME:-$HOME/.local/share}/dev.tacoda.bastion/server"
  units="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
  mkdir -p "$units" "$data"
  cat > "$units/bastion-server.service" <<UNIT
[Unit]
Description=Bastion server and its stack
After=network-online.target

[Service]
ExecStart=$bin/bastion-server --checkout $share --data $data --port 49190 --engine ws://127.0.0.1:49160
Restart=on-failure

[Install]
WantedBy=default.target
UNIT
  systemctl --user daemon-reload
  systemctl --user enable --now bastion-server.service
  say "the server runs as the systemd user service bastion-server"
  say "to keep it running after you log out: loginctl enable-linger $(id -un)"
fi

case ":$PATH:" in
  *":$prefix/bin:"*) ;;
  *) say "add $prefix/bin to your PATH" ;;
esac
command -v iii >/dev/null 2>&1 || say "Bastion's stack runs on iii 0.25.0, which is not installed yet; bastion-server's doctor says how"
say "Bastion $version is installed"
