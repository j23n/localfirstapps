#!/usr/bin/env bash
#
# Generate UniFFI Swift + C header from a host-built contacts-ffi dylib.
# Uses the gallery workspace's uniffi-bindgen so core/ stays free of
# the CLI crate (ADR 0002 R13).
#
# Writes:
#   LocalContacts/ContactsCore.swift
#   LocalContacts/ContactsCoreFFI.h
#
# Usage: from apps/contacts, ./scripts/generate_bindings.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="$(cd "$ROOT/../.." && pwd)"
CORE_DIR="$REPO/core"
BINDGEN_DIR="$REPO/apps/gallery/core"
MODULE="ContactsCore"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-1.97.1}"

echo "==> cargo build -p contacts-ffi (host cdylib)"
(cd "$CORE_DIR" && cargo build --locked -p contacts-ffi)

DYLIB=""
for c in "$CORE_DIR/target/debug/libcontacts_ffi.so" \
         "$CORE_DIR/target/debug/libcontacts_ffi.dylib"; do
    if [[ -f "$c" ]]; then DYLIB="$c"; break; fi
done
if [[ -z "$DYLIB" ]]; then
    echo "error: no host libcontacts_ffi.{so,dylib} under core/target/debug" >&2
    exit 1
fi
echo "    bindgen library: $DYLIB"

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

# C decls come from the bridging header, not an import of the clang module.
python3 -c '
import pathlib, re, sys
p = pathlib.Path(sys.argv[1])
text = p.read_text()
patched, n = re.subn(
    r"\n#if canImport\(ContactsCoreFFI\)\nimport ContactsCoreFFI\n#endif\n",
    "\n",
    text,
    count=1,
)
if n:
    p.write_text(patched)
' "$STAGING/$MODULE.swift"

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

copy_if_changed "$STAGING/$MODULE.swift" "$ROOT/LocalContacts/ContactsCore.swift"
copy_if_changed "$STAGING/${MODULE}FFI.h" "$ROOT/LocalContacts/ContactsCoreFFI.h"
echo "done"
