# Conformance harness

Structural checks against the ADRs. Review time is the bottleneck; this
reduces it. It does not remove it (ADR 0007 R16).

The **first** check is the one a source grep cannot do.

```
python3 conformance/graph/check.py                  # exit 1 while red
python3 conformance/graph/check.py --expect-violations gallery-geo
```

## Graph — ADR 0002 R13

`conformance/graph/` walks `core/`'s resolved `Cargo.lock` against a
written allowlist. The allowlist has one entry: `ort` / `ort-sys`,
build-time, offline override `ORT_LIB_LOCATION`.

It starts **red**. `gallery-geo` depends on `ureq` (Nominatim). That is
the honest state until Phase 2 ships `localcore-geo` and deletes the
client. CI asserts this exact red so a *second* networking crate cannot
hide behind the known one.

`--expect-violations` is dropped when the graph goes green.

## Source — Milestone A (Phase 1)

```
python3 conformance/source/check.py
python3 conformance/source/check.py --self-test
```

Production Swift/Go must not import `FileProvider` or `MetricKit`, or
name `NSFileProvider*`, `ubiquitousItem*`, or `MXMetric*`. Generated
`GalleryCore.swift`, the Linux UniFFI shim, `vendor/`, and
`apps/health/reference/web-ui/` are excluded. Rust `Vfs::probe_provider`
stays until Phase 2.

`.github/workflows/conformance.yml` runs this next to the graph check.

## Later

AST checks land here as later phases make their requirements
mechanically true. The type-system guard (ADR 0003 R6) is Phase 2.
