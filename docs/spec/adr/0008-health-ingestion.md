# ADR 0008: Health ingestion

- Status: Accepted
- Date: 2026-09-11

## Scope

Where localhealth's data comes from, and why the Apple Health export format
is no longer one of the answers.

The storage design is unchanged and is not restated here: an append-only
event log, content-addressed blobs, and a disposable SQLite projection
(ADR 0005 R1, R4, R6). This document specifies only ingestion.

## Requirements

**R1.** localhealth ingests from exactly three sources, each owned by the
platform that can reach it:

| Source | Ingested by | Mechanism |
|---|---|---|
| Apple Health | iOS shell | HealthKit, R2 |
| Garmin FIT files | Linux / Comet shell | watch mount → blob |
| Lab result PDFs | Linux / Comet shell | file import → blob |

Ingestion is a **host surface** (ADR 0007 R15): it is expected to differ per
platform, and a shell that cannot reach a source simply does not offer it.
Every shell reads the whole archive regardless of which device ingested it.

**R2.** **Apple Health is read from HealthKit, not from an export file.** The
iOS shell queries the system health store through an anchored, incremental
query and never parses `export.xml`. The streaming XML adapter is retired,
not ported.

**R3.** **Bulk data never enters the log** (ADR 0005 R6). An ingestion run
serialises the samples it read into a canonical NDJSON **blob**, stores it by
content hash, and appends **exactly one `blob_import` event** naming it. A
run that read nothing appends nothing.

Sample records are blob content. The log records that an ingestion happened.

**R4.** **The query anchor is per-device state.** It records this device's
position in this device's health store, is meaningless elsewhere, and MUST
NOT sync (ADR 0005 R5, second exception).

**R5.** **Deletions are events.** Where the source reports that a previously
seen sample was removed, ingestion appends a `retract` event naming it. This
is the one thing an export file cannot express and the reason the archive can
now be correct rather than merely cumulative.

**R6.** **Identity comes from the source.** Each sample carries the stable
identifier the health store assigns it, and that identifier is the dedup key.
The projection MUST NOT infer identity by hashing a sample's fields, and MUST
NOT dedup by counting group members.

**R7.** **Re-ingestion is idempotent** (ADR 0005 R1, tier 2). Running
ingestion twice with no intervening change appends no event and alters no
projection row.

**R8.** **Read authorisation is unobservable, and the archive says so.**
HealthKit does not report a denied read: absent data and withheld data are
indistinguishable by design. Therefore localhealth MUST NOT treat "no samples
of this type" as evidence of anything. The gaps report names every configured
type with no data since a stated date, and a person judges it. An app that
silently reported an archive as complete under a partial grant would be
worse than one with no gaps report at all.

**R9.** **Cutover is dated, not retroactive.** Existing observations derived
from `export.xml` imports remain in the log untouched (ADR 0005 R1: the log
is append-only). The first HealthKit ingestion is bounded to samples after
the end of the last export import, so the two eras do not overlap and cannot
double-count. The boundary date is recorded in the archive.

**R10.** **Apple's type identifiers remain the canonical vocabulary.** FIT
fields and lab results map into that namespace. No parallel taxonomy is
invented. This survives the change of source unaltered, because it was never
a property of the export format.

**R11.** Ingestion performs no network I/O (ADR 0006 R9). Reading a system
health store is local. That the operating system may itself have synchronised
that store from a vendor cloud is outside the app, exactly as the
synchroniser placing files in a folder is outside the app; the app originates
no request.

## Conformance

- No source parses XML, and no Apple export adapter exists in the tree.
- A fixture health store ingested twice appends one `blob_import` event and
  produces one set of projection rows.
- An ingestion run over an unchanged store appends nothing.
- The log contains no sample-level records; every sample is inside a blob.
- A deleted sample produces a `retract` event, and the projection drops it.
- The anchor is absent from the synchronised folder.
- Dedup is by source identifier; a test asserts two samples agreeing on every
  field but differing in identifier are both retained.
- The gaps report names a configured type with no data, and the wording
  distinguishes "none seen" from any claim about completeness.
- An archive holding pre-cutover export data and post-cutover HealthKit data
  double-counts nothing across the boundary date.

## Rationale

The export path was the riskiest component in the family, and it was risky
because of what Apple's export format discards. It has no stable record
identifiers, which is the sole reason the Go implementation carries a
group-hash dedup heuristic that inserts `max(0, count_in_export −
count_in_db)` rows; it cannot express a deletion, so the archive could only
grow; and each export overlaps almost entirely with the last, so every import
paid full dedup cost over a 200 MB–2 GB file. Porting 1,754 lines of
streaming XML parser to a second language — for an archive whose own
documentation says silent corruption is the worst possible outcome — bought
nothing except a faithful reproduction of those three problems.

Reading the store directly removes all three at once. Samples arrive with
identifiers, so R6 replaces a heuristic with a key and R7 becomes true by
construction rather than by argument. Deletions arrive, so R5 can exist.
Anchored queries return only what changed, so ingestion is proportional to
new data rather than to history. And a first run with no anchor returns the
full history, so nothing is lost by retiring the export.

R3 is the constraint this change had to be shaped around rather than
through. A decade of samples is millions of records; as log lines that is
roughly a gigabyte of append-only, permanently-synchronised state. Batching
into content-addressed blobs keeps the original design exactly — one blob,
one event — and changes only who produces the blob and how large it is.

R8 is the honest cost. The export was a single artefact a person could look
at; HealthKit is a permission surface that declines silently. The archive
cannot detect that, so it must not claim otherwise, and the gaps report
becomes load-bearing rather than a convenience.

The division in R1 is a benefit rather than a concession. iOS reaches the
health store; a laptop reaches a mounted watch and a scanner's output. Both
shells now have work only they can do, which is a better argument for
building both than uniformity was.
