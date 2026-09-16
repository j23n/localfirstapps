#!/usr/bin/env bash
#
# Generate committed UniFFI Swift/header from a host-built music-ffi dylib.
# The bindgen CLI stays in the gallery app workspace, never core/.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="$(cd "$ROOT/../.." && pwd)"
CORE_DIR="$REPO/core"
BINDGEN_DIR="$REPO/apps/gallery/core"
MODULE="MusicCore"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-1.97.1}"

echo "==> cargo build -p music-ffi (host cdylib)"
(cd "$CORE_DIR" && cargo build --locked -p music-ffi)

DYLIB=""
for candidate in "$CORE_DIR/target/debug/libmusic_ffi.so" \
                 "$CORE_DIR/target/debug/libmusic_ffi.dylib"; do
    if [[ -f "$candidate" ]]; then
        DYLIB="$candidate"
        break
    fi
done
if [[ -z "$DYLIB" ]]; then
    echo "error: no host libmusic_ffi.{so,dylib} under core/target/debug" >&2
    exit 1
fi

STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT

echo "==> uniffi-bindgen (swift)"
(cd "$BINDGEN_DIR" && cargo run --locked --quiet -p uniffi-bindgen --bin uniffi-bindgen-swift -- \
    --swift-sources --headers --modulemap \
    --module-name "${MODULE}FFI" \
    --modulemap-filename module.modulemap \
    "$DYLIB" "$STAGING")

for expected in "$MODULE.swift" "${MODULE}FFI.h" "module.modulemap"; do
    [[ -f "$STAGING/$expected" ]] || {
        echo "error: uniffi-bindgen did not produce $expected" >&2
        ls -1 "$STAGING" >&2
        exit 1
    }
done

# C declarations come from the bridging header, not a generated module import.
python3 -c '
import pathlib, re, sys
p = pathlib.Path(sys.argv[1])
text = p.read_text()
patched, _ = re.subn(
    r"\n#if canImport\(MusicCoreFFI\)\nimport MusicCoreFFI\n#endif\n",
    "\n",
    text,
    count=1,
)
for output, contents in [
    (p, patched),
    (pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[2]).read_text()),
]:
    output.write_text("\n".join(line.rstrip() for line in contents.splitlines()) + "\n")
' "$STAGING/$MODULE.swift" "$STAGING/${MODULE}FFI.h"

copy_if_changed() {
    local src="$1" dst="$2"
    mkdir -p "$(dirname "$dst")"
    if [[ ! -f "$dst" ]] || ! cmp -s "$src" "$dst"; then
        cp "$src" "$dst"
        echo "    updated ${dst#"$ROOT/"}"
    else
        echo "    unchanged ${dst#"$ROOT/"}"
    fi
}

copy_if_changed "$STAGING/$MODULE.swift" "$ROOT/LocalMusic/MusicCore.swift"
copy_if_changed "$STAGING/${MODULE}FFI.h" "$ROOT/LocalMusic/MusicCoreFFI.h"
echo "done"
