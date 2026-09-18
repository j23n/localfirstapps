#!/usr/bin/env bash
#
# Generate the synthetic library (if needed) and run the ignored 20k
# performance-regression suite (scan → enrich → index → memories → bounded
# FFI windows). CI uses fresh generated data and the deterministic hard
# ceilings in the tests; the optional machine-local timing baseline remains
# useful for tighter developer regressions.
#
# Usage (from anywhere):
#   ./apps/gallery/scripts/e2e_20k.sh
#   LOCALGALLERY_E2E_COUNT=300 ./apps/gallery/scripts/e2e_20k.sh   # smoke
#   LOCALGALLERY_E2E_RECORD=1 ./apps/gallery/scripts/e2e_20k.sh    # rewrite goldens
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
COUNT="${LOCALGALLERY_E2E_COUNT:-20000}"
TODAY="${LOCALGALLERY_E2E_TODAY:-2026-06-11}"
SEED="${LOCALGALLERY_E2E_SEED:-42}"
GENERATOR_VERSION="pillow-12.3.0"
FFI_MAX_SECONDS="${LOCALGALLERY_E2E_FFI_MAX_SECONDS:-30}"
TOTAL_MAX_SECONDS="${LOCALGALLERY_E2E_TOTAL_MAX_SECONDS:-240}"
OUT="${LOCALGALLERY_E2E_LIBRARY:-${TMPDIR:-/tmp}/localgallery-e2e-library}"
SCRIPT="${ROOT}/apps/gallery/scripts/generate_test_library.py"
MARKER="${OUT}/.generated"

export LOCALGALLERY_E2E_COUNT="${COUNT}"
export LOCALGALLERY_E2E_TODAY="${TODAY}"
export LOCALGALLERY_E2E_SEED="${SEED}"
export LOCALGALLERY_E2E_LIBRARY="${OUT}"
export LOCALGALLERY_E2E_RECORD="${LOCALGALLERY_E2E_RECORD:-}"

MARKER_VALUE="${COUNT} ${TODAY} ${SEED} ${GENERATOR_VERSION}"
if [[ ! -f "${MARKER}" ]] || [[ "$(cat "${MARKER}")" != "${MARKER_VALUE}" ]]; then
    echo "==> generate_test_library.py --count ${COUNT} --today ${TODAY} --out ${OUT}"
    mkdir -p "${OUT}"
    if command -v uv >/dev/null 2>&1; then
        uv run "${SCRIPT}" --out "${OUT}" --count "${COUNT}" --seed "${SEED}" --today "${TODAY}"
    else
        GENERATOR_VENV="${TMPDIR:-/tmp}/localgallery-generator-pillow-12.3.0"
        if [[ ! -x "${GENERATOR_VENV}/bin/python" ]]; then
            python3 -m venv "${GENERATOR_VENV}"
            "${GENERATOR_VENV}/bin/pip" install --disable-pip-version-check \
                "pillow==12.3.0"
        fi
        "${GENERATOR_VENV}/bin/python" "${SCRIPT}" \
            --out "${OUT}" --count "${COUNT}" --seed "${SEED}" --today "${TODAY}"
    fi
    echo "${MARKER_VALUE}" > "${MARKER}"
else
    echo "==> reusing ${OUT} (${COUNT} stills, ${TODAY})"
fi

cd "${ROOT}/apps/gallery/core"
echo "==> precompile generated-data tests (excluded from data-path metrics)"
cargo test --locked --release -p gallery-scan --test e2e_generated_library --no-run
cargo test --locked --release -p gallery-ffi --test e2e_windows --no-run
STARTED="$(date +%s)"

echo "==> gallery-scan e2e_generated_library (scan, enrich, index, memories)"
cargo test --locked --release -p gallery-scan --test e2e_generated_library \
    generated_library_regression \
    -- --ignored --nocapture --exact

echo "==> gallery-ffi e2e_windows (20k structure, bounded reads, stale refusal)"
FFI_STARTED="$(date +%s)"
cargo test --locked --release -p gallery-ffi --test e2e_windows \
    generated_library_ffi_windows_are_bounded_and_generation_checked \
    -- --ignored --nocapture --exact
FFI_ELAPSED="$(( $(date +%s) - FFI_STARTED ))"
TOTAL_ELAPSED="$(( $(date +%s) - STARTED ))"

echo "[gallery-e2e-20k] metric_count=${COUNT}"
echo "[gallery-e2e-20k] metric_ffi_window_max=128 hard_limit=256"
echo "[gallery-e2e-20k] metric_ffi_elapsed_seconds=${FFI_ELAPSED} ceiling=${FFI_MAX_SECONDS}"
echo "[gallery-e2e-20k] metric_total_elapsed_seconds=${TOTAL_ELAPSED} ceiling=${TOTAL_MAX_SECONDS}"

if (( FFI_ELAPSED > FFI_MAX_SECONDS )); then
    echo "[gallery-e2e-20k] FFI window regression: ${FFI_ELAPSED}s > ${FFI_MAX_SECONDS}s" >&2
    exit 1
fi
if (( TOTAL_ELAPSED > TOTAL_MAX_SECONDS )); then
    echo "[gallery-e2e-20k] total regression: ${TOTAL_ELAPSED}s > ${TOTAL_MAX_SECONDS}s" >&2
    exit 1
fi
