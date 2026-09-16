# Conformance harness

Structural checks against the ADRs. Review time is the bottleneck; this
reduces it. It does not remove it (ADR 0007 R16).

The **first** check is the one a source grep cannot do.

```
python3 conformance/graph/check.py
python3 conformance/graph/check.py --self-test
```

## Dependency tripwire — ADR 0002 R13

`conformance/graph/` walks every core lockfile that exists —
`core/Cargo.lock` and `apps/gallery/core/Cargo.lock` — and unions
findings by package name. Preferring only `core/` would hide a
networking crate that lives only in the extracted workspace.
`--lockfile` is a single-file override.

The policy is zero-exception by default. Every `[[allow]]` is an explicit
reviewed build-time exception with a reason, offline override, and the exact
known policy hits below its roots. The checker fails malformed, duplicate,
unused, or unreviewed entries. It also fails if those transitive hits grow or
shrink without an allowlist update. The current exception is `ort` /
`ort-sys`; `ORT_LIB_LOCATION` bypasses their build-time binary download.
It remains valid because gallery enables `download-binaries` but not
`fetch-models`; `cargo tree -p gallery-ml -e all --locked` places `ureq` and
its TLS/OpenSSL chain under `ort-sys [build-dependencies]`, not the runtime
dependency tree.

The graph is **green**. Place names come from `localcore-geo` (packed
gazetteer + admin-0 polygons). `gallery-geo` and its `ureq` client are
gone. `--expect-violations` remains for pinning a known-red graph; it
is not used here.

This is a dependency-review tripwire over a curated crate list. Green does
not prove that no dependency can open a socket.

### Offline ORT build gate

Run this when the ORT version, features, or exception changes. First install
the normal gallery-core build prerequisites (including OpenSSL development
files), pre-provision a target-matched ONNX Runtime static library and the
locked Cargo sources, then disconnect the runner's network using the
platform's network namespace or interface control. From a clean checkout:

```
cd apps/gallery/core
export ORT_LIB_LOCATION=/absolute/path/to/target-matched/onnxruntime/lib
test -f "$ORT_LIB_LOCATION/libonnxruntime.a"
CARGO_NET_OFFLINE=true \
  CARGO_TARGET_DIR="${TMPDIR:-/tmp}/localfiles-ort-offline-target" \
  cargo build --locked -p gallery-ml
```

Remove the temporary target directory before repeating the gate so
`ort-sys`'s build script is exercised. No model pack is needed for this
compile-only probe.

This is deliberately a documented review gate rather than CI: the repository
does not ship a target-specific ONNX Runtime archive, obtaining that archive
would perform the very network download under test, and reliable interface
isolation is runner-specific. A fake library would only make the probe pass
the linker and would not validate the documented override.

## Source — Milestone A (Phase 1)

```
python3 conformance/source/check.py
python3 conformance/source/check.py --self-test
```

Production Swift/Go must not import `FileProvider` or `MetricKit`, or
name `NSFileProvider*`, `ubiquitousItem*`, or `MXMetric*`. Generated
`GalleryCore.swift`, the Linux UniFFI shim, `vendor/`, and
`apps/health/reference/web-ui/` are excluded.

`.github/workflows/conformance.yml` runs this next to the graph check.
Crate tests are a separate gate: `.github/workflows/rust.yml`
(`localcore` and `gallery-core`).

## Display records — ADR 0003 R6

```
python3 conformance/r6/check.py                  # exit 1 while red
python3 conformance/r6/check.py --expect-violations
python3 conformance/r6/check.py --self-test
```

`conformance/r6/` walks `apps/gallery/core/gallery-ffi/src/**/*.rs` and
enumerates `uniffi::Record` plus other exported types. A Record is
green only when every field is a display-ready scalar for exactly one
ADR 0004 slot kind. The taxonomy is
[`docs/spec/adr/0003-r6-surface.md`](../docs/spec/adr/0003-r6-surface.md).

It starts **red**. Today's gallery FFI ships `ScanPhoto`,
`MemoryGenerationInputs`, face/cluster records, and the rest of the
domain surface. That is the honest state until a later rewrite.
`expected.txt` is the inventory. CI asserts this exact red so a *new*
Record cannot hide behind the known ones.

`--expect-violations` with no names reads `expected.txt`. Named
arguments override the file (same shape as the graph check). The flag
is dropped when the surface goes green.

## Later

AST checks land here as later phases make their requirements
mechanically true.
