#!/usr/bin/env bash
# Build libgallery_ffi.so, then `swift build` the UniFFI Swift against it.
# This is the Fedora-side compile signal. Fails naming the missing tool.
set -euo pipefail

SHIM="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SHIM/../.." && pwd)"
CORE="$ROOT/core"

if ! pkg-config --exists openssl && [[ -z "${OPENSSL_DIR:-}${OPENSSL_INCLUDE_DIR:-}" ]]; then
    echo "error: OpenSSL headers not found (pkg-config openssl / OPENSSL_DIR)." >&2
    echo "       Fedora: dnf install openssl-devel   Debian: apt install libssl-dev" >&2
    exit 1
fi

if ! command -v swift >/dev/null 2>&1; then
    echo "error: swift not on PATH. On Fedora: dnf install swift-lang" >&2
    echo "      or install a Swift.org toolchain into ~/.cache and put swift on PATH." >&2
    exit 1
fi

echo "==> cargo build -p gallery-ffi"
(cd "$CORE" && cargo build --locked -p gallery-ffi)

LIBDIR="$CORE/target/debug"
if [[ ! -f "$LIBDIR/libgallery_ffi.so" && ! -f "$LIBDIR/libgallery_ffi.dylib" ]]; then
    echo "error: libgallery_ffi not in $LIBDIR" >&2
    exit 1
fi

echo "==> swift build"
(
    cd "$SHIM"
    export PKG_CONFIG_PATH="${PKG_CONFIG_PATH:-}"
    swift build -Xlinker -L"$LIBDIR" -Xlinker -rpath -Xlinker "$LIBDIR"
)
echo "ok: linux swift shim"
