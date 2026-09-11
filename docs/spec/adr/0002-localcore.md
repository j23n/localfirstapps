# ADR 0002: `localcore` — the folder projection engine

- Status: Accepted
- Date: 2026-09-11
- Revised: 2026-09-11 (r2); 2026-09-11 (native Linux path, no portal)

## Scope

The shared engine underneath all four apps. It turns a directory tree into an
ordered, searchable, incrementally-maintained projection, and keeps that
projection correct as the tree changes.

`localcore` knows nothing about photos, contacts, tracks or health events. It
is generic over a **record** type supplied by an app core.

## Requirements

**R1.** `localcore` MUST NOT reference any platform API, UI toolkit, or app
core. Its only I/O is through the `Vfs` port (R3).

**R2.** `localcore` MUST NOT perform network I/O. No crate in its dependency
graph may open a socket at runtime. The check is R13's, not a source grep.

**R3.** All filesystem access goes through a `Vfs` port: list, stat, read,
write, rename, remove. Writes MUST be atomic (temp file plus rename within
the same directory). A default implementation over the host filesystem is
provided; platforms with additional requirements supply their own.

The temp-file prefix is a parameter of the `Vfs`, not a constant, and each
app MUST ship a synchroniser ignore rule for its own prefix — a temp file
created inside a synced directory is otherwise replicated and then deleted
on every write.

**R4.** **Stable identity.** A record's id is derived from its path by
SHA-256 over the UTF-8 bytes of the standardised path, taking the first 16
bytes and setting the RFC 4122 variant and version-5 marker nibbles. Paths
MUST be normalised to **NFC** before hashing. Identity is a pure function of
the path: the same path yields the same id on every platform and every run.

NFC normalisation is a change from the current implementation, which hashes
the bytes as they arrive and derives different ids for NFC and NFD spellings
of one name. Adopting it re-keys every record whose path is stored decomposed
and is a user-data migration, not a definition (see the plan).

**R5.** **Scanning.** `localcore` walks a tree and reports an outcome per
entry. It offers three request kinds:

- `light` — reuse cached records whose size and modification time are
  unchanged; one stat per file; no expensive probes.
- `full` — rebuild every record; probe everything.
- `auto` — light, promoted to full when the last full pass is older than the
  configured interval.

A scan request MUST be satisfied only by a pass that could have answered it:
not by a weaker kind, and not by a pass that began before the request was
made. Concurrent scans of one root are deduplicated; scans of different roots
serialise.

**R6.** **Every per-file query is a stat.** Size, modification time and type
are the only attributes read. `localcore` MUST NOT read an attribute whose
answer requires IPC, a daemon, or a fetch (ADR 0005 R2). There is no batching
tier and no probe budget, because there is nothing expensive left to batch.

R6 assumes stat is cheap. Linux delivery is a native path on the host
filesystem (no Flatpak, no portal), so that assumption holds. A deployment
that made stat expensive — a sandbox filesystem proxy, for instance —
would be a finding against the deployment, and would reopen this
requirement rather than being absorbed silently.

**R7.** **Conflict awareness.** The scanner recognises sync-conflict copies
by filename and reports them as conflict groups rather than records
(ADR 0005 R7). They never receive an id under R4.

**R8.** **Indexing.** `localcore` owns one record table per projection; sort
order, the search corpus, and every grouping are lists of indices into it. No
index copies a record. Text matching MUST compare by Unicode canonical
equivalence, so that a precomposed query matches a decomposed filename.

**R9.** **Event log and projection.** `localcore` provides the tier-2 event
log described in ADR 0005: device-partitioned append, tolerant read, and a
deterministic total order over the union of all device logs.

**R10.** **Derived-data substrate.** `localcore` provides the queue and
content-addressed cache described in ADR 0006. Every expensive derived
artefact in every app uses it; no app builds its own queue.

**R11.** Every ordering, comparison and merge in `localcore` MUST be a pure,
total function of its inputs. Given identical inputs, two devices produce
identical output, byte for byte. Every ordering MUST be **total**: where a
primary key can tie, a tiebreak is declared and tested.

**R12.** `localcore` MUST NOT hold a projection as the authority. Every cache
and index it owns is disposable and rebuildable from the folder plus the
event log.

**R13.** **The dependency graph is the audit surface.** Conformance to R2 and
to ADR 0006 R9 is checked over the resolved graph, not over source text. The
check runs against a written allowlist, and the allowlist has exactly one
entry:

| Crate | Why | Offline override |
|---|---|---|
| `ort` (and `ort-sys`) | build-time fetch of the prebuilt ONNX Runtime static library | `ORT_LIB_LOCATION` pointing at a directory holding `libonnxruntime.a` |

An allowlisted crate MUST be build-time only and MUST have a documented
override that makes a clean-checkout build succeed with no network. Adding a
second entry is an amendment to this document.

A source grep is not an acceptable substitute: it passes a crate that opens a
socket from a dependency, which is how a live reverse-geocoding client
survived inside `core/` under a no-network doctrine.

## Conformance

- The resolved dependency graph for `core/` contains no networking, UI, or
  platform crate outside R13's allowlist, checked by one command over one
  lockfile.
- A clean-checkout build succeeds with the network interface down, using each
  allowlisted crate's documented override.
- Identity vectors: a committed fixture of path→id pairs, including NFC and
  NFD spellings of the same name, passes on every platform, and asserts they
  now resolve to the **same** id.
- A counting `Vfs` records only list, stat and read calls across a full
  scan; no other attribute query occurs.
- A scan request issued while a weaker or earlier pass is in flight causes a
  further pass.
- Search over a corpus containing decomposed filenames finds them from
  precomposed queries.
- Two projections built from the same inputs in different orders compare
  byte-identical.
- Every declared ordering has a tiebreak test over inputs that tie on the
  primary key.
- Deleting every cache and rebuilding reproduces the same projection.
- Each app's ignore rules cover its `Vfs` temp prefix.

## Rationale

Each app had grown its own walker, its own idea of freshness, and its own
index. They are the same problem: a tree that changes underneath you, most of
which has not changed. Solving it once is the largest single reduction in
code and in the number of places a subtle bug can live.

R11 is not fastidiousness. Devices reconcile by rebuilding the same
projection independently (ADR 0005); if two devices can order the same inputs
differently, they disagree about the library while both being "correct".

R13 (r2) exists because the r1 conformance bullet — "`cargo tree` contains no
networking crate" — was unsatisfiable on the day it was written, for a reason
ADR 0006 R9 already permitted. A rule with a documented, bounded exception is
enforceable; a rule with an undocumented one is decoration.
