package portable

// readmeText is the static export README. It must not include a timestamp
// so two exports of an unchanged archive are byte-identical except
// manifest.json's exported_at field.
const readmeText = `# Health archive portable export

This directory is a self-contained copy of a personal health archive.
You do not need the original program, Go, or SQLite to read it.

## What is here

- ` + "`events.ndjson`" + ` — every event from every device, one JSON object per line, sorted by timestamp then id.
- ` + "`blobs/`" + ` — the raw source files. Layout is content-addressed: ` + "`blobs/sha256/ab/cd/`" + ` followed by the 64-character lowercase SHA-256 hex digest of the file.
- ` + "`manifest.json`" + ` — format version, when this export was written, event and blob counts, per-blob SHA-256, and the tool version that wrote it.
- ` + "`README.md`" + ` — this file.

## Events

Each line is one event:

` + "```json" + `
{"id":"<uuidv7>","ts":"<UTC RFC3339>","dev":"<device>","type":"<type>","body":{}}
` + "```" + `

- ` + "`id`" + ` is a UUIDv7. It never changes; corrections do not edit this line.
- ` + "`ts`" + ` is always UTC (suffix ` + "`Z`" + `).
- ` + "`dev`" + ` is the device that wrote the event (` + "`laptop`" + `, ` + "`manual`" + `, …).
- ` + "`type`" + ` is one of: ` + "`blob_import`" + `, ` + "`observation`" + `, ` + "`med_start`" + `, ` + "`med_stop`" + `, ` + "`med_event`" + `, ` + "`meditation`" + `, ` + "`note`" + `, ` + "`extraction`" + `, ` + "`supersede`" + `, ` + "`retract`" + `.
- ` + "`body`" + ` is a JSON object. For ` + "`blob_import`" + ` it includes ` + "`sha256`" + `, ` + "`size`" + `, ` + "`name`" + `, and sometimes ` + "`kind`" + ` (` + "`apple`" + ` for an Apple Health export). ` + "`supersede`" + ` and ` + "`retract`" + ` include ` + "`target`" + `, the ` + "`id`" + ` of the event they correct.

The log is append-only. A correction is a new ` + "`supersede`" + ` or ` + "`retract`" + ` line, never an edit of an earlier line.

## Three tiers

The live archive on disk is three directories. Only the first two are the record.

1. **log/** — the event files, split per device and month. This export concatenates them into ` + "`events.ndjson`" + `.
2. **blobs/** — immutable source files, hashed. Copied here as-is.
3. **derived/** — a SQLite database rebuilt from log + blobs. It is **not** part of this export and must not be treated as the record.

` + "`derived/`" + ` is a cache. Throw it away and rebuild.

## Blobs

A blob is the original file, byte-for-byte: Apple Health export zips, Garmin FIT files, lab PDFs, and anything else that was ingested. The filename *is* the SHA-256 of the contents. If they disagree, the file is corrupt.

Bulk sensor samples stay inside these files. The event log only records that the file was ingested (` + "`blob_import`" + `).

## Checking integrity

For each entry in ` + "`manifest.json`" + ` → ` + "`blobs`" + `, hash the file at ` + "`blobs/sha256/<first two hex>/<next two>/<full hash>`" + ` and confirm it matches. ` + "`event_count`" + ` should equal the number of non-empty lines in ` + "`events.ndjson`" + `.
`
