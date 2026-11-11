#!/usr/bin/env bash
# Write the Homebrew tap's formula and cask for a release (PLAN.md F10):
#   Formula/bastion.rb  the server and the CLI, with their stack
#   Casks/bastion.rb    Bastion.app, on the formula, with bastion-desktop on the path
# Usage: scripts/brew.sh <version> <SHA256SUMS> <tap checkout>
set -euo pipefail

version="${1:?usage: scripts/brew.sh <version> <SHA256SUMS> <tap>}"
sums="${2:?the SHA256SUMS of the release}"
tap="${3:?a checkout of the tap}"
v="${version#v}"
url="https://github.com/tacoda/bastion/releases/download/v$v"

sum() {
  local found
  found="$(grep " $1\$" "$sums" | cut -d ' ' -f 1)"
  [ -n "$found" ] || { echo "$1 is not in $sums" >&2; exit 1; }
  echo "$found"
}

mkdir -p "$tap/Formula" "$tap/Casks"

cat > "$tap/Formula/bastion.rb" <<EOF
class Bastion < Formula
  desc "Software factory: agents plan, build and land changes under governance"
  homepage "https://tacoda.github.io/bastion/"
  version "$v"

  on_macos do
    on_arm do
      url "$url/bastion-$v-aarch64-apple-darwin.tar.gz"
      sha256 "$(sum "bastion-$v-aarch64-apple-darwin.tar.gz")"
    end
  end

  on_linux do
    on_intel do
      url "$url/bastion-$v-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "$(sum "bastion-$v-x86_64-unknown-linux-gnu.tar.gz")"
    end
    on_arm do
      url "$url/bastion-$v-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "$(sum "bastion-$v-aarch64-unknown-linux-gnu.tar.gz")"
    end
  end

  def install
    # The stack stays beside the binaries, where they look for it.
    libexec.install "bin", "share"
    bin.install_symlink libexec/"bin/bastion", libexec/"bin/bastion-server"
  end

  def caveats
    <<~TEXT
      Bastion's stack runs on iii 0.25.0, installed on its own;
      \`bastion-server\` checks for it and says how before it starts.
    TEXT
  end

  test do
    assert_match "usage: bastion", shell_output("#{bin}/bastion 2>&1", 1)
  end
end
EOF

cat > "$tap/Casks/bastion.rb" <<EOF
cask "bastion" do
  version "$v"
  sha256 "$(sum "bastion-desktop-$v-aarch64-apple-darwin.zip")"

  url "$url/bastion-desktop-#{version}-aarch64-apple-darwin.zip"
  name "Bastion"
  desc "Watch and steer the Bastion software factory"
  homepage "https://tacoda.github.io/bastion/"

  depends_on arch: :arm64
  depends_on macos: :ventura
  # The CLI and the server come from the formula, so they are on the path once.
  depends_on formula: "tacoda/bastion/bastion"

  app "Bastion.app"
  binary "#{appdir}/Bastion.app/Contents/MacOS/bastion-desktop"

  # Not signed with a Developer ID yet, and Homebrew dropped --no-quarantine:
  # the download is pinned to its signed sum, so the quarantine mark goes.
  postflight do
    system_command "/usr/bin/xattr",
                   args: ["-dr", "com.apple.quarantine", "#{appdir}/Bastion.app"]
  end
end
EOF

echo "wrote $tap/Formula/bastion.rb and $tap/Casks/bastion.rb for $v"
