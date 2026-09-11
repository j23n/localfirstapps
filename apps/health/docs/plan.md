# Personal Health Archive — Design

**Goal:** a local, vendor-free health record the user owns. Apple Health export is the historical baseline and the canonical vocabulary. Garmin Instinct 1 Solar FIT files, blood tests via local PDF extraction, and medication and meditation logging complete the record. Linux, EU, no cloud, **no network required to use the app**.

**Language:** Go. **Implementation:** primarily by coding agents, under the constraints in `CLAUDE.md`.

What is implemented and what is not is listed in `README.md` → Status.

---

## 0. Principles

| Priority | Because |
|---|---|
| **Ingest reliability** is feature #1 | A gap you didn't notice is unrecoverable. The watch overwrites. |
| **The log is the record; the DB is disposable** | Rebuild beats migrate. Sources are re-parsed many times. |
| **Apple's vocabulary is canonical** | A comprehensive documented taxonomy exists; a second one is strictly more work with less coverage. |
| **Verification over trust** | Agents write plausible, subtly wrong code. Golden files catch it. |
| **Minimal, frozen, vendored dependencies** | Custody applies to the supply chain too. |
| **Provenance on every value** | Sources overlap. Untraceable numbers aren't a record. |
| **Nothing is edited, only superseded** | Keep what the model extracted, not just the correction. |
| **Network is never in the app's path** | Full function offline, always. Sync is opportunistic, or absent. |
| **One static binary** | Portability and packaging are requirements. |

---

## 1. Language and packaging

### Go

| Requirement | Why Go |
|---|---|
| Easy to package | Single static binary. No runtime, no interpreter. |
| Portable | Native `go build` on each machine (Mac/arm, Linux/arm, Linux/x86). |
| FIT parsing | `muktihari/fit` — see §5. |
| 2 GB XML | `encoding/xml` streaming `Decoder`. Constant memory. |
| SQLite | `github.com/mattn/go-sqlite3` — cgo, sqlite.org amalgamation. Trusted C, build on the target. |
| Local model | Ollama over HTTP. No bindings. |
| Small attack surface | Stdlib-heavy. Few dependencies. Stable API surface that agents model accurately. |

**This choice is contained.** The log is the contract; the language only implements the projection. If the language changes, the archive survives untouched — rewrite the projection and rebuild.

### Packaging

| Target | Approach |
|---|---|
| Linux desktop (x86 or arm) | Native `CGO_ENABLED=1 go build`. Needs `gcc` (Debian/Ubuntu `build-essential`, Fedora `dnf install gcc`). |
| macOS Apple Silicon | Native `CGO_ENABLED=1 go build`. Needs Xcode CLT (`clang`). |
| Mecha Comet | Native `CGO_ENABLED=1 go build` on the device (or another linux/arm64). No cross-compile. |
| UI assets | `//go:embed` the HTML/CSS/JS. **No CDN, no node_modules, offline by construction.** |
| Android (out of scope for this repo) | A separate app appending NDJSON into a synced folder. No shared code. |

---

## 2. Two surfaces, one binary

**`archive` — the CLI.** Ingestion, projection, rebuild, export, fsck. Desktop-only: needs a USB mount, a real filesystem, and (for labs) Ollama.

**Web UI (preserved, not shipped).** `reference/web-ui/` is the Phase 6
brief. `archive serve` is no longer a product command.

> **The append-only log is a language-agnostic contract.** Any program that appends valid NDJSON into `log/<device>/` participates. No FFI, no shared code.

---

## 3. Architecture

```
archive/
  log/
    laptop/2026-09.ndjson      # append-only, one JSON object per line
    comet/2026-09.ndjson       # per-device file = no write conflict, ever
  blobs/
    sha256/ab/cd/abcd1234…     # Apple export zips, FIT files, lab PDFs
  derived/
    archive.db                 # SQLite projection. Rebuildable. Never synced.
    uiusage.log                # local UI usage log. Disposable.
  config/                      # optional per-archive TOML overrides
```

**Tier 1, the log.** Only things the user *asserts*: a dose taken, a meditation session, a verified lab value, a correction, the fact that a file was ingested. Thousands of lines a year. Greppable.

**Tier 2, blobs.** Raw source files, content-addressed by SHA-256. Immutable.

**Tier 3, derived.** SQLite, rebuilt by replaying log + blobs. Deterministic: two rebuilds of the same root are byte-identical. Config that affects the projection is read only from embedded defaults and `<archive root>/config/`, never from the working directory or environment, so the root alone determines the output.

> **Hard rule:** bulk sensor data never enters the log. Apple's millions of `Record` elements and Garmin's HR samples stay inside their blobs. Ingesting a source appends *one* `blob_import` event.

**Why the disposable DB earns its keep:** sources are re-parsed repeatedly — undocumented Garmin fields, Apple dedup rules, lab extraction quality. Each is a rebuild, not a migration.

### Conflict model

Events are immutable and every device writes its own files, so concurrent writers cannot conflict. No CRDT, no merge logic. Corrections are `supersede` / `retract` events applied last-writer-wins by timestamp.

### Sync — optional, never required

- **Sneakernet.** Copy `log/` and `blobs/` to a USB stick; union the files. Valid because everything is immutable.
- **Syncthing**, if wanted. Sync `log/` and `blobs/`, exclude `derived/`.

---

## 4. Adapter: Apple Health — the load-bearing source

### Canonical vocabulary

Apple's type identifiers **are the archive's vocabulary.** `HKQuantityTypeIdentifierHeartRate`, `HKCategoryTypeIdentifierSleepAnalysis` and friends are `observation.kind` values. Garmin FIT fields map *into* this namespace.

### What's in the export

DTD declares `HealthData (ExportDate, Me, (Record|Correlation|Workout|ActivitySummary|ClinicalRecord)*)`.

| Element | Contents |
|---|---|
| `Record` | The bulk. `type`, `unit`, `value`, `sourceName`, `sourceVersion`, `device`, `creationDate`, `startDate`, `endDate`, nested `MetadataEntry` and `HeartRateVariabilityMetadataList` |
| `Workout` | With `WorkoutEvent`, `WorkoutStatistics`, `WorkoutRoute`, metadata |
| `ActivitySummary` | Daily rings |
| `Correlation` | Food entries, blood pressure pairs |
| `Me` | DOB, biological sex, blood type |
| `ClinicalRecord` | Empty for EU accounts (no Cures Act API) |

Alongside `export.xml`: `workout-routes/*.gpx`, ECG voltage CSVs, State of Mind entries (iOS 17+).

**The export does not contain the medication log** (verified against HealthKit Export Version 14, XML and CDA). Medication history is entered manually as events.

### Four gotchas, all load-bearing

**Correlation children are duplicates.** Apple's own DTD comment states records appearing as children of a Correlation *also* appear as top-level records. The adapter skips the nested ones.

**Never validate against the embedded DTD.** Apple's DTD can be out of sync with the document it ships with (`WorkoutStatistics`, `CardioFitnessMedicationsUse`). The adapter parses permissively with `xml.Decoder.Token()`.

**Cross-source duplication.** iPhone and Watch both record steps; `sourceName` distinguishes them and naive aggregation double-counts. Precedence is an explicit table (`config/sources.toml`), never a heuristic.

**Snapshot, not increment.** Each export overlaps almost entirely with the last. One blob, one event — the projection dedups at record level. Apple gives no stable per-record UUIDs, so the synthetic key is `hash(type, sourceName, startDate, endDate, value, sorted MetadataEntry pairs)`. The key identifies a **group**: insert `max(0, count_in_this_export − count_already_in_db)` rows. Metadata is in the hash because identical type/time/value HeartRate samples can differ only by `HKMetadataKeyHeartRateMotionContext`.

**Plus:** 200 MB–2 GB. Streaming only.

### Known omissions

- Medication log — see above.
- Some category severities are exported without a value (reported for `HKCategoryTypeIdentifierHotFlashes`). Spot-check types you care about.
- No API guarantee. Apple can change the format without warning — another reason the raw zip stays an immutable blob forever.

---

## 5. Adapter: Garmin Instinct 1 Solar (not implemented)

### Library: `muktihari/fit`

Not the official SDK, not `tormoder/fit` (unmaintained since Sept 2024; its README points to muktihari).

`muktihari/fit` preserves message arrival order and **unknown messages** — exactly what the undocumented Monitor/Sleep fields need. FIT Protocol V2, profile generated from `Profile.xlsx`.

`FitCSVTool.jar` is the cross-check oracle.

### Files

| Dir | Contents | Retention on device |
|---|---|---|
| `Activity/` | One file per workout | ~200 files |
| `Monitor/` | All-day metrics, several/day | ~75 files |
| `Metrics/` | Training/physiological | ~100 files |
| `Sleep/` | Per-night | **~20 nights** |

**Documented:** activities (GPS, HR, pace, cadence, laps, altitude, temp), all-day HR (~2 min), steps/calories/floors/intensity minutes, `current_activity_type_intensity`, `stress_level`, sleep stages, SpO2.

**Present but undocumented:** Body Battery, VO2 max — computed on-device by Firstbeat. Identified by correlating a raw field number against the watch UI.

**Not on Instinct 1** (gen-2+ only): HRV status, respiration rate, Health Snapshot, sleep score.

### The honest limit

**No FIT library reverse-engineers the undocumented fields.** They track Garmin's published profile. The knowledge lives in forum threads (GoldenCheetah users decoded session field 193 as RPE×10 and 192 as Feel by correlation).

Findings go in `notes/fields.md` and are mapped in the projection only. The rebuildable DB makes guessing safe.

### Transfer

USB mass storage. Gadgetbridge's `garmin-bridge` supports GFDI V2 devices (Instinct 2 and later), not the Instinct 1.

---

## 6. Event catalogue

```json
{"id":"<uuidv7>","ts":"2026-09-04T08:12:03.000000000Z","dev":"laptop",
 "type":"med_event","body":{"med_id":"…","taken":true}}
```

`ts` is UTC with exactly nine fractional digits; the projection orders by `(ts, id)`.

| Event type | Body | Notes |
|---|---|---|
| `blob_import` | `sha256`, `size`, `name`, `kind` | One per Apple export zip / FIT file / lab PDF. `kind` is the adapter (`apple`, …). Readers also accept the aliases `blob`=`sha256` and `orig_filename`=`name`. |
| `observation` | `kind`, `value`, `unit`, `observed_at` | Manual only. Apple Records stay in the blob. `kind` uses Apple identifiers. |
| `med_start` | `name`, `dose`, `unit`, `schedule`, `from` | |
| `med_stop` | `med_id`, `on` | |
| `med_event` | `med_id`, `taken` | |
| `meditation` | `minutes`, `started_at`, `kind` | |
| `note` | `text`, `about_date` | |
| `extraction` | `blob`, `model`, `raw_json` | What the model *said*. Unverified by definition. |
| `supersede` | `target`, `body` | |
| `retract` | `target`, `reason` | |

**Corrections are events, not edits.** `verified` and `deleted_at` are not columns. A retracted or superseded `blob_import` is not projected.

### Projection tables — all derived, all rebuildable

The live schema is in `internal/projection`; this is its shape.

```sql
events(id, ts, dev, type, body, superseded_by, retracted)
blobs(sha256, size, name)
observations(dedup_key, n, kind, source, source_version, device,
             start_ts, end_ts, start_offset, end_offset, unit, value,
             metadata, blob_sha256, event_id)
episodes(dedup_key, kind, source, start_ts, end_ts, start_offset, end_offset,
         body, blob_sha256, event_id,
         elapsed_seconds, moving_seconds, pause_count,
         route_segments, route_points, route_bbox, route_polyline,
         ascent_m, descent_m)
workout_event(episode_id, seq, kind, time_utc, …)
source_precedence(source_name, rank)
attachments(blob_sha256, path, size)
apple_exports(sha256, locale, export_date, me)
```

`blob_sha256` + `event_id` on every observation/episode is the provenance rule: a number without a source file is not a record. `source_precedence` is global per `sourceName`; per-kind ranks are a projection change when Apple/Garmin overlap requires them.

Medication and lab tables (`medication`, `med_event`, `document`) belong to the manual-input and lab milestones and do not exist yet.

---

## 7. Dependencies — frozen list

| Need | Use |
|---|---|
| FIT parsing | `github.com/muktihari/fit` |
| DB | `github.com/mattn/go-sqlite3` |
| Everything else | **Go stdlib** |
| Frontend | Chart.js, vendored and embedded |
| External tools (not linked) | `restic`, Ollama, `FitCSVTool.jar`, Syncthing |

**This list is closed.** Adding a dependency requires explicit human approval — not an agent's judgement call.

**Written in stdlib, not imported:** JSON encoding, SHA-256 hashing, XML streaming, HTTP serving, asset embedding, UUIDv7 (~20 lines over `crypto/rand`), a minimal TOML reader for `config/*.toml`.

**Never write:** a FIT parser, an XML parser, a CRDT, a sync engine, an ORM, an auth system.

---

## 8. Supply-chain posture

- **Vendor everything.** `go mod vendor`, committed. Builds are hermetic and offline — "no network" is a build property, not just a runtime one. `GOFLAGS=-mod=vendor`.
- **Pin exact versions.** No version ranges. Upgrades are deliberate, reviewed, and land in their own commit.
- **`govulncheck` in CI.** Official Go tooling, pinned version.
- **Frontend assets are vendored files with a hash-assertion test**, not CDN links. A test that fails when the bytes change is how tampering is noticed.
- **Go's module checksum database** gives tamper-evidence on `go.sum`. Left on.
- **CI actions are pinned to commit SHAs** and run with `contents: read`.

**SQLite is the sqlite.org amalgamation compiled with cgo** (`mattn/go-sqlite3`). That is a C compiler at build time and a native build per OS/arch. It is preferred over a Go transliteration of SQLite: the C is what the rest of the world runs. Pin the module, vendor it, never auto-upgrade.

---

## 9. Verification apparatus

Agents write plausible, subtly wrong code that compiles and passes shallow tests. For an archive, silent corruption is the worst outcome. **This section is the primary quality mechanism — it matters more than the language choice.**

**Golden-file tests.** Small real fixtures — a trimmed Apple export, and one file from each Garmin directory once that adapter exists — with exact parsed output asserted. Fixtures are structural slices of real exports with identifying values substituted; see `notes/apple.md`.

**Rebuild determinism in CI.** Two consecutive rebuilds produce byte-identical databases. A test, not a habit.

**Idempotency tests.** Re-import the same blob; zero new events and zero DB changes.

**Dedup tests with real overlap.** Two Apple exports taken weeks apart, overlapping. No double-counting. The same for an Apple/Garmin overlapping date range.

**Manual spot-checks — the part no test replaces.** Pick ten days and compare the projection against what the Health app and watch display. **Units errors and timezone offsets pass every type check and every unit test.**

**Timestamp rollover test.** Explicit test for `timestamp_16` wraparound with a fixture that crosses the boundary (with the FIT adapter).

---

## 10. Local UI

Read-only. Same binary. `//go:embed`ed Chart.js and the Recursive font.

- Binds `127.0.0.1` only; refuses any other address.
- Rejects requests whose `Host` is not `127.0.0.1` / `localhost` and requests with `Sec-Fetch-Site: cross-site` (DNS-rebinding defence).
- Content-Security-Policy with a per-request nonce, `X-Content-Type-Options`, `Referrer-Policy`, `X-Frame-Options`.
- HTTP read/write/idle timeouts. Internal errors are logged to stderr; responses carry a generic message.
- Appends a local usage log at `derived/uiusage.log` (path + query, `0600`, rotated at 4 MiB). Disposable; never transmitted.

---

## 11. Lab extraction (not implemented)

- **Local model** (Ollama + vision). A cloud API, if ever, is a deliberate per-document choice with logged consent.
- **Strict JSON**, one object per analyte: `{name, value, unit, reference_range, collected_date}`. Reject and retry rather than accepting partials.
- **Always keep the original** blob. Extraction quality improves; re-runs are expected.
- **Confirmation UI is not optional.** PDF page beside parsed values; confirm or correct; appends the `supersede`.
- **Normalise unit names, never values.**

---

## 12. Safety

- **LUKS** covers the stolen laptop. SQLCipher only if the archive travels. No custom crypto.
- **`restic`/`borg`** to encrypted offsite storage, user-held key, restore tested. See `docs/durability.md`.
- **Keep `archive/` out of `~/Dropbox`, `~/Nextcloud`.** The CLI warns when the root sits under a cloud-sync folder name.
- **Nothing binds beyond `127.0.0.1`.**
- **Archive files are `0600`, directories `0700`.**
- **Deletion is deliberate, and that's a real trade.** `retract` hides a value; it doesn't remove it. Genuine erasure means rewriting log files by hand. Defensible for an archive — but a conscious choice.
- Personal record, not a medical device.

---

## 13. Gotchas that silently corrupt the timeline

**`timestamp_16` rollover (Garmin).** Monitor files use 16-bit truncated timestamps:

```go
absolute = base + ((t16 - base) & 0xFFFF)
```

Scrambles data without erroring. Tested explicitly.

**Timezone.** FIT timestamps are UTC from a 1989-12-31 epoch. Apple exports carry offsets in the timestamp strings. Everything is stored UTC and converted at display; the original offset is kept alongside (`start_offset`, `end_offset`).

**Chained FIT files.** Monitor files can be concatenated. One file is not necessarily one stream.

**Torn log tail.** A crash between write and fsync can leave a partial final line. Reads report it as a distinct error (`torn tail`, with path and offset) and never repair it.
