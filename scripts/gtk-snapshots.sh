#!/usr/bin/env bash
# Headless GTK design-pass captures. CI must not run this.
#
# Usage: scripts/gtk-snapshots.sh <contacts|music>
#
# When mutter is present, every implemented route is captured at 540x620 and
# 1280x800 in prefer-light and prefer-dark. PNGs are written under
# docs/screenshots/gtk-before/ only when a capture actually succeeds.
# When mutter is absent the script prints a skip and exits 0.

set -euo pipefail

usage() {
  echo "usage: $0 <contacts|music>" >&2
  exit 2
}

APP="${1:-}"
case "$APP" in
  contacts | music) ;;
  *) usage ;;
esac

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=mutter-headless.sh
source "$ROOT/scripts/mutter-headless.sh"
OUT="$ROOT/docs/screenshots/gtk-before"
SIZES=(540x620 1280x800)
SCHEMES=(prefer-light prefer-dark)

if ! command -v mutter >/dev/null 2>&1; then
  echo "gtk-snapshots: mutter is not installed; skipping headless capture (exit 0)."
  exit 0
fi

if [[ "$APP" == contacts ]]; then
  PACKAGE=contacts-gtk
  BINARY=localcontacts
  FOLDER="$ROOT/core/contacts-core/fixtures/r8/disjoint"
  ROUTES=(
    contact-list
    sync-conflict-group
    tag-management
    contact-edit
    folder-picker
    settings
    logs
    contact-detail
  )
else
  PACKAGE=music-gtk
  BINARY=localmusic
  FOLDER="$ROOT/shells/fixtures/music"
  ROUTES=(
    library
    settings
    folder-picker
    playlist-list
    now-playing
    logs
    playlist-detail
    add-tracks
    sync-conflict-group
  )
fi

echo "gtk-snapshots: building $PACKAGE"
(
  cd "$ROOT/shells"
  cargo build --locked -p "$PACKAGE"
)
BIN="$ROOT/shells/target/debug/$BINARY"
if [[ ! -x "$BIN" ]]; then
  echo "gtk-snapshots: missing binary $BIN" >&2
  exit 1
fi

cleanup() {
  mutter_headless_stop
  if [[ -n "${MUTTER_XDG_CONFIG:-}" ]]; then
    rm -rf "$MUTTER_XDG_CONFIG"
  fi
}
trap cleanup EXIT

if ! mutter_headless_start; then
  echo "gtk-snapshots: mutter failed to start; skipping headless capture (exit 0)."
  exit 0
fi

mkdir -p "$OUT"
wrote=0
failed=0

for route in "${ROUTES[@]}"; do
  for size in "${SIZES[@]}"; do
    for scheme in "${SCHEMES[@]}"; do
      case "$scheme" in
        prefer-dark) scheme_name=dark ;;
        *) scheme_name=light ;;
      esac
      dest="$OUT/${APP}-${route}-${size}-${scheme_name}.png"
      tmp="${dest}.part.png"
      rm -f "$tmp"
      extra=()
      if [[ "$route" != folder-picker ]]; then
        extra+=(--folder "$FOLDER")
      fi
      if ADW_DEBUG_COLOR_SCHEME="$scheme" \
        "$BIN" --route "$route" --snapshot "$tmp" --size "$size" "${extra[@]}"; then
        if [[ -f "$tmp" ]]; then
          mv "$tmp" "$dest"
          echo "wrote $dest"
          wrote=$((wrote + 1))
        else
          echo "skip (no png): $dest"
          failed=$((failed + 1))
        fi
      else
        echo "skip (capture failed): $dest"
        rm -f "$tmp"
        failed=$((failed + 1))
      fi
    done
  done
done

echo "gtk-snapshots: $wrote written, $failed skipped"
exit 0
