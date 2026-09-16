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
`GalleryCore.swift`, the Linux UniFFI shim, and `vendor/` are excluded.
Health has no source-tree exception; its retired web implementation was
deleted and `apps/health/ui-spec/` contains static TOML/JSON only.

This is a regression check for host APIs retired at Milestone A, not a ban on
selecting a provider-backed folder whose bytes are already local. ADR 0005 R2
forbids app-initiated materialisation and provider lifecycle in the domain
model.

`.github/workflows/conformance.yml` runs this next to the graph check.
Crate tests are a separate gate: `.github/workflows/rust.yml`
(`localcore` and `gallery-core`).

## Human review controls — ADR 0007 R16

```
python3 conformance/r16/check.py
python3 conformance/r16/check.py --self-test
```

`conformance/r16/checklist.toml` is the machine-readable source of truth for
the requirements CI cannot decide. Stable `r16:` markers and exact prompts
bind it to `.github/pull_request_template.md`. The template requires prose
answers for shell-owned policy (ADR 0001 R4 / ADR 0003 R5), FFI display
readiness (ADR 0003 R6), adaptive native shell behavior (ADR 0004 R8/R9),
single app-core assertions and shared preservation fixtures (ADR 0007 R7/R8),
and unavailable real inputs (ADR 0007 R9).

The checker reads only files in the checkout. It has no dependency on a
pull-request event or GitHub API, so the same command can move unchanged into
a reusable workflow later.

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

## Semantic debt guard

```
python3 conformance/semantic/check.py
python3 conformance/semantic/check.py --self-test
```

`conformance/semantic/check.py` catches behavior that the API-spelling grep
and Record taxonomy do not:

- production Gallery `PhotoLocality`, `DownloadStatus`, related FFI fields,
  provider-placeholder branches, and QuickLook/materialization behavior;
- exported Contacts `vcard_text` / `save_vcard` whole-domain payloads;
- production Rust calls that use best-effort `Vfs.exists` for an
  authoritative decision instead of propagating `try_exists` errors.

Known debt is explicit in `conformance/semantic/baseline.toml`. Every entry
pins an exact category, path, symbol, and occurrence count, plus its rationale
and target wave. A new path/symbol or another occurrence fails; removing debt
makes the baseline stale so the removal and allowlist update land together.

Only production source is scanned. Separate `tests`/`*Tests`, fixtures,
benches, vendor/build output, Rust `cfg(test)` items, generated
`GalleryCore.swift`, and the generated Linux Swift shim are excluded.
Comments and string literals are masked rather than treated as semantics.

Contacts currently has no semantic-baseline exception: normal loads and
saves use typed drafts, while `export_vcard_text` is named and scoped as an
explicit export operation.

## Workflow

`.github/workflows/conformance.yml` runs both new checkers as checkout-local
jobs, including their self-tests. Their commands and inputs remain independent
of workflow event shape for later reusable-workflow conversion.
