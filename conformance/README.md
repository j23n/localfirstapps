# Conformance harness

Structural checks against the ADRs. Review time is the bottleneck; this
reduces it. It does not remove it (ADR 0007 R16).

The **first** check is the one a source grep cannot do.

```
python3 conformance/graph/check.py
python3 conformance/graph/check.py --self-test
```

## Graph — ADR 0002 R13

`conformance/graph/` walks every core lockfile that exists —
`core/Cargo.lock` and `apps/gallery/core/Cargo.lock` — and unions
findings by package name. Preferring only `core/` would hide a
networking crate that lives only in the extracted workspace.
`--lockfile` is a single-file override.

The allowlist has one entry: `ort` / `ort-sys`, build-time, offline
override `ORT_LIB_LOCATION`.

The graph is **green**. Place names come from `localcore-geo` (packed
gazetteer + admin-0 polygons). `gallery-geo` and its `ureq` client are
gone. `--expect-violations` remains for pinning a known-red graph; it
is not used here.

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
