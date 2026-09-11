# archive

Lives at `apps/health` in the localfiles monorepo. Commands below are
from that directory.

A local-only, single-user, append-only personal health archive. Apple Health
exports are the historical baseline and the canonical type vocabulary. Nothing
in this program talks to the network.

Design: [docs/plan.md](docs/plan.md). Backup and recovery:
[docs/durability.md](docs/durability.md).

## Design

- **Append-only event log.** Corrections are new `supersede` / `retract` events. Log lines are never rewritten.
- **Content-addressed blobs.** Bulk sensor data stays in the source file. An import appends one `blob_import` event.
- **Disposable SQLite projection.** `derived/archive.db` is rebuilt from `log/` + `blobs/`. No migrations.
- **Offline.** UI assets are `//go:embed`ed. No cloud APIs, telemetry, or CDN.
- **Apple identifiers as vocabulary.** Every other source maps into that namespace. No parallel taxonomy.

## Build

Requires Go 1.25+, a C compiler (cgo), and the vendored tree
(`GOFLAGS=-mod=vendor`). SQLite is the [sqlite.org](https://sqlite.org)
amalgamation via [`github.com/mattn/go-sqlite3`](https://github.com/mattn/go-sqlite3).
Build **on the machine you will run on**. Do not cross-compile.
The binary links the platform C library (libSystem on macOS, glibc on Linux).

Export these for every `go build` / `go test`:

```bash
export CGO_ENABLED=1
export GOFLAGS=-mod=vendor
```

### macOS (Apple Silicon)

Xcode Command Line Tools provide `clang`:

```bash
xcode-select -p >/dev/null || xcode-select --install
go build -o archive ./cmd/archive
```

### Linux (arm64 and x86_64)

Including the Mecha Comet (arm64). Same packages on both architectures.
`gcc` is enough: the driver compiles the sqlite.org amalgamation and does
not need a distro `sqlite-devel` / `libsqlite3-dev`.

Debian / Ubuntu:

```bash
sudo apt-get install -y build-essential
go build -o archive ./cmd/archive
```

Fedora:

```bash
sudo dnf install -y gcc
go build -o archive ./cmd/archive
```

## Quick start

```bash
go build -o archive ./cmd/archive

./archive import -dev laptop export.zip   # hash into blobs/; one blob_import
./archive rebuild                         # reconstruct derived/archive.db
./archive serve                           # read-only UI on 127.0.0.1:8080
./archive query                           # events from the projection
./archive observations -kind StepCount -on 2026-09-01
./archive fsck
./archive export -out /tmp/archive-copy
```

`-root` defaults to `$ARCHIVE_ROOT` or `./archive`.

```
usage: archive [-root DIR] <command> [args]

commands:
  append        write an event to the log
                  -dev -type [-body]
  import        hash a file into blobs and record blob_import
                  -dev [-name] FILE
  rebuild       reconstruct derived/archive.db from log + blobs
  query         read events (or observations/episodes via extra arg)
  observations  read projected records
                  -kind -source -on -from -to -sum -by-source -summary
  episodes      read projected workouts/summaries
  kinds         list observation and episode kinds
  sources       list observation sourceNames
  stats         counts and observation date range
  export        write a portable copy of the archive to -out DIR
  fsck          verify blob hashes and references (read-only)
  serve         read-only web UI on 127.0.0.1 (default :8080)
```

Local calendar dates (`-on` / `-from` / `-to`) use `$ARCHIVE_TZ` (IANA name or
`+0200`), otherwise the process local zone.

## Configuration

| Source | Role |
|---|---|
| `$ARCHIVE_ROOT` | Archive directory (default `./archive`) |
| `$ARCHIVE_TZ` | Display timezone for local calendar dates |
| `config/*.toml` in this repo | Embedded defaults: `kinds.toml`, `ui.toml`, `projection.toml`, `sources.toml` |
| `<archive root>/config/<name>.toml` | Per-archive overrides of those files |

Configuration is read from those two places only, so a rebuild depends on
nothing but the archive root and the binary.

## Privacy and security

- `archive serve` binds `127.0.0.1` only, rejects non-localhost `Host` headers
  (DNS rebinding), sets CSP and other security headers, and uses HTTP timeouts.
- Archive files are written mode `0600`, directories `0700`.
- The UI appends a local usage log at `<root>/derived/uiusage.log` (path and
  query per request, mode `0600`, rotated at 4 MiB). It is never transmitted.
  Delete that file (and `uiusage.log.1` if present) to clear it. Exclude
  `derived/` from backup; see [docs/durability.md](docs/durability.md).
- `archive/` is gitignored. Never commit a live archive, a real Apple export,
  or real GPX.
- Fixtures under `testdata/apple` are structural slices of a real export with
  identifying values substituted (DOB, sex, device identifiers, source names,
  timezone, body metrics) and GPX coordinates shifted by an undisclosed constant offset (route geometry preserved, location not).

## Testing

Silent corruption is the failure mode this project is built against.

- Golden-file tests against anonymized fixtures in `testdata/`.
- Two consecutive rebuilds must produce byte-identical databases.
- Re-importing a blob yields zero new events and zero DB changes.

```bash
CGO_ENABLED=1 GOFLAGS=-mod=vendor go test ./...
```

## Releasing

Tags are `vMAJOR.MINOR.PATCH`. A tag whose commit lacks a matching
`CHANGELOG.md` heading fails CI, so do not `git tag` by hand.

```bash
./scripts/release.sh v0.2.0        # draft ## [0.2.0] from git log; does not commit
$EDITOR CHANGELOG.md               # rewrite into Added/Changed/Fixed, drop noise
./scripts/release.sh v0.2.0 --tag  # commit CHANGELOG.md and create the annotated tag
git push origin HEAD v0.2.0
```

The first command only inserts commit subjects so you have something to edit.
CI still requires the heading on the tagged commit; it does not care whether
the bullets came from git log or from you. Bump `internal/version.Version`
before `--tag` if the portable-export version should match.

Pushing the tag runs `.github/workflows/release.yml`: native builds on
macOS/arm64, Linux/arm64, and Linux/x86_64, then a GitHub Release whose notes
are that changelog section.

## Layout

```
archive/                 # data, gitignored — never commit
  log/<device>/YYYY-MM.ndjson
  blobs/sha256/ab/cd/<hash>
  derived/archive.db
  derived/uiusage.log
  config/                # optional per-archive TOML overrides
cmd/archive/
config/                  # embedded defaults
internal/
  adapters/apple/
  blobs/  event/  log/  portable/  projection/
  ui/                    # Chart.js + Recursive, embedded
  uuid/  version/
docs/plan.md
docs/durability.md
CHANGELOG.md
scripts/release.sh       # writes CHANGELOG + annotated tag
testdata/                # anonymized fixtures
vendor/                  # mattn/go-sqlite3 + sqlite.org amalgamation
```

## Status

Implemented: event log, blob store, UUIDv7, Apple Health adapter (Records,
Correlations, Workouts including GPX routes, ActivitySummary), SQLite
projection, gaps report, portable export / restore / fsck, read-only web UI.

Not implemented: Garmin Instinct 1 Solar FIT adapter (`muktihari/fit`),
dedicated medication/meditation commands (generic `append` only), local lab
PDF extraction, sync.

## License

MIT. See [LICENSE](LICENSE). Third-party notices:
[THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
