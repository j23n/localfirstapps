# Durability

Backup of the machine is a whole-laptop restic job. This document does not
configure restic. It says what that backup must keep, what it must skip, and
how to recover the archive after a restore.

## What must be preserved

Two directories under the archive root (`$ARCHIVE_ROOT`, default `./archive`):

| Path | Why |
|---|---|
| `log/` | The append-only event record. Per-device monthly NDJSON. This is the timeline. |
| `blobs/` | Content-addressed source files (Apple export zips, FIT files, lab PDFs). Filename is the SHA-256 of the bytes. |

Together they are the archive. `config/` under the root holds optional
per-archive TOML overrides and is worth keeping but is not the record.
Everything else can be thrown away.

## What must not be preserved

**`derived/`** — disposable outputs such as the SQLite projection
(`derived/archive.db`). The projection is rebuilt from `log/` + `blobs/`.
Exclude the whole `derived/` directory from backup and sync.

Do not sync `derived/` (or the rest of the archive) through Dropbox, Nextcloud,
OneDrive, iCloud, or Google Drive. The CLI warns at startup if `$ARCHIVE_ROOT`
sits under one of those names.

All files under the root are written mode `0600`; directories `0700`.

### restic exclude

Add this line to the existing whole-laptop restic backup (one extra exclude;
do not add a second restic job for the archive):

```
--exclude $ARCHIVE_ROOT/derived
```

If the archive root is the default `archive` directory under the backup path,
that is:

```
--exclude archive/derived
```

`$ARCHIVE_ROOT` is the directory that contains `log/`, `blobs/`, and `derived/`.
Excluding `derived` by name alone (`--exclude derived`) also works if that
directory name is unique on the laptop; the path form above is the precise one.

## Recovery

1. Restore `log/` and `blobs/` from restic into `$ARCHIVE_ROOT`. Leave `derived/`
   absent or empty.
2. `archive -root $ARCHIVE_ROOT rebuild`
3. `archive -root $ARCHIVE_ROOT fsck`
4. Compare counts to the last known-good totals (events, blobs, observations,
   episodes). `fsck` prints event and blob counts; `archive stats` prints
   observation and episode counts from the rebuilt DB.

A portable `archive export -out DIR` is a second, lock-in-free copy
(`events.ndjson` + `blobs/` + `manifest.json` + `README.md`). It is not the
restic restore path. Recovery from restic uses the native `log/` + `blobs/`
layout. Restoring a portable export into a root that already contains some of
its events appends only the missing ones.

To recover from that portable form:

```bash
archive -root /path/to/restored restore -from /path/to/export
archive -root /path/to/restored rebuild
archive -root /path/to/restored fsck
```

`restore` keeps event ids, hashes every blob into the content-addressed store,
and is idempotent: running the same command again does not append duplicate
events. It does not repair or rewrite an existing log.

`archive fsck` is read-only. It never repairs. It reports:

- a blob whose bytes do not match its path,
- a `blob_import` whose blob is missing,
- a blob that no event references,
- a torn final log line (partial write with no trailing newline), with path and offset.

## Recovery drill

The test suite exercises the drill on fixtures: copy `log/` + `blobs/` only,
rebuild, fsck, and compare event / blob / observation / episode counts with the
source archive. Two consecutive rebuilds of the same root are byte-identical.
Run the same steps by hand against the live archive after any restore.
