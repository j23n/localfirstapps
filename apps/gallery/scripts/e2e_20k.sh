#!/usr/bin/env bash
#
# Generate the synthetic library (if needed) and run the ignored 20k
# performance-regression suite (scan → enrich → index → memories).
# Local-only — not a GitHub workflow.
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
OUT="${LOCALGALLERY_E2E_LIBRARY:-${TMPDIR:-/tmp}/localgallery-e2e-library}"
SCRIPT="${ROOT}/apps/gallery/scripts/generate_test_library.py"
MARKER="${OUT}/.generated"

export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-1.97.1}"
export LOCALGALLERY_E2E_COUNT="${COUNT}"
export LOCALGALLERY_E2E_TODAY="${TODAY}"
export LOCALGALLERY_E2E_SEED="${SEED}"
export LOCALGALLERY_E2E_LIBRARY="${OUT}"
export LOCALGALLERY_E2E_RECORD="${LOCALGALLERY_E2E_RECORD:-}"

if [[ ! -f "${MARKER}" ]] || [[ "$(cat "${MARKER}")" != "${COUNT} ${TODAY} ${SEED}" ]]; then
    echo "==> generate_test_library.py --count ${COUNT} --today ${TODAY} --out ${OUT}"
    mkdir -p "${OUT}"
    if command -v uv >/dev/null 2>&1; then
        uv run "${SCRIPT}" --out "${OUT}" --count "${COUNT}" --seed "${SEED}" --today "${TODAY}"
    else
        python3 "${SCRIPT}" --out "${OUT}" --count "${COUNT}" --seed "${SEED}" --today "${TODAY}"
    fi
    echo "${COUNT} ${TODAY} ${SEED}" > "${MARKER}"
else
    echo "==> reusing ${OUT} (${COUNT} stills, ${TODAY})"
fi

cd "${ROOT}/apps/gallery/core"
echo "==> gallery-scan e2e_generated_library (scan, enrich, index, memories)"
cargo test --locked --release -p gallery-scan --test e2e_generated_library \
    generated_library_regression \
    -- --ignored --nocapture --exact
