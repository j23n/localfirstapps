# ADR 0002: `localcore` — the folder projection engine

- Status: Accepted
- Date: 2026-09-11
- Revised: 2026-09-11 (r2); 2026-09-11 (native Linux path, no portal);
  2026-09-16 (dependency-tripwire exception policy);
  2026-09-16 (portal experiment controls)

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
graph may open a socket at runtime. R13's resolved-graph check is a tripwire
for known socket-capable dependency families, not proof of that runtime
property.

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

R6 assumes stat is cheap enough for a pass over the selected folder. The
current Linux build uses a native host path. Phase 5B's native controls
measured stat, watch, rescan, and atomic rename successfully, but its
environment had no desktop session, document-portal owner, or persisted
folder grant. Flatpak support is therefore rejected for this revision. Any
packaging that proxies filesystem calls MUST run those measurements through
a real grant, including after session restart, before claiming conformance;
an expensive or semantically different result reopens this requirement.

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

**R13.** **The dependency graph is a review tripwire.** The resolved graph is
checked for curated networking, UI, and platform crate families; source text
alone cannot expose transitive dependencies. Passing this finite policy is
not proof that no dependency can open a socket, so R2 still requires code and
runtime review.

The allowlist is empty by default. An exception MUST be explicitly reviewed,
MUST be build-time only, MUST document an offline override, and MUST enumerate
the exact policy hits below its roots. The checker rejects malformed,
duplicate, unused, or unreviewed entries and any undocumented transitive
policy-hit drift. The current reviewed exception is:

| Crate | Why | Expected policy hits | Offline override |
|---|---|---|---|
| `ort` (and `ort-sys`) | `ort-sys`'s `download-binaries` fetches the prebuilt ONNX Runtime static library at build time; `ort`'s `fetch-models` is disabled | `native-tls`, `openssl`, `openssl-sys`, `ureq`, `ureq-proto` | `CARGO_NET_OFFLINE=true` with `ORT_LIB_LOCATION` pointing at a target-matched directory holding `libonnxruntime.a` |

Adding or changing an entry requires review here and in
`conformance/graph/allowlist.toml`; the check does not infer that approval
from the dependency graph.

A source grep is not an acceptable substitute: it passes a crate that opens a
socket from a dependency, which is how a live reverse-geocoding client
survived inside `core/` under a no-network doctrine.

## Conformance

- The resolved dependency graphs for both core workspaces trip on no known
  networking, UI, or platform crate outside R13's reviewed exceptions,
  checked by one command.
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
ADR 0006 R9 already permitted. The 2026-09-16 revision removes the arbitrary
one-entry cap without weakening the product promise: runtime networking stays
at zero, while every build-time exception remains explicit, offline-capable,
and human-reviewed.
