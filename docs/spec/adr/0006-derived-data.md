# ADR 0006: Derived data, capabilities, and model packs

- Status: Accepted
- Date: 2026-09-11
- Revised: 2026-09-11 (r2); 2026-09-11 (spikes: SFace; ε not ISA-sized); 2026-09-12 (R10 fixture-pack exception, lifted the same day)

## Scope

Everything computed from a file rather than read from it: thumbnails, image
tags, face clusters, place names, audio artwork, waveforms. These are the
largest source of complexity in the family, and they share one shape.

## Requirements

### The substrate

**R1.** Every expensive derived artefact is produced through one substrate in
`localcore` (ADR 0002 R10): a persistent work queue plus a
content-addressed result cache. No app implements its own queue.

**R2.** Results are keyed on the **content hash** of the input, never on a
path. A file that moves or is renamed keeps its derived data; two identical
files share it.

**R3.** A capability declares **two independent version constants**:

- an **input version**, naming which inputs this build can process. Raising
  it re-opens work previously refused as unsupported, and invalidates
  nothing.
- a **semantic version**, naming how a result is produced. Raising it
  invalidates cached results.

These MUST NOT be conflated. Gaining the ability to open a new format is not
a reason to recompute every result already held.

**R4.** A run is **resumable and cancellable**. On start it reclaims work
abandoned by a previous run, re-opens items refused under an older input
version, and re-checks items whose source changed. Cancellation leaves the
queue consistent.

**R5.** Capabilities are **independent**. One being unavailable, failing, or
disabled MUST NOT prevent another from running, and each is separately
resumable.

**R6.** Derived data is a projection (ADR 0005 R3): disposable, never
synchronised, and rebuildable. Deleting the cache costs time and nothing
else.

**R7.** Results that belong to the user are written to tier 1 in the
interoperable format — image tags and people as XMP keywords and regions —
so that another tool reads what this one produced. The cache holds only what
would be merely expensive to recompute.

**R8.** Capability progress and completion are reported through the app core
(ADR 0003 R8), not observed by shells reading the cache.

### Constraints on capabilities

**R9.** **No capability performs network I/O**, at runtime, on any platform.
No coordinate, filename, hash, or file content leaves the device by any path
at any time, and no code observes network state. There are no exceptions and
no configuration that introduces one.

**Build-time** downloads (a toolchain fetching a prebuilt dependency) are not
runtime code paths, but every one MUST appear on ADR 0002 R13's allowlist with
a documented offline override, so that a build succeeds with no network
available. Conformance is the dependency-graph check in ADR 0002 R13; a source
grep does not discharge this requirement.

**R10.** **Place names come from a bundled dataset.** Photo coordinates
resolve to a locality and a country entirely offline. The dataset is chosen
at these two tiers:

- **Locality** — GeoNames `allCountries` rows with feature class `P`
  (populated places), including hamlets with population 0.
  Neighbourhood sections (`PPLX`) and historical/abandoned codes are
  omitted; lookup snaps to a PPLC/PPLA seat within 25 km so an
  arrondissement does not beat the city. Streams, peaks, schools, and
  other non-`P` features are not packed (R11). `cities500` /
  `cities1000` remain rebuild options. Licence CC BY 4.0, which
  permits redistribution with attribution (R13).
- **Country** — point-in-polygon against a public-domain admin-0 boundary
  set (Natural Earth or equivalent). Nearest-locality search MUST NOT decide
  the country: near a border it picks the wrong one, and country is what tag
  and title data depends on.

The packed gazetteer is larger than the old 10 MB city-dump cap; it
MUST stay under 256 MB. The raw `allCountries` file (every named
feature) is not what ships — only class `P`.

A lookup answers with the nearest populated place, which is not the same as
the place that contains the point; a photo in open country may resolve to a
town some distance away. This is accepted.

The committed pack is GeoNames `allCountries` (class `P`) plus Natural
Earth 10 m admin-0 (`scripts/pack_geo.py --fetch`).

**R11.** **Points of interest are not a capability.** No POI or landmark
dataset is bundled, and no landmark is inferred from coordinates. Proximity
identifies where the camera stood, not what it saw, and planet-scale POI data
is three orders of magnitude larger than the place data above.

Landmark names reach the apps as **sidecar metadata**, written by the desktop
tagger from image content, and are read like any other tag. An app MUST
render a landmark name it finds and MUST NOT try to derive one.

**R12.** Photo tagging and face clustering run **on-device**, on every
platform, from a **single model pack**. There is one pack; there are no
variants, no feature-gated subsets, and no per-platform difference in what a
pack contains.

**R13.** **Every weight in a distributed pack MUST carry a licence permitting
redistribution.** A model that may not be redistributed cannot be in the
pack, and therefore cannot be a capability. This constrains model selection
and is not negotiable at build time.

R12 and R13 together were unsatisfied while the embedder was insightface
`buffalo_sc` (research / non-commercial) and the `full` / `tagging` pack
split existed as the workaround. **Spike outcome 1 is recorded:** the
embedder is OpenCV Zoo SFace and the detector is YuNet, both
redistributable (Apache-2.0 / MIT). R12 and R13 stand unchanged. One pack;
`PACK_VARIANT` is retired. Swapping models changes `face_pack_key` and is
migration M3 under ADR 0005 R19.

**R14.** A pack is identified by version and verified by hash before use. An
app with no pack, or a pack failing verification, disables the capabilities
that need it and remains fully functional otherwise.

**R15.** **Results MUST converge across devices.** Every device may run every
capability. Convergence is achieved by recording decisions in tier 1 and
respecting them, not by requiring bit-identical arithmetic on different
instruction sets:

- A capability writing to tier 1 MUST be **idempotent at the byte level**: a
  re-run that reaches the same decisions writes nothing at all, leaving
  modification time untouched. A synchroniser must see no event.
- A capability MUST read the decisions already recorded in the file and treat
  them as input, not recompute the file from scratch.
- Values written into tier 1 MUST be rounded to a fixed, platform-independent
  textual form, so that equal values produce equal bytes.
- Every ordering in an emitted list MUST be total (ADR 0002 R11).

**R16.** **Every thresholded decision whose outcome reaches tier 1 carries a
retention band.** Where such a comparison is made, a decision this core
already recorded is retained while the score remains within `threshold − ε`.
This applies without exception to tag scoring, face detection confidence, and
matching a face to an already-named cluster.

It does **not** apply to decisions confined to the cache. A clustering
threshold that only partitions unnamed faces writes nothing to a file and
needs no band; adding one there is meaningless, because there is no recorded
prior decision to retain.

`ε` is declared in the pack manifest. **Cross-instruction-set drift is
assumed negligible** (spike 0.6): ε is a conventional retention band, not a
measured ISA margin, and no arm64 / x86-64 fixture is required.

A capability with a thresholded decision and no retention band is
non-conforming: two devices straddling the bar will rewrite each other's
results indefinitely.

**R17.** **Pack precedence: newer wins, older defers.** A file records the
pack version its core-owned fields were produced by. A device whose installed
pack is **older** than the version recorded in the file MUST NOT rewrite
those fields; it reads them and leaves them alone. A device whose pack is
newer rewrites them and records the new version.

Without this, a device left on an older release rewrites a newer device's
results on every run, and every rewrite is a genuine content change that no
determinism rule can absorb.

**R18.** Decoding inputs is an explicit, enumerated seam. Which decoder
handles which format is a property of the build, declared statically, not
discovered at runtime.

## Conformance

- Every capability in every app is registered with the substrate; a search
  for a second queue implementation finds none.
- Moving a file to a new path preserves all of its derived data.
- Raising a capability's input version re-opens refused work and recomputes
  nothing already held; raising its semantic version recomputes.
- Killing a run mid-queue and restarting completes without loss or
  duplication.
- Disabling one capability leaves the others running.
- Deleting the cache and re-running reproduces identical results.
- ADR 0002 R13's graph check passes over `core/`, and no source references a
  network-path or reachability API.
- The whole test suite passes with the network interface down, and place
  lookup returns a locality and a country while it is down.
- Bundled place data totals under 256 MB, and its licences are recorded.
- Two coordinates either side of a land border resolve to different
  countries.
- No POI or landmark dataset is present; a landmark name in a sidecar is
  rendered, and none is derived from coordinates.
- The pack manifest enumerates every weight with its licence, and every
  entry permits redistribution.
- Re-running a capability over unchanged inputs writes no file and changes no
  modification time.
- Every thresholded decision has a retention-band test: a score just inside
  the band retains a recorded decision, one outside it does not. ε is the
  conventional band in the pack manifest; no cross-ISA drift fixture is
  required.
- A file carrying a newer pack version is left untouched by a core running an
  older pack, and rewritten by one running a newer pack.
- Two devices tagging the same fixture library, each seeded with the other's
  output, reach a fixed point in one further pass and write nothing on the
  next.

## Rationale

Tagging, faces, thumbnails and geocoding were built as four separate
pipelines with four caches and four invalidation rules. They are one problem:
expensive output, derived from immutable content, that must survive restarts
and must not be recomputed without cause. One substrate makes each new
capability small, and makes the invalidation rule — where these bugs actually
live — a single implementation.

R3 records an expensive lesson already learned once: conflating "which
formats can this build open" with "how are pixels produced" means shipping a
new decoder re-runs inference over an entire library.

R9 and R13 follow from the product rather than from engineering taste. An app
whose premise is that nothing leaves the device cannot send coordinates to a
geocoding service, and a single opinionated pack shipped to users cannot
contain weights that forbid redistribution. Both constrain which models and
datasets are viable, and both are load-bearing for the promise the product
makes.

R15 through R17 (r2) replace a requirement that asked for bit-identical
results across instruction sets. Convergence is by recorded decision:
idempotent writes, a retention band, and pack precedence. Spike 0.6 further
records that **ISA drift is assumed negligible**, so ε is a conventional
band in the pack manifest rather than a measured cross-ISA margin. The
remaining gaps R15–R17 close are the band itself (tagging had one; face
detection and auto-tag matching did not) and the precedence rule for
version skew.

R9 is unconditional because every candidate exemption turned out to be
avoidable. Host file materialisation goes with ADR 0005 R2's on-disk rule,
the loopback UI server goes with localhealth gaining native shells, and
link-state observation existed only to decide whether to prefetch
placeholders — which no longer exist. A rule with no exceptions is one an
audit can check by searching for socket and HTTP crates.

R10 costs on the order of 120 MB of packed class-`P` places — still
small beside the model pack — to retire the one outbound request in
the family. R11 declines the much larger dataset that would follow,
and does so on accuracy grounds before cost: coordinates say where
the photographer stood, and the desktop tagger already identifies
landmarks from the image itself and writes them where every app can
read them.
