#!/usr/bin/env bash
# Gallery UI / shell catalog performance regression.
#
# Usage:
#   scripts/gtk-perf.sh smoke    # tiny folder + headless mutter --bench
#   scripts/gtk-perf.sh 20k      # generated library: display-free catalog
#                                # test, then mutter --bench if mutter exists
#
# Without mutter the GTK half prints a skip and exits 0 (same as
# gtk-snapshots.sh). CI / `cargo test` do not run this. Core 20k stays
# apps/gallery/scripts/e2e_20k.sh — do not invent a second rust.yml job.
#
# leftover_open persist is a worker and is skipped under --bench.
# leftover_open_ms on the GTK thread must stay ~0.

set -euo pipefail

usage() {
  echo "usage: $0 <smoke|20k>" >&2
  exit 2
}

PROFILE="${1:-}"
case "$PROFILE" in
  smoke | 20k) ;;
  *) usage ;;
esac

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=mutter-headless.sh
source "$ROOT/scripts/mutter-headless.sh"

metric() {
  local line key
  key="$1"
  line="$(printf '%s\n' "$2" | grep -E "metric_${key}=" | tail -1 || true)"
  if [[ -z "$line" ]]; then
    echo "gtk-perf: missing metric_${key}" >&2
    return 1
  fi
  echo "${line#*metric_${key}=}" | awk '{print $1}'
}

assert_lt() {
  local name="$1" value="$2" ceiling="$3"
  awk -v n="$name" -v v="$value" -v c="$ceiling" 'BEGIN {
    if (v+0 > c+0) {
      printf "gtk-perf: %s %s > %s\n", n, v, c > "/dev/stderr"
      exit 1
    }
    printf "gtk-perf: %s %s <= %s\n", n, v, c
  }'
}

start_mutter() {
  if mutter_headless_start; then
    return 0
  fi
  echo "gtk-perf: mutter is not installed or failed to start; skipping GTK --bench (exit 0)."
  return 1
}

write_smoke_folder() {
  local dir="$1"
  mkdir -p "$dir"
  python3 - "$dir" <<'PY'
import pathlib, sys
# Minimal 1×1 JPEG (same bytes as gallery-gtk session tests).
jpeg = bytes([
    0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01,
    0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43,
    0x00, 0x08, 0x06, 0x06, 0x07, 0x06, 0x05, 0x08, 0x07, 0x07, 0x07, 0x09,
    0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D, 0x0C, 0x0B, 0x0B, 0x0C, 0x19, 0x12,
    0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D, 0x1A, 0x1C, 0x1C, 0x20,
    0x24, 0x2E, 0x27, 0x20, 0x22, 0x2C, 0x23, 0x1C, 0x1C, 0x28, 0x37, 0x29,
    0x2C, 0x30, 0x31, 0x34, 0x34, 0x34, 0x1F, 0x27, 0x39, 0x3D, 0x38, 0x32,
    0x3C, 0x2E, 0x33, 0x34, 0x32, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01,
    0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00, 0x14, 0x00, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xC4, 0x00, 0x14, 0x10, 0x01, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00,
    0x00, 0x3F, 0x00, 0x7B, 0xFF, 0xD9,
])
root = pathlib.Path(sys.argv[1])
for i in range(8):
    (root / f"smoke-{i:02d}.jpg").write_bytes(jpeg)
PY
}

ensure_20k_library() {
  local out="${LOCALGALLERY_E2E_LIBRARY:-${TMPDIR:-/tmp}/localgallery-e2e-library}"
  export LOCALGALLERY_E2E_LIBRARY="$out"
  export LOCALGALLERY_E2E_COUNT="${LOCALGALLERY_E2E_COUNT:-20000}"
  export LOCALGALLERY_E2E_TODAY="${LOCALGALLERY_E2E_TODAY:-2026-06-11}"
  export LOCALGALLERY_E2E_SEED="${LOCALGALLERY_E2E_SEED:-42}"
  local count="$LOCALGALLERY_E2E_COUNT"
  local today="$LOCALGALLERY_E2E_TODAY"
  local seed="$LOCALGALLERY_E2E_SEED"
  local marker="${out}/.generated"
  local want="${count} ${today} ${seed} pillow-12.3.0"
  if [[ -f "$marker" ]] && [[ "$(cat "$marker")" == "$want" ]]; then
    echo "==> reusing ${out}"
    return 0
  fi
  echo "==> generate_test_library.py --count ${count} --today ${today} --out ${out}"
  mkdir -p "$out"
  local script="$ROOT/apps/gallery/scripts/generate_test_library.py"
  if command -v uv >/dev/null 2>&1; then
    uv run "$script" --out "$out" --count "$count" --seed "$seed" --today "$today"
  else
    local venv="${TMPDIR:-/tmp}/localgallery-generator-pillow-12.3.0"
    if [[ ! -x "${venv}/bin/python" ]]; then
      python3 -m venv "$venv"
      "${venv}/bin/pip" install --disable-pip-version-check "pillow==12.3.0"
    fi
    "${venv}/bin/python" "$script" --out "$out" --count "$count" --seed "$seed" --today "$today"
  fi
  echo "$want" > "$marker"
}

run_catalog_test() {
  echo "==> gallery-gtk e2e_catalog (display-free Session + ViewList)"
  (
    cd "$ROOT/shells"
    cargo test --locked --release -p gallery-gtk --test e2e_catalog \
      generated_library_shell_catalog_is_bounded \
      -- --ignored --nocapture --exact
  )
}

run_bench() {
  local folder="$1"
  local leftover_max="$2"
  local refill_max="$3"
  local gtk_max="$4"
  local ready_max="$5"
  echo "==> building gallery-gtk --release"
  (
    cd "$ROOT/shells"
    cargo build --locked --release -p gallery-gtk
  )
  local bin="$ROOT/shells/target/release/localgallery"
  if [[ ! -x "$bin" ]]; then
    echo "gtk-perf: missing $bin" >&2
    exit 1
  fi
  if ! start_mutter; then
    return 0
  fi
  trap mutter_headless_stop EXIT
  echo "==> localgallery --bench --folder ${folder}"
  local log
  log="$(mktemp)"
  set +e
  LOCALGALLERY_GTK_BENCH_TIMEOUT_SECS="${LOCALGALLERY_GTK_BENCH_TIMEOUT_SECS:-600}" \
    "$bin" --bench --folder "$folder" --size 1280x800 | tee "$log"
  local status=${PIPESTATUS[0]}
  set -e
  if [[ "$status" -ne 0 ]]; then
    echo "gtk-perf: --bench exited $status" >&2
    rm -f "$log"
    return 1
  fi
  local leftover refill gtk_thread ready photos
  leftover="$(metric leftover_open_ms "$(cat "$log")")"
  refill="$(metric refill_all_ms "$(cat "$log")")"
  gtk_thread="$(metric gtk_thread_ms "$(cat "$log")")"
  ready="$(metric ready_ms "$(cat "$log")")"
  photos="$(metric photos "$(cat "$log")")"
  rm -f "$log"
  echo "gtk-perf: photos=${photos}"
  assert_lt leftover_open_ms "$leftover" "$leftover_max"
  assert_lt refill_all_ms "$refill" "$refill_max"
  assert_lt gtk_thread_ms "$gtk_thread" "$gtk_max"
  assert_lt ready_ms "$ready" "$ready_max"
}

case "$PROFILE" in
  smoke)
    SMOKE_DIR="$(mktemp -d)"
    write_smoke_folder "$SMOKE_DIR"
    # Tiny library: leftover_open + 8-row ListStore must stay interactive.
    run_bench "$SMOKE_DIR" \
      "${LOCALGALLERY_GTK_BENCH_LEFTOVER_MAX_MS:-5000}" \
      "${LOCALGALLERY_GTK_BENCH_REFILL_MAX_MS:-2000}" \
      "${LOCALGALLERY_GTK_BENCH_GTK_THREAD_MAX_MS:-8000}" \
      "${LOCALGALLERY_GTK_BENCH_READY_MAX_MS:-30000}"
    rm -rf "$SMOKE_DIR"
    ;;
  20k)
    ensure_20k_library
    run_catalog_test
    # Regression caps, not a liveness target. leftover_open is a second
    # walk+enrich on the GTK thread today.
    run_bench "${LOCALGALLERY_E2E_LIBRARY}" \
      "${LOCALGALLERY_GTK_BENCH_LEFTOVER_MAX_MS:-50}" \
      "${LOCALGALLERY_GTK_BENCH_REFILL_MAX_MS:-2000}" \
      "${LOCALGALLERY_GTK_BENCH_GTK_THREAD_MAX_MS:-2000}" \
      "${LOCALGALLERY_GTK_BENCH_READY_MAX_MS:-15000}"
    ;;
esac
