#!/usr/bin/env bash
#
# Build contacts-ffi for iOS and assemble:
#
#   build/core/ContactsCore.xcframework
#   LocalContacts/ContactsCore.swift
#   LocalContacts/ContactsCoreFFI.h
#
# Usage:
#   ./scripts/build_ffi.sh [--release] [--sdk iphoneos|iphonesimulator|all]
#
# Xcode's LocalContacts target runs this as a pre-build script.
set -euo pipefail

readonly DEVICE_TARGET="aarch64-apple-ios"
readonly SIM_TARGET="aarch64-apple-ios-sim"
readonly LIB_NAME="libcontacts_ffi.a"
readonly DYLIB_NAME="libcontacts_ffi.dylib"
readonly MODULE="ContactsCore"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="$(cd "$ROOT/../.." && pwd)"
CORE_DIR="$REPO/core"
BINDGEN_DIR="$REPO/apps/gallery/core"
OUT_DIR="$ROOT/build/core"
XCFRAMEWORK="$OUT_DIR/$MODULE.xcframework"

PROFILE="debug"
CARGO_PROFILE="dev"
SDK="all"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --release) PROFILE="release"; CARGO_PROFILE="release"; shift ;;
        --sdk)
            SDK="${2:?--sdk requires iphoneos, iphonesimulator, or all}"
            shift 2
            ;;
        --sdk=*)
            SDK="${1#--sdk=}"
            shift
            ;;
        *)
            echo "usage: $0 [--release] [--sdk iphoneos|iphonesimulator|all]" >&2
            exit 2
            ;;
    esac
done

case "$SDK" in
    iphoneos) BUILD_DEVICE=1; BUILD_SIM=0 ;;
    iphonesimulator) BUILD_DEVICE=0; BUILD_SIM=1 ;;
    all) BUILD_DEVICE=1; BUILD_SIM=1 ;;
    *)
        echo "error: unknown --sdk $SDK" >&2
        exit 2
        ;;
esac

# Inside Xcode, only build the slice that matches the destination.
if [[ -n "${PLATFORM_NAME:-}" ]]; then
    case "$PLATFORM_NAME" in
        iphoneos) BUILD_DEVICE=1; BUILD_SIM=0 ;;
        iphonesimulator) BUILD_DEVICE=0; BUILD_SIM=1 ;;
    esac
fi
if [[ "${CONFIGURATION:-}" == "Release" ]]; then
    PROFILE="release"
    CARGO_PROFILE="release"
fi

mkdir -p "$OUT_DIR"

build_target() {
    local target="$1"
    local built_dir="$CORE_DIR/target/$target/$PROFILE"
    rm -f "$built_dir/$LIB_NAME" "$built_dir/$DYLIB_NAME"
    echo "==> cargo build ($PROFILE, $target)"
    (cd "$CORE_DIR" && cargo build --locked -p contacts-ffi --target "$target" --profile "$CARGO_PROFILE")
    [[ -f "$built_dir/$LIB_NAME" ]] || {
        echo "error: $built_dir/$LIB_NAME missing" >&2
        exit 1
    }
}

DEVICE_LIB=""
SIM_LIB=""
if [[ "$BUILD_DEVICE" -eq 1 ]]; then
    build_target "$DEVICE_TARGET"
    DEVICE_LIB="$CORE_DIR/target/$DEVICE_TARGET/$PROFILE/$LIB_NAME"
fi
if [[ "$BUILD_SIM" -eq 1 ]]; then
    build_target "$SIM_TARGET"
    SIM_LIB="$CORE_DIR/target/$SIM_TARGET/$PROFILE/$LIB_NAME"
fi

echo "==> uniffi-bindgen (swift)"
STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT
SIM_DYLIB="$CORE_DIR/target/$SIM_TARGET/$PROFILE/$DYLIB_NAME"
DEVICE_DYLIB="$CORE_DIR/target/$DEVICE_TARGET/$PROFILE/$DYLIB_NAME"
BINDGEN_DYLIB=""
if [[ "$BUILD_SIM" -eq 1 && -f "$SIM_DYLIB" ]]; then
    BINDGEN_DYLIB="$SIM_DYLIB"
elif [[ "$BUILD_DEVICE" -eq 1 && -f "$DEVICE_DYLIB" ]]; then
    BINDGEN_DYLIB="$DEVICE_DYLIB"
fi
if [[ -z "$BINDGEN_DYLIB" ]]; then
    echo "error: no contacts-ffi dylib from this build" >&2
    exit 1
fi
(cd "$BINDGEN_DIR" && cargo run --locked --quiet -p uniffi-bindgen --bin uniffi-bindgen-swift -- \
    --swift-sources --headers --modulemap \
    --module-name "${MODULE}FFI" \
    --modulemap-filename module.modulemap \
    "$BINDGEN_DYLIB" "$STAGING")

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
for output, contents in [
    (p, patched),
    (pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[2]).read_text()),
]:
    output.write_text("\n".join(line.rstrip() for line in contents.splitlines()) + "\n")
' "$STAGING/$MODULE.swift" "$STAGING/${MODULE}FFI.h"

cp "$STAGING/$MODULE.swift" "$ROOT/LocalContacts/ContactsCore.swift"
cp "$STAGING/${MODULE}FFI.h" "$ROOT/LocalContacts/ContactsCoreFFI.h"

echo "==> xcframework"
rm -rf "$XCFRAMEWORK"
ARGS=()
if [[ -n "$DEVICE_LIB" ]]; then
    ARGS+=(-library "$DEVICE_LIB" -headers "$STAGING")
fi
if [[ -n "$SIM_LIB" ]]; then
    ARGS+=(-library "$SIM_LIB" -headers "$STAGING")
fi
xcodebuild -create-xcframework "${ARGS[@]}" -output "$XCFRAMEWORK"
echo "    $XCFRAMEWORK"
