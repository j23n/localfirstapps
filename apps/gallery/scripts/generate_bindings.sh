#!/usr/bin/env bash
#
# Generate UniFFI Swift + C header from a host-built gallery-ffi dylib.
# No Xcode, no xcframework — this is what Linux and CI run.
#
# Writes:
#   LocalGallery/GalleryCore.swift     committed bindings (iOS-stripped)
#   LocalGallery/GalleryCoreFFI.h      committed C decls
#   linux/swift-shim/…                 Linux compile-signal copies (import kept)
#
# Usage: from apps/gallery, ./scripts/generate_bindings.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CORE_DIR="$ROOT/core"
MODULE="GalleryCore"

if ! pkg-config --exists openssl && [[ -z "${OPENSSL_DIR:-}${OPENSSL_INCLUDE_DIR:-}" ]]; then
    echo "error: OpenSSL headers not found (pkg-config openssl / OPENSSL_DIR)." >&2
    echo "       Fedora: dnf install openssl-devel   Debian: apt install libssl-dev" >&2
    echo "       Needed because ort → ureq → native-tls on the host build." >&2
    exit 1
fi

echo "==> cargo build -p gallery-ffi (host cdylib)"
(cd "$CORE_DIR" && cargo build --locked -p gallery-ffi)

DYLIB=""
for c in "$CORE_DIR/target/debug/libgallery_ffi.so" \
         "$CORE_DIR/target/debug/libgallery_ffi.dylib"; do
    if [[ -f "$c" ]]; then DYLIB="$c"; break; fi
done
if [[ -z "$DYLIB" ]]; then
    echo "error: no host libgallery_ffi.{so,dylib} under core/target/debug" >&2
    exit 1
fi
echo "    bindgen library: $DYLIB"

STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT

echo "==> uniffi-bindgen (swift)"
(cd "$CORE_DIR" && cargo run --locked --quiet -p uniffi-bindgen --bin uniffi-bindgen-swift -- \
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

strip_gallerycore_ffi_import() {
    python3 -c '
import pathlib, re, sys
p = pathlib.Path(sys.argv[1])
text = p.read_text()
patched, n = re.subn(
    r"\n#if canImport\(GalleryCoreFFI\)\nimport GalleryCoreFFI\n#endif\n",
    "\n",
    text,
    count=1,
)
if n:
    p.write_text(patched)
' "$1"
}

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

# iOS: committed, import stripped (bridging header supplies C decls).
cp "$STAGING/$MODULE.swift" "$STAGING/$MODULE.swift.ios"
strip_gallerycore_ffi_import "$STAGING/$MODULE.swift.ios"
copy_if_changed "$STAGING/$MODULE.swift.ios" "$ROOT/LocalGallery/$MODULE.swift"
copy_if_changed "$STAGING/${MODULE}FFI.h" "$ROOT/LocalGallery/${MODULE}FFI.h"

# Linux shim: keep the import; header sits next to Package.swift.
# Do not copy uniffi's modulemap — it `use`s Darwin, which Linux Swift
# does not have. The compile signal only needs the header and the .so.
SHIM="$ROOT/linux/swift-shim"
copy_if_changed "$STAGING/$MODULE.swift" "$SHIM/Sources/GalleryFFICheck/GalleryCore.swift"
copy_if_changed "$STAGING/${MODULE}FFI.h" "$SHIM/CGalleryCoreFFI/GalleryCoreFFI.h"
cat > "$STAGING/module.modulemap.linux" <<'EOF'
module GalleryCoreFFI {
    header "GalleryCoreFFI.h"
    link "gallery_ffi"
    export *
}
EOF
copy_if_changed "$STAGING/module.modulemap.linux" "$SHIM/CGalleryCoreFFI/module.modulemap"

# UniFFI copies rustdoc into Swift `/** */` blocks. A rustdoc `Foo/*` opens a
# nested comment that never closes, and Xcode then fails at EOF.
reject_nested_swift_block_comments() {
    python3 -c '
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
depth = 0
line = 1
i = 0
while i < len(text):
    if text[i] == "\n":
        line += 1
        i += 1
        continue
    pair = text[i : i + 2]
    if pair == "/*":
        depth += 1
        if depth > 1:
            print(
                f"{path}:{line}: nested /* inside a Swift block comment "
                "(rewrite the rustdoc; `Places/*` is the usual culprit)",
                file=sys.stderr,
            )
            sys.exit(1)
        i += 2
        continue
    if pair == "*/":
        depth = max(0, depth - 1)
        i += 2
        continue
    i += 1
if depth:
    print(f"{path}: unterminated block comment", file=sys.stderr)
    sys.exit(1)
' "$1"
}

reject_nested_swift_block_comments "$ROOT/LocalGallery/$MODULE.swift"
reject_nested_swift_block_comments "$SHIM/Sources/GalleryFFICheck/GalleryCore.swift"

echo
echo "ios:  LocalGallery/$MODULE.swift"
echo "hdr:  LocalGallery/${MODULE}FFI.h"
echo "shim: linux/swift-shim/"
