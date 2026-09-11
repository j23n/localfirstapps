#!/usr/bin/env bash
# One-time Mac setup. Sibling of docker/bootstrap.sh.
# Installs the host tools the iOS shells need. Does not build the apps.
# Idempotent — re-run it after moving the workspace.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"   # mac/
workspace="$(cd "$here/.." && pwd)"                     # monorepo root

echo "workspace: $workspace"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "error: mac/bootstrap.sh is for macOS. On Linux use docker/bootstrap.sh." >&2
  exit 1
fi

missing=()
[ -d "$workspace/.git" ] || missing+=("monorepo .git")
for r in gallery contacts music health; do
  [ -d "$workspace/apps/$r" ] || missing+=("apps/$r")
done
if [ ${#missing[@]} -gt 0 ]; then
  echo "error: missing under $workspace: ${missing[*]}" >&2
  exit 1
fi

if ! xcode-select -p >/dev/null 2>&1; then
  echo "error: Xcode CLT not selected. Run: xcode-select --install" >&2
  echo "      or open Xcode once and accept the license." >&2
  exit 1
fi
echo "xcode-select: $(xcode-select -p)"
xcodebuild -version 2>/dev/null | head -2 || echo "note: xcodebuild not on PATH yet"

# rustup — Homebrew's rustup is keg-only.
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
if ! command -v rustup >/dev/null 2>&1; then
  echo "error: rustup not found. Install it, then re-run:" >&2
  echo "      brew install rustup && rustup-init -y" >&2
  echo "      or https://rustup.rs" >&2
  exit 1
fi
echo "rustup: $(command -v rustup)"
# Pull the pin in apps/gallery/core/rust-toolchain.toml (1.97.1 + iOS targets).
(cd "$workspace/apps/gallery/core" && rustup show)

# XcodeGen — same pin CI uses, not `brew install xcodegen`.
xcodegen_installer="$workspace/apps/gallery/scripts/install_xcodegen.sh"
if [[ ! -x "$xcodegen_installer" ]]; then
  echo "error: missing $xcodegen_installer" >&2
  exit 1
fi
export PATH="${HOME}/.local/bin:$PATH"
if command -v xcodegen >/dev/null 2>&1 && xcodegen --version 2>/dev/null | grep -q '2\.46\.0'; then
  echo "xcodegen: $(command -v xcodegen) (2.46.0)"
else
  echo "installing XcodeGen 2.46.0 into ~/.local/bin"
  /bin/bash "$xcodegen_installer"
fi

cat <<EOF

ok. next, from the app directory:

  cd $workspace/apps/gallery
  ./scripts/build_core.sh
  ./scripts/prepare_pack.sh          # optional; empty pack is fine
  xcodegen
  open LocalGallery.xcodeproj

contacts and music: cd apps/<name> && xcodegen && open LocalX.xcodeproj

Linux / Rust / Go verification stays in the Fedora container:
  cd $workspace/docker && ./bootstrap.sh
EOF
