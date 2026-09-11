# ADR 0005: Files, state tiers, conflicts, and reconciliation

- Status: Accepted
- Date: 2026-09-11
- Revised: 2026-09-11 (r2)

## Scope

What lives on disk, which parts sync, what happens when two devices disagree,
and how agreement is reached.

## Requirements

### Tiers

**R1.** All persistent state is exactly one of two tiers.

**Tier 1 — user-facing files.** The photos, `.xmp` sidecars, `.vcf`, audio,
`.m3u`. Foreign tools read and write these, so the format is not ours to
choose. Read-mostly; writes are preservation-first (ADR 0003 R2) and occur
only as the direct result of a user action.

**Tier 2 — app-owned state.** Work queues, people bookkeeping, playback
position, hidden and featured sets, the health event log. Nobody else reads
these, so the format is ours, and it is R3's.

**R2.** **Files are on disk or they do not exist.** An app reads a plain
directory tree. There are no placeholders, no download states, no
materialisation, no remote badges, and no per-file attribute that requires a
daemon, an IPC round trip, or a fetch to answer. A path the filesystem does
not currently hold bytes for is simply absent from the projection until it
is present.

Cross-device availability is the synchroniser's job, not the app's: the
folder is whatever Syncthing has placed there.

**R3.** A projection — any index, cache, or database rebuildable from tiers 1
and 2 — is authority for nothing. It MUST be safe to delete at any moment,
and it MUST NOT be synchronised. Each app ships the ignore rule that excludes
its projection directory from synchronisation.

### Tier 2 form

**R4.** Tier-2 state is an **append-only event log, partitioned by device**:
one directory per device id, files appended only by that device. No device
ever writes another device's file.

**R5.** Tier-2 state that should follow the user lives inside the synced
folder under one dotted application directory. It MUST NOT live in
`UserDefaults` or an equivalent per-device store, with two exceptions, both of
which MUST be per-device and MUST NOT sync:

- the device's own identifier;
- a **cursor into a per-device external source** — a HealthKit anchor
  (ADR 0008 R4), a security-scoped bookmark, a last-seen mount token. These
  describe this device's relationship to something outside the folder, and
  are meaningless on another device.

Every app's persisted values are enumerated in its repository and classified
tier 1, tier 2, or per-device-exception. There is no unclassified remainder,
and the exception list is closed to these two shapes.

**R6.** Bulk data never enters a log. A log records that something happened
and references a content-addressed blob. Blobs are stored by hash, so
identical content occupies an identical path with identical bytes.

### Conflicts

**R7.** Sync-conflict copies — files matching
`<base>.sync-conflict-<date>-<time>-<device>.<ext>` — are **never content**.
They are excluded from indexing, identity, enrichment, and playback, and are
surfaced as conflict groups naming the surviving file, its copies, and each
copy's parsed timestamp and origin device. An unresolved group is visible
state, never silently hidden.

**R8.** Resolution policy is per app, in its app core:

| App | Unit | Rule |
|---|---|---|
| contacts | `.vcf` | Mergeable. Disjoint field changes merge automatically; the same field changed on both sides is presented for choice. |
| gallery | `.xmp`; rarely the image | Image bytes are never rewritten, so an image conflict is near-always identical content and either copy serves. Sidecars merge: union of human keywords, newest wins for core-owned fields. |
| music | `.m3u` | Ordered list; union preserving the surviving file's order, or presented for choice. Audio files are immutable. |
| health | — | Cannot occur; R4's disjoint writers make it structurally impossible. |

**R9.** An automatic merge MUST be deterministic: identical inputs produce
byte-identical output, under a canonical serialisation with a fixed field
order. A merge that cannot meet this bar is manual-only. Two devices
resolving the same group independently must converge, not produce a new
conflict.

**R10.** Deleting the losing file is the one unprompted write these apps make
to user data, and it happens only on an explicit user choice. Resolution
never deletes a file the user has not decided against.

**R11.** Delete-versus-modify resolves toward keeping data: with no sibling
to compare, the surviving copy is surfaced and the deletion is confirmed,
never applied silently.

### Reconciliation

**R12.** Devices are reconciled by **projection**, never by merging logs.
Disjoint writers under R3 mean synchronisation performs a union. Agreement is
reached in memory, by reading the union of all device logs and applying a
deterministic total order over `(timestamp, event id)` with globally unique
ids.

**R13.** Tier-2 state is modelled as **operations, not snapshots**. An event
records `person_hidden{path}`, never `hidden_set = [...]`. A snapshot of a
collection has no defensible last-writer-wins answer and discards one
device's work; operations union regardless of arrival order, and only a
genuine race on one key falls through to timestamp order.

**R14.** A key that can change MUST change by event. Where tier-2 state is
keyed by something tier 1 owns — a person's tag path, whose name lives in
XMP — a rename is logged (`person_renamed{from, to}`) so replay migrates
every device identically, rather than being applied as a local-only
migration.

**R15.** A projection is **rebuilt, not patched**, when an event arrives that
sorts before what has already been projected. Appends after the high-water
mark MAY be applied incrementally.

**R16.** A **torn final line is a normal state, not corruption**. Whole-file
synchronisation can deliver a log ending in a partial record, repaired on the
next pass. A reader MUST distinguish a truncated last line from malformed
content mid-file, and MUST NOT report the archive as corrupt for the former.

**R17.** Ordering is wall-clock last-writer-wins. Its failure mode is clock
skew, and it is accepted for a single user with time-synchronised devices.

**R18.** A log grows without bound and is synchronised to every device.
Each app MUST state, in its repository, the expected growth rate of its
tier-2 log and either a compaction rule or a written argument that none is
needed. An operation log of user gestures needs no compaction; a log fed by
an automated producer does. Compaction, where it exists, is a rewrite into a
new device directory followed by retirement of the old one — never an edit to
an existing file (R4).

**R19.** Every change to a tier-2 schema, an identity rule, or a derived-data
key is a **migration**, and a migration is not complete until existing state
survives it. Each one ships with a fixture holding pre-change state and an
assertion that the post-change build reads it without loss. A migration that
cannot preserve state MUST say so in release notes before it ships, naming
what is lost.

## Conformance

- No source references a placeholder, download state, ubiquitous-item
  attribute, or file-provider API.
- A path present in a cached projection but absent from disk drops out of
  the projection on the next pass without an error state.
- Every persisted value is classified tier 1 or tier 2 in the app's
  documentation, with no unclassified remainder.
- Deleting every projection and relaunching reproduces identical state.
- No projection path appears in the synced folder without an ignore rule.
- A conflict copy placed in a fixture tree produces a conflict group and
  never a record, an id, or an enrichment job.
- The same conflict pair merged on two independently-built projections
  produces byte-identical output.
- A log truncated mid-record on its last line reads successfully, dropping
  only that record; the same damage mid-file is reported as an error.
- Two device logs replayed in opposite file order produce identical
  projections.
- A rename replayed on a device holding pre-rename events migrates them.
- Each app's persisted-value inventory is complete, and every per-device
  entry matches one of R5's two exception shapes.
- Each app states its log growth rate and its compaction rule or its
  argument for having none.
- Every migration in the repository has a pre-change fixture and a
  survival assertion.

## Rationale

Two devices editing one file is unavoidable for tier 1, because foreign tools
own the format. It is entirely avoidable for tier 2, because we own it — and
localhealth already demonstrates the avoidance: partition by device, address
by content, treat the database as disposable, and there is nothing left to
conflict. Generalising that is cheaper than resolving conflicts we chose to
create.

R8 is the requirement most easily lost. A merge that is nearly deterministic
produces a fresh conflict on each sync, and presents to the user as
synchronisation itself being broken.
