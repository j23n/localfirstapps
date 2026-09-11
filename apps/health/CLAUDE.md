# CLAUDE.md

Personal health archive. Single user. Local-only. Go.

Ingests Apple Health exports (the historical baseline) into an append-only event log. Garmin Instinct 1 Solar FIT files, blood test PDFs, and manual medication/meditation entries are in scope and not implemented; see Status.

Design: `docs/plan.md`. Read it before proposing architectural changes.

---

## Hard constraints

Violating any of these is a bug, not a tradeoff.

1. **The log is append-only.** Never rewrite, edit, or delete a line in `log/`. Corrections are new `supersede` / `retract` events.
2. **Bulk data never enters the log.** Apple's millions of `Record` elements and Garmin's HR samples stay inside their blobs. Ingesting a source appends exactly ONE `blob_import` event.
3. **`derived/archive.db` is disposable.** Fully reconstructible from `log/` + `blobs/` by `archive rebuild`. No migrations — change the projection and rebuild.
4. **No network in any code path.** No cloud APIs, no telemetry, no CDN links, no runtime fetching. UI assets are `//go:embed`ed. The app must work fully offline.
5. **Nothing binds beyond `127.0.0.1`.**
6. **All timestamps stored UTC.** Convert at display only.
7. **Imports are idempotent.** Re-running produces zero new events and zero DB changes.
8. **Apple's type identifiers are the canonical vocabulary.** Garmin fields map into that namespace. Never invent a parallel taxonomy.
9. **The dependency list is frozen.** See below. Adding any dependency requires explicit human approval — never an agent's judgement call. If a task seems to need a new library, stop and ask.
10. **SQLite is upstream via cgo** (`github.com/mattn/go-sqlite3`). Build on the target (Mac/arm, Linux/arm, Linux/x86). Do not cross-compile.
11. **If a prerequisite input is missing, STOP AND ASK.** Never synthesize test inputs (fake FIT, invented export.xml, generated “real” fixtures). Trim from the actual file or wait.
12. **Fixtures are anonymized.** Never commit real Apple exports, GPX routes, or other identifying source files.

---

## Dependencies — frozen

| Need | Use |
|---|---|
| FIT parsing | `github.com/muktihari/fit` |
| DB | `github.com/mattn/go-sqlite3` (cgo, sqlite.org amalgamation) |
| **Everything else** | **Go stdlib** |
| Frontend | Chart.js, vendored, embedded |

**Write in stdlib, don't import:** JSON, SHA-256, XML streaming, HTTP serving, asset embedding, **UUIDv7** (~20 lines over `crypto/rand`).

**Do not use `tormoder/fit`** — unmaintained since Sept 2024; its own README points to `muktihari/fit`.

**Never write:** a FIT parser, an XML parser, a CRDT, a sync engine, an ORM, an auth system.

**Supply chain:** vendored (`go mod vendor`, `GOFLAGS=-mod=vendor`), exact pinned versions, `govulncheck` in CI. Frontend assets have a hash-assertion test — if the bytes change, the test fails. SQLite is cgo; `CGO_ENABLED=1`.

---

## Layout

```
archive/                     # data, gitignored, NEVER committed
  log/<device>/YYYY-MM.ndjson
  blobs/sha256/ab/cd/<hash>
  derived/archive.db
  derived/uiusage.log        # local UI usage log; delete to clear
  config/                    # optional per-archive TOML overrides
cmd/archive/                 # CLI entrypoint
config/                      # embedded defaults (kinds, ui, projection, sources)
internal/
  log/                       # event append + read
  blobs/                     # content-addressed store
  event/
  uuid/
  version/
  adapters/apple/            # streaming XML
  adapters/fit/              # not implemented: muktihari/fit
  adapters/labs/             # not implemented: Ollama vision extraction
  projection/                # log+blobs -> sqlite
  portable/                  # export, restore, fsck
  ui/                        # embedded Chart.js UI
    assets/                  # app.js, app.css, fonts/, vendor/chart
    templates/
docs/plan.md
docs/durability.md
notes/fields.md              # reverse-engineered FIT field findings
testdata/                    # anonymized fixtures — never commit real exports or GPX
vendor/
```

---

## Config and file permissions

Embedded defaults: `config/kinds.toml`, `ui.toml`, `projection.toml`, `sources.toml`.
Per-archive overrides: `<archive root>/config/<name>.toml`. Those two places are
the only config sources — never the working directory, never environment
variables — so a rebuild depends on the root and the binary alone. The only
environment variables are `$ARCHIVE_ROOT` (root path) and `$ARCHIVE_TZ`
(display zone).

All archive files are written `0600`; directories `0700`.

---

## Event format

One JSON object per line:

```json
{"id":"<uuidv7>","ts":"<iso8601 utc>","dev":"<device>","type":"<type>","body":{}}
```

Types: `blob_import`, `observation`, `med_start`, `med_stop`, `med_event`, `meditation`, `note`, `extraction`, `supersede`, `retract`.

`supersede` and `retract` carry `target` = the id of the event they act on. The projection applies them last-writer-wins by `ts`.

---

## Testing — this is the primary quality mechanism

Plausible-but-wrong parsing code compiles and passes shallow tests. For an archive, silent corruption is the worst outcome. Every adapter change ships with:

- **Golden-file tests** against anonymized fixtures in `testdata/` (structural slices of real exports; identifying values substituted). Assert exact parsed output. Never synthetic bytes. Never commit real exports or GPX.
- **Rebuild determinism**: two consecutive rebuilds produce byte-identical databases.
- **Idempotency**: re-importing a blob yields zero new events, zero DB changes.
- **Dedup with real overlap**: two overlapping Apple exports must not double-count. Same for overlapping Apple/Garmin date ranges.
- **`timestamp_16` rollover**: explicit test with a fixture crossing the boundary.

Units errors and timezone offsets pass every type check and every unit test. Flag them for human spot-checking rather than assuming correctness.

---

## Apple Health adapter

**Stream only.** Exports run 200MB–2GB. `xml.Decoder.Token()`, never load the document.

**Never validate against the embedded DTD.** Parse permissively with `xml.Decoder.Token()`. Apple's DTD can disagree with the document it ships with (`WorkoutStatistics`, `CardioFitnessMedicationsUse`). Do not assume the DTD matches the instance.

**Correlation children are duplicates.** Apple's DTD comment states records inside a `Correlation` also appear as top-level records. Skip the nested ones.

**Bare `export.xml` and zipped exports are both accepted.** Blob paths carry no extension; the adapter sniffs content (`PK\x03\x04` vs `<HealthData`).

**No stable record UUIDs.** Dedup key: `hash(type, sourceName, startDate, endDate, value, sorted MetadataEntry pairs)`. The key is a GROUP: insert only `max(0, count_in_export − count_in_db)` rows.

**Cross-source duplication.** iPhone and Watch both logged steps. Respect `source_precedence`; never sum blindly across `sourceName`.

**Snapshot semantics.** Each export overlaps almost entirely with the last. One blob, one event, record-level dedup in the projection.

---

## FIT adapter

**`timestamp_16` rollover.** Monitor files use 16-bit truncated timestamps:

```go
absolute = base + ((t16 - base) & 0xFFFF)
```

Silently scrambles data if wrong.

**Chained files.** Monitor files can be concatenated. Don't assume one file is one stream.

**Unknown messages are expected and important.** Garmin has stated Monitor and Sleep files contain undocumented fields they won't document. `muktihari/fit` preserves unknown messages — keep them, don't drop them. Record findings in `notes/fields.md`; map them in the projection layer only.

**Device is Instinct 1 Solar.** No HRV status, no respiration rate, no sleep score — gen-2+ only. Don't write code expecting them.

---

## Status

**Implemented:** event log, blob store, UUIDv7, Apple Health adapter (Records, Correlations, Workouts including GPX routes, ActivitySummary), SQLite projection, gaps report, portable export / restore / fsck, read-only web UI (`archive serve`). Backup is external (restic) — do not add backup tooling.

**Not implemented:** FIT importer (mount → blob → one `blob_import` → projection maps into Apple's vocabulary; Instinct 1 Solar only, no gen-2 fields), dedicated medication/meditation commands (generic `append` only), lab extraction (Ollama), sync.

---

## Conventions

- Go 1.25+, cgo + a C compiler, standard layout, `gofmt`, `go vet`, `govulncheck`
- Build on the target OS/arch. See README → Build.
- Tests use anonymized fixtures from `testdata/`, not synthetic bytes
- Prefer boring, readable code — stdlib over cleverness
