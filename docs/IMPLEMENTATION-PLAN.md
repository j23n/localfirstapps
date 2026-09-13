# localfiles — implementation plan (r2)

Four apps, three platforms, one engineer with an agent fleet, evenings and
weekends. Written against spec r2 (`spec-r2/`).

---

## 1. What changed from r1, and why it matters

r1 was an inventory problem rather than a reasoning problem: the things it
listed were right, and the things it omitted were what made the schedule
fiction. Six corrections, each of which moves real work:

| r1 said | Actually |
|---|---|
| M4 forces a schema bump and a full rescan | **No.** Every member of `ContentVersion` is already optional on both sides; it shrinks by one field and `LibrarySnapshot` stays at v20. Checked, not assumed. |
| "The 21.5k Foundation-only lines are the prize — they move to Rust more or less directly" | **~6–8k.** 19,666 of the 54,832 classified lines are XCTest. Another 3,349 in gallery alone — `FaceService`, `TaggingService`, `CoreScanner`, `CoreMemories`, `CoreLibraryIndex` — are *adapters for already-ported Rust*. They are replaced, not moved. |
| Geocoding removal is one line in a deletion table | `gallery-geo` is **891 lines of live Nominatim client inside `core/`**, exported over FFI, used by `gallery-ffi`, `gallery-session` and `linux/`. And its replacement — a bundled gazetteer with point-in-polygon country resolution — is a **new crate**, not a deletion. |
| Phase 2 deletions are "negative code, behaviourally identical" | `FileProviderDetector.ContentVersion` sits inside `SidecarCandidate` and `PhotoFile`, both inside `LibrarySnapshot` v20, **read and written by Rust**. It is a cross-language on-disk schema change forcing a full rescan. |
| Nothing about model licences | `PACK_VARIANT=full\|tagging` existed *because* the face embedder was research/non-commercial. **Spike answered: OpenCV Zoo SFace + YuNet (Apache-2.0).** R12/R13 stand; one pack; M3 is real. |
| `git subtree add` "preserving history" | **Verified: it does not.** `git log <path>` returns 1 commit where the original has 15; `--follow` returns 0. `filter-repo` then `merge --allow-unrelated-histories` returns all 15. |
| Path-based agent routing | **27% of gallery's last 30 commits touch both Swift and `core/*.rs`**, and they are the architecturally significant ones. Routing by path routes file edits inside a work item, not work items. |

Two more that r1 had no entry for at all: **four user-data migrations**, and
**design tokens with a dark palette that does not exist** (`Design.swift` is a
light-only literal palette; libadwaita follows the system dark preference).

---

## 2. The shape of the problem

Coverage today — 4.5 of 12 cells:

| | iOS | Fedora | Comet |
|---|---|---|---|
| localgallery | 40.5k Swift | 6.3k GTK | `--comet` |
| localcontacts | 7.2k Swift | — | — |
| localmusic | 7.1k Swift | — | — |
| localhealth | — | Go CLI; web UI preserved at `reference/web-ui/` (not shipped) | — |

Honest Swift classification, production only (test targets excluded):

| | views | Apple-bound | portable logic | FFI adapters |
|---|---|---|---|---|
| localgallery | 13,585 | ~7,000 | ~4,000 | 3,349 |
| localcontacts | 2,236 | ~1,400 | ~1,000 | — |
| localmusic | 4,222 | 158 | ~750 | — |

**~6k lines is what genuinely moves to Rust.** The case for this project is
not line-count arbitrage; it is that domain rules asserted once are asserted
once, and that a second GTK app costs a shell rather than a product.

Two findings from the code that drive sequencing:

- **No `.sync-conflict` handling exists anywhere** — zero matches in 54k lines
  of Rust. ADR 0005's conflict rules are net-new, and the bug is live in all
  four apps today.
- **`gallery-scan` is not cleanly generic** — 469 photo-vocabulary hits, public
  surface is `MediaKind` / `IMAGE_EXTENSIONS` / `VIDEO_EXTENSIONS`. The *walk*
  extracts; classification stays app-side. `gallery-vfs` extracts cleanly
  except that `ProviderAttrs` is in its public `Vfs` surface (33 references)
  and `TEMP_PREFIX` is a constant that must become a parameter.

---

## 3. The organizing principle, corrected

Review time is the bottleneck. The conformance harness reduces it; **it does
not remove it**, and r1 assumed otherwise.

Roughly 62 of the spec's requirements are mechanically checkable, and they
are overwhelmingly *absence* rules — no socket crate, no colour literal, no
file-provider API. The ones that actually get broken are positive rules about
where a decision lives: ADR 0001 R4, ADR 0003 R5, ADR 0004 R8/R9, ADR 0007
R7. The existing GTK shell, written against a conventions document that
forbade exactly this, carries 27 comparator sites and 42 formatting sites.

So the harness is built in two halves:

1. **Structural checks** — the dependency-graph check first (ADR 0002 R13),
   then greps and AST checks. Cheap, total, run in the Fedora container.
2. **The type-system guard** — ADR 0003 R6. Only ids, strings, booleans and
   pre-ordered id lists cross to a shell. A shell handed no record has
   nothing to sort and no field to format. This is worth more than every
   grep combined, and it costs one design decision in Phase 1.

What neither catches goes in the PR template as a review question
(ADR 0007 R16), and **that review time is budgeted, not wished away**.

---

---

## 4. Milestones — the review points

Phases are work; milestones are where you stop and read. Each is chosen so
that you hold **one** mental model of the codebase, not two.

| | Milestone | You review | Holds in your head |
|---|---|---|---|
| **A** | **Clean Slate** — monorepo, pure deletions, spec in tree, harness red, spikes answered | the ADRs themselves, against real directories | the old codebase, cleaned. Nothing new exists yet. |
| **B** | **`localcore` is real** — extracted, gallery running on it unchanged, headless harness green | the new architecture, against working code | the new core. The old shape is gone from `core/`. |
| **C** | **First shell vertical** — contacts on three platforms over `shell-kit` | ADR 0004's vocabulary, having met a second toolkit | one app, end to end |
| **D** | **Per app** — music, gallery, health core loops | the gap list, with a working app in hand | one app at a time |

**A is the one that matters for the question this plan was reorganised
around.** At A, four ADR requirements are already *true in the tree* rather
than aspirational — you review ADR 0005 R2 against a codebase with no
placeholder concept in it, not against one where you imagine its absence.
Nothing has been built yet, so there is no future architecture to hold
alongside the current one.

### The migration register

Five, and ADR 0005 R19 requires each to ship with a pre-change fixture and a
survival assertion. They are listed here in one place because r1 scheduled
none of them and they are the only work in this plan that can lose a user's
state.

| | Migration | Lands | Status |
|---|---|---|---|
| **M1** | Stable ids re-key to NFC | Phase 2 re-key; leftover cache rewrite **5** | person state, thumbnails, memory ids and widget deep links were path-keyed |
| **M2** | Tier-2 `UserDefaults` → event log | Phase 2 dual-write; cutover **5** | seven path-keyed values in gallery alone |
| **M3** | Face-cluster re-key on a model swap | Phase 2 fixture; pack swap **5** | survival fixture is B; SFace + YuNet lands in Phase 5 |
| **M4** | `LibrarySnapshot` sidecar identity | Phase 1 | **checked — not a migration.** See Phase 1. |
| **M5** | Apple Health dated cutover | Phase 6 | real but trivial; a bounded first query, nothing rewritten |

### Milestone A exit criteria

1. Monorepo exists, layout per ADR 0001 R1/R9, originals archived read-only.
2. The pure deletions have landed (Phase 1) — ~6,500 lines, no replacements.
3. `CONVENTIONS.md` retired, all 18 sections dispositioned.
4. The dependency-graph check runs, is **red**, and its allowlist is written.
5. All three spikes have written answers (`docs/spec/spikes/`): SFace + YuNet
   (R12/R13 outcome 1); no Flatpak or portal; ISA drift assumed negligible.
6. All four apps build and test exactly as before.

---

## 5. The phases

### Phase 0 — Foundations and the three spikes

No app behaviour changes. Nothing here blocks on a Mac.

**0.1 Monorepo — done.** Each app was rewritten with
`git filter-repo --to-subdirectory-filter` and merged
`--allow-unrelated-histories`. Directory names are the short ones:

`apps/{gallery,contacts,music,health}`

SHAs changed; `git log apps/<name>` keeps the original commits. The
standalone GitHub remotes still need redirect READMEs — Phase 3,
with 3.0.

Current tree (`core/` is extracted; `shells/` is still Phase 3):

```
.agents/            agent instructions (CONVENTIONS.md retired in 0.2)
docs/               index.html style.css screenshots/   (Pages source)
  spec/             the eight ADRs + spike answers
  IMPLEMENTATION-PLAN.md
docker/
mac/                bootstrap.sh — Xcode CLT, rustup pin, XcodeGen
conformance/        graph check (ADR 0002 R13) is green; R6 is expected-red
core/               localcore-{vfs,walk,id,conflict,queue,log,blob,geo}
apps/gallery/       was localgallery (Swift + core/ + linux/)
apps/contacts/      was localcontacts
apps/music/         was localmusic
apps/health/        was localhealth
```

Eventual tree, after later phases:

```
.agents/
docs/               Pages + spec/
docker/  mac/
core/               workspace 1: localcore-* and <app>-core-*   (no UI deps)
shells/             workspace 2: shell-kit-gtk + four Linux shells
apps/*/ios/         SwiftUI shells
conformance/
```

CI: **one always-running `gate` job** that computes path filters, with every
required check depending on it. Path-filtered required checks that simply
never report are the classic monorepo deadlock, and worse here — a `core/**`
change that breaks the FFI would not run the iOS job at all. Keep
`fetch-depth: 0` (`set_build_number.sh` needs the commit count). Note and
accept: all three iOS apps now share one build number (~184, monotonic, so
TestFlight is fine), and a localmusic commit bumps localgallery's.

**0.2 Retire `CONVENTIONS.md` — done.** Delete plus a sorting pass. All 18
sections are dispositioned in ADR 0007's rationale. State management, app
shell and UIKit appearance dropped as one toolkit's idioms; folder access,
stable ids, design tokens, settings, file I/O, logging and testing sit in
the ADR that owns each; bundle identifiers, build commands and the README
template are ADR 0007 R6, R12, R18. The six contradictions (second
vocabulary table, Store-in-the-shell, identity without NFC, `Data.write`
vs temp-rename, per-app `os.Logger`, per-repo CI) are gone because the
file is gone. Spec is in `docs/spec/`.

**0.3 Environments — done.** `docker/` as delivered (monorepo mount).
`mac/bootstrap.sh` is its sibling: Xcode CLT, the gallery rustup pin,
XcodeGen 2.46.0 via the existing checksummed installer. Work-item
routing lives in `.agents/ROUTING.md` (plan §6).

**0.4 Commit the bindings, and give Linux a compile signal — done.**
Generated UniFFI Swift is committed at
`apps/gallery/LocalGallery/GalleryCore.swift` (plus the C header). The
`.gitignore` comment that forbade this is replaced, not deleted — the
hazard (checksum mismatch against a stale xcframework) is still real;
`.github/workflows/bindings.yml` regenerates and fails on drift. Linux
gets `apps/gallery/linux/swift-shim/` (`swift build` against a host
`libgallery_ffi.so`) and `scripts/generate_bindings.sh` as the
Xcode-free refresh. `openssl-devel` is now a required agent-image
package — `ort` → `ureq` → `native-tls` on the host build.

**0.5 The conformance harness, graph check first — done.**
`conformance/graph/check.py` walks both `core/Cargo.lock` and
`apps/gallery/core/Cargo.lock` against `conformance/graph/allowlist.toml`.
One entry: `ort` / `ort-sys`, build-time, `ORT_LIB_LOCATION`. Phase 2
deleted `gallery-geo`; the check is **green**. R6 stays expected-red
(`conformance/r6/expected.txt`) until gallery-ffi is rewritten.

**0.6 Three spikes — answered.** Written answers live in `docs/spec/spikes/`.

| Spike | Answer | Consequence |
|---|---|---|
| **Face licence** | **Outcome 1.** OpenCV Zoo SFace (Apache-2.0, 128-D ONNX, LFW 99.40%) + YuNet. AuraFace and FaceX MFN rejected. | R12/R13 stand. One pack; `PACK_VARIANT` retires. **M3 is real.** |
| **Flatpak portal** | **Do not use Flatpak or portals.** Native GTK binary, host filesystem, folder is a path, share is a file save. | ADR 0002 R6 holds. ADR 0007 R15 Linux rows lose the portal. Phase 3 ships no Flatpak manifest. |
| **Cross-ISA ε** | **ISA causes no significant difference.** No fixture; ε is not a measured ISA margin. | R16 amended: conventional retention band only. No Phase 2 ε work item. |

> **Gate:** all four apps build and test exactly as before, from the new
> layout, with no behaviour change. The graph check is red and its allowlist
> is written. All three spikes have written answers.

Size: M. Almost entirely agent work.

---

### Phase 1 — Cleanup (Milestone A)

**Deletions only. Nothing is replaced, nothing is built.** This phase exists
so that the ADR review happens against one codebase rather than two, and so
that `localcore` is extracted from a smaller surface.

Recorded before the cut (2026-09-12):

- **M4 wire — preferred.** `downloadStatus` is `#[serde(default)]` and
  omitted when `local`; `contentIdentifier` stays optional for decode.
  Snapshot stays v20. The committed `library_snapshot_v20.json` is the
  pre-change fixture; it must still decode.
- **Gate scope.** Production Swift/Go: no `FileProvider` / `NSFileProvider*`
  / `ubiquitousItem*` / `MetricKit` / `MXMetric*`. Rust
  `Vfs::probe_provider` and generated `VfsProviderAttrs` stay until Phase 2
  lifts Vfs. iOS uses a local-only probe (defaults); placeholders are
  absent from the projection.
- **Health web UI is preserved**, not deleted. `archive serve` leaves the
  product (ADR 0006 R9). The screens, charts, templates and goldens move
  to `apps/health/reference/web-ui/` as the Phase 6 brief. ADR 0004 has
  no `chart` / metric-card kind; that is an R4 amendment when health
  gets shells, not a one-off widget.

| Delete / move | Makes true |
|---|---|
| `PhotoMaterializer`, `CloudStorageService`, `FileProviderDetector`, `RemoteBadge`, `CoreProviderProbe`, Cloud Storage settings, materialize/cloud APIs | ADR 0005 R2 |
| `CrashDiagnosticsService` ×3 (MetricKit) and its Settings chrome | ADR 0006 R9, ADR 0007 R17 |
| `archive serve` (loopback). UI relocated, not deleted | ADR 0006 R9 |

**What cannot move here, and why.** Three deletions are replacement-gated;
pulling them forward ships a regression:

| Deferred to | Deletion | Blocked on |
|---|---|---|
| **5** (pack shipped in B) | `nominatim_lookup` FFI + Linux call sites + `GeocodingService` | Unify iOS onto `run_places`; `localcore-geo` already exists |
| **5** | `ImageIOHeicDecoder` (~100) | the decoder seam consolidated |
| **done (3.1)** | — | `contacts-core` wires `localcore-conflict` (R8–R11). |

**Not deletions — r1 mislabelled these.** `PhotoExporter` (125) re-encodes for
share-sheet export; it is not a decoder and no ADR retires it. `EXIFService`
(80) is the info panel's data source and owns `exifDateFormatter`, whose
missing `timeZone` is deliberate. Both are "replace with a named replacement"
rows, and neither replacement is named yet.

**Amend, do not retire, `localgallery/docs/adr/0002`.** It carries four
decisions and only one is about provider probes. Decision 3 ("a light pass may
reuse a cached row only after a live size+mtime check") *is* ADR 0002 R5's
definition of `light`; decision 1 is its dedupe rule; decision 4 is why
`LibrarySnapshot` is at v20 rather than v21. Strike decision 2, keep the rest,
point at the new ADR.

**M4 — checked, and it is not a risk.** Removing `FileProviderDetector`
touches `SidecarCandidate` and `PhotoFile.SidecarStatus`, both inside
`LibrarySnapshot` v20, which the Rust core also reads and writes. r1 assumed
a version bump and a forced rescan. The types say otherwise:

```swift
struct ContentVersion: Hashable, Codable, Sendable {
    var contentIdentifier: String?     // provider-vended — stops being written
    var modificationDate: Date?        // stat identity — stays (ADR 0002 R6)
    var size: Int64?                   // stat identity — stays
}
```

All three are optional on both sides: Rust marks each
`skip_serializing_if = "Option::is_none"`, and Swift already hand-writes a
tolerant `init(from:)`. So `ContentVersion` **shrinks rather than
disappearing** — it loses the one provider-vended member and keeps exactly the
size-plus-mtime pair ADR 0002 R6 requires. Moving it out of
`FileProviderDetector`'s namespace is a type move; Codable keys do not carry
the enclosing type's name, so the wire format is untouched.

`downloadStatus` is the only non-optional member, and the Rust enum already
derives `#[default] Local`. Two ways to retire it, both one line:

- **Preferred** — add `#[serde(default)]` to the Rust field and
  `decodeIfPresent ?? .local` in Swift, then stop writing the key. Removes the
  concept from the wire format, which is what ADR 0005 R2 wants.
- **Fallback** — keep writing `"downloadStatus": "local"` forever. Zero
  decoder changes on either side.

**No `LibrarySnapshot` version bump, no cache eviction, no forced rescan, no
memories regeneration.** Milestone A is deletions only, as advertised. This
was checked against the source rather than assumed, because the r1 plan's
assumption was the one thing that would have made Milestone A cost users
something.

The Swift deletions batch into one Mac session; the rest is container work.

> **Gate (Milestone A):** all four apps build and test as before. No source
> references a placeholder, download state, ubiquitous-item attribute, or
> file-provider API. No source links a crash-reporting framework. The graph
> check is red with exactly one un-allowlisted entry (`gallery-geo`), which is
> the honest state until Phase 2. **`LibrarySnapshot` is still v20**, and a
> fixture written by the pre-deletion build decodes on the post-deletion build
> with no rescan.

Size: M, and genuinely negative. **This is the ADR review point.**

---

### Phase 2 — `localcore`, settled against gallery (Milestone B)

**The API is settled by the app that will break it.** r1 proved "core + two
shells" on localcontacts, which has no scanner, no queue, no cache, no event
log and no capability of any kind — so ADR 0006's 18 requirements and ADR
0002's freshness tiers would have been designed against a workload that
cannot reveal their cost, then bent for gallery two phases later.

So Phase 2 is a **headless Linux harness over gallery's real 20k-photo tree**.
No shell, no FFI, no parity. Gallery already has the fixtures and the cost
data — and after Phase 1 it has ~6,500 fewer lines of behaviour to preserve.

| Crate | From | Notes |
|---|---|---|
| `localcore-vfs` | `gallery-vfs` (1,308) | `ProviderAttrs` comes **out** of the `Vfs` surface — Phase 1's deletions have already removed its callers, which is the main reason cleanup goes first. `TEMP_PREFIX` becomes a parameter, plus a `.stignore` entry per app. |
| `localcore-id` | `gallery-model::stable_uuid` | **Now NFC-normalising** (ADR 0002 R4). Ships with **M1**. |
| `localcore-walk` | `gallery-scan` walk only | Classification stays app-side, parameterised by extension set. |
| `localcore-conflict` | **new** | ADR 0005 R7–R11. Grammar, detection, resolution policy. Fixture suite before any app calls it. Then wired into all four apps — the live bug fix. |
| `localcore-queue` | `gallery-ml`'s queue, generalised | ADR 0006 R1–R5. The substrate every capability uses. |
| `localcore-geo` | **new** | ADR 0006 R10. Packed `allCountries` class `P` + admin-0 point-in-polygon + spatial index + a data-build step. **1.5–2.5k lines. A crate, not a deletion** — r1 had it in a table of things being removed. Gates the `gallery-geo` / `GeocodingService` removal deferred from Phase 1. |
| `localcore-log` | localhealth's Go `internal/log` (354) | Designed against **both** consumers in one PR: localhealth's `blob_import` shape *and* gallery's `person_hidden` / `person_renamed` operation events with a replay test. Ships with **M2**. |
| `localcore-blob` | localhealth's Go `internal/blobs` (266) | Content-addressed store. |

Also here: **ADR 0003 R6's type boundary** is designed now, because every
later phase depends on it.

Migrations landing in this phase (ADR 0005 R19 — each with a pre-change
fixture and a survival assertion):

| | Migration | Trigger |
|---|---|---|
| **M1** | Stable ids re-key NFC→ | ADR 0002 R4. Person state, thumbnails, memory ids and widget deep links are all path-keyed. |
| **M2** | Tier-2 `UserDefaults` → event log | ADR 0005 R5/R13/R14. Gallery persists `me`, `hiddenPeople`, `featured`, `pinnedPeople`, `featuredPhotoByPerson`, `mePersonPath`, `personContactLinks` — all path-keyed snapshots, which R13 forbids. `migratePersonState` becomes a replayed `person_renamed` event. |
| **M3** | Face-cluster re-key | Survival fixture is B. The SFace + YuNet pack swap is Phase 5. |

> **Gate (Milestone B)** — closed 2026-09-12 on the amended list. The
> backlog table below is not unfinished extract.
>
> - Graph check green.
> - Root `rust.yml` runs `cargo test --locked --workspace` for `core/`
>   and `apps/gallery/core`.
> - Root `apps.yml` requires gallery Linux headless + iOS simulator,
>   contacts iOS, music iOS, and health `go test ./internal/log
>   ./internal/event`. Nested `apps/*/.github` copies do not fire here.
> - Conflict copies are never content. Gallery FFI scan asserts that on
>   `gallery-minimal`; contacts/music `SyncConflictTests` run in `apps.yml`.
> - `localcore-geo` border test passes. The shipped pack is GeoNames
>   `allCountries` (class `P`) + NE 10 m admin-0.
> - `localcore-log` has a gallery replay test and a health Go↔Rust golden.
> - M1–M3 **survival fixtures** pass. M2 is still dual-write; cutover
>   is Phase 5. M3 pack swap is Phase 5.
> - 20k: `scan_tree` exists. Local `e2e_20k.sh` records scan / enrich /
>   index / memories against `e2e_baselines/` (not a merge gate yet;
>   GitHub job is Phase 5; no tree in-repo).
> - Glue: `gazetteer_lookup` → sidecar (FFI + session + Swift); FFI scan
>   excludes `.sync-conflict`. PersonLog attach and `StableUUIDVectorTests`
>   are in `LocalGalleryTests` (gallery iOS job).

**Backlog** (left B; one register). Deferral assigns a phase —
nothing is "whenever," optional, or release-notes-only.
`shell-kit`, tokens, and a R6-clean contacts FFI are Phase 3, not
unphased leftovers. Rows marked 4/5/6 may start once gallery iOS is
green; they still land in that phase and do not block 3.1.

| Item | Phase | Notes |
|---|---|---|
| R8–R11 merge for contacts `.vcf` | **done (3.1)** | `contacts-core` + `fixtures/r8/`. Not a port of Apple `ContactMerge`. |
| Delete contacts/music `SyncConflict` twins | **done (3.1)** | One shared `localcore-conflict/support/SyncConflict.swift`. Tests read the grammar files. FFI bind is 3.4. |
| Queue / scan cache keys: NFC or `StableId` | **3.4** | No queue in 3.1. NFC before the first enqueue. |
| `localcore-log` / blob through `Vfs` | **3.4** | No folder log in 3.1. Before iOS contacts writes one. |
| Open event-type set | **3.4** | If contacts logs. Envelope stays; `known_type` is not a monorepo enum. |
| Do not copy PeopleStore dual-write | **done (3.1)** | Held. vCards on disk are the authority. |
| Old-remote redirect READMEs | **3** | With 3.0. Standalone remotes still need them. |
| `allCountries` class `P` + NE admin-0 pack | **done (B)** | Shipped (`pack_geo.py --fetch`). Rebuild if the dump updates. |
| R8–R11 for music `.m3u` | **4** | Playlist write through `localcore-vfs`. |
| Unify iOS onto `run_places` | **5** | Two orchestrators. Pack is shipped; collapse the Swift loop. |
| M2 UserDefaults cutover | **5** | After gallery iOS is green. Do not copy the dual-write into 3.1. |
| M1 thumb / widget / `library_cache` rewrite | **5** | Path-keyed leftovers. Rewrite them; do not leave orphans as the plan. |
| Queue places / thumbs / EXIF | **5** | Tagging and faces already use `localcore-queue`. |
| M3 pack swap (SFace + YuNet) | **5** | Top remaining product risk. Spike may run during 3; swap lands here. |
| `ImageIOHeicDecoder` | **5** | Decoder seam with the gallery core loop. |
| R8–R11 for gallery `.xmp` | **5** | Same conflict grammar as contacts, on sidecars. |
| gallery-ffi R6 rewrite (45 Records) | **5** | ADR 0003 R4 windowing. |
| 20k-tree as a CI gate | **5** | Local `e2e_20k.sh` exists now (`#[ignore]`, structural golden under `e2e_baselines/`). The GitHub job lands with the gallery vertical. |
| Health Go `internal/log` / `internal/blobs` delete | **6** | After the Rust projection reproduces a real archive. |

Size: L. All Linux-container work.

---

### Phase 3 — `shell-kit` and localcontacts, the first *shell* vertical (Milestone C)

Scope corrected: this proves **ADR 0004 and the Syncthing vCard merge**,
not "core + two shells". `localcore` was settled in Phase 2.

**Do not confuse the two conflict UIs.** Today's
`ContactMerge` / `ConflictResolutionSheet` merge *Apple Contacts* onto a
local card (`CNSyncService`, 459 lines). That is an iOS port (ADR 0007
R15) and stays. The C-loop conflict is ADR 0005 R8: two `.vcf` files
named by Syncthing, disjoint fields merge automatically, the same field
on both sides is a choice, output is byte-identical under a canonical
write (R9), losing copies are deleted only after an explicit choice
(R10), delete-vs-modify keeps data (R11). Contacts already *exclude*
conflict names (`SyncConflict.isConflictName`); they do not group or
merge them. Linux has no Apple Contacts, so Fedora/Comet cannot satisfy
"resolve a conflict" via the CN sheet.

Rows marked **3.1** are this phase. Rows marked **4 / 5 / 6** do not
block C and may start once gallery iOS is green; they still land in
the phase in the table.

**Sequence** (same lesson as Phase 2: settle the core against a real
workload before drawing shells).

1. **3.0 Watch `apps.yml`.** **Done** on `8423be8` — gallery iOS compiled
   and the suite ran under Xcode 26.6. Not a C feature.
2. **3.1 `contacts-core` headless** — **done.** `core/contacts-core`
   + `contacts-ffi`. Walk `.vcf` through `localcore-walk` +
   `localcore-conflict` groups; parse/write; index/search/save through
   `localcore-vfs` atomic write; **R8–R11** on `fixtures/r8/`
   (disjoint auto-merge, same-field choice, canonical bytes, no silent
   delete, delete-vs-modify keeps data). No queue and no folder log
   yet (NFC / Vfs-log are 3.4). Dual-write not copied. The two Swift
   `SyncConflict` twins are one shared file under
   `localcore-conflict/support/`; tests read the grammar fixtures.
   UniFFI is R6-clean (`TextRow` / `FieldRow`); `cargo test` is the
   gate. iOS still uses the Swift parser until 3.4. No GTK.
3. **3.2 Tokens + R14 codegen** (next). One token table per app
   (accent, surfaces, ink, dark companions). Emit Swift + GTK CSS/named
   colours. Someone *authors* dark values — that is design time, not
   an agent guess. A colour literal in a new shell file is a defect.
4. **3.3 `shell-kit-gtk`** — one binding per ADR 0004 R4 kind, no app
   core, no `localcore`. Dummy spec so a missing kind fails the build.
   `shells/` workspace (ADR 0001 R9).
5. **3.4 iOS shell over the core.** Views remain; `ContactsStore` FS
   becomes FFI. `CNSyncService` stays a port. Keep the Apple conflict
   sheet. Add the Syncthing-group sheet (the one Linux will also have).
6. **3.5 GTK + Comet.** Same binary, `--comet` (compact + bottom nav).
   Host filesystem is a path. No Flatpak. Share is a file save.
7. **3.6 Milestone C review.** Expect to revise ADR 0004. Budget it.

> **Gate (Milestone C):** localcontacts' **core loop** on iOS, Fedora
> and Comet — choose a folder, list, search, view, edit, save, resolve
> a **Syncthing `.vcf` group** (R8–R11). Apple Contacts sync remains
> iOS-only. ADR 0004 conformance over both shells. `contacts-core` FFI
> is R6-clean. `gallery-ffi` stays expected-red.

Size: L. 3.1 was Linux-container. 3.4 needs `macos-26`.

---

### Phase 4 — localmusic

Same shape, reusing `shell-kit`. Additions: the media port (AVFoundation /
gstreamer), MPRIS on Linux (ADR 0007 R15), playlist writing through
`localcore-vfs`'s atomic write, and **R8–R11** on Syncthing `.m3u`
pairs (same grammar as contacts `.vcf`).

Least portable logic (~750 lines) and the most views, so shell work dominates
— which is exactly what `shell-kit` should now be absorbing. If Phase 4's GTK
shell is not markedly cheaper than Phase 3's, `shell-kit` is not working and
that is the signal to fix it before gallery.

Size: M.

---

### Phase 5 — localgallery

**Core loop, not full parity** — with a numbered gap list in the spec.

| In the Linux core loop | Explicitly not (gap list) |
|---|---|
| browse, folder tree, search | slideshow ambient music (`SlideshowMusic`, AVAudioEngine) |
| viewer, zoom, video playback | MP4 slideshow export (`SlideshowVideoRenderer`, AVAssetWriter) |
| tags, tag grid | widgets (ADR 0007 R15) |
| people **read**, person pages | face review UI |
| places, memories rail | Live Photos — no Linux equivalent exists |
| scan, tagging, face scan | |

Plus: move the residual portable Swift into `gallery-core`; the FFI moves to
ADR 0003 R4's structure/window split (this is where the 20k-item grid either
scrolls or does not); media port for video thumbnails.

Phase 5 also lands the deferred gallery rows: unify iOS onto
`run_places`, M2 UserDefaults cutover, M1 thumb / widget /
`library_cache` rewrite, queue places / thumbs / EXIF, M3 SFace +
YuNet pack swap, `ImageIOHeicDecoder`, R8–R11 on `.xmp`, and the 20k
e2e GitHub job (the local `e2e_20k.sh` suite already exists).

That gap list is the difference between roughly 8k and 30k lines of GTK.
Each row is a decision you can revisit later with a working app in hand.

Size: L–XL. Per-screen agent tasks against the slot vocabulary.

---

### Phase 6 — localhealth

Full Rust port, both shells — and **materially smaller than r1**, because
ADR 0008 retires the riskiest component instead of porting it.

1. `health-core` over `localcore-log` / `localcore-blob` (Phase 2).
   Projection to SQLite.
2. **HealthKit ingestion in the iOS shell** (ADR 0008 R2–R7): anchored query,
   canonical NDJSON blob per batch, one `blob_import` event, `retract` on
   deletion, dedup on the source identifier. The 1,754-line streaming XML
   adapter is **deleted, not ported** — and with it the group-hash dedup
   heuristic and the differential harness r1 needed to survive it.
3. **M5: dated cutover.** First anchored query bounded to samples after the
   last export import. Old export-derived events stay in the log untouched.
4. FIT adapter — new code in Rust, not a port (unimplemented in Go too).
   Linux-side: watch mount → blob.
5. Gaps report carries ADR 0008 R8's wording: HealthKit cannot report a
   denied read, so "none seen" never means "complete".
6. Two shells, **core loop only**: ingest, browse, one chart, gaps report.
   The IA brief is `apps/health/reference/web-ui/`. A chart / metric-card
   is an ADR 0004 R4 amendment (one native binding per platform), not a
   port of Chart.js.
7. Go tree deleted once the projection reproduces a real archive. The
   reference web UI stays until that amendment is written.

What survives from r1's differential plan: the Go projection's golden
fixtures still pin the **projection**, compared as a canonical ordered JSON
dump per table — not "byte-identical SQLite", which two bindings will never
produce. Add a memory-ceiling test: a Rust port that accidentally buffers
passes every small-fixture test.

Size: L. (r1 sized this XL; ADR 0008 and core-loop parity are why.)

---

## 6. How work routes to agents

Living copy: [`.agents/ROUTING.md`](../.agents/ROUTING.md).

**By work item, not by path.** 27% of gallery's recent commits touch both
Swift and `core/*.rs`, and they are the architecturally significant ones —
"Move Places, pack, and merge policy into the core" is 26 Swift files and 23
Rust files. Routing by path routes *file edits inside a work item*.

**Additive FFI is the discipline that makes this work.** New functions land
alongside old; Swift migrates; the old surface is removed. `main` is never
knowingly unbuildable for a platform, so `macos-26` stays a signal rather than
an expected-red job — which is precisely when committed-binding drift would
otherwise go unnoticed.

| Work | Verified by |
|---|---|
| `core/**`, `shells/**`, `conformance/**`, `docs/**` | `cargo test` in the container, seconds |
| FFI surface change | Linux `swift build` shim (0.4), then `macos-26` |
| `apps/*/ios/**` | `macos-26` by default; Mac VM interactively when >1 round |

**Task shape:** one requirement, one fixture, one PR. "Make `contacts-core`
satisfy ADR 0005 R7, here is the fixture directory, the check must go red to
green." Reviewable in minutes because the check is the review — for the 62
requirements where that is true. For the rest, the PR template asks the
review question (ADR 0007 R16) and you read the diff.

---

## 7. Risks, ranked

1. **Face-model swap quality.** The licence spike came back positive
   (SFace + YuNet). What remains is whether personal-library clustering
   holds up after M3 orphans every user-assigned name.
2. **ADR 0003 R4's windowed boundary not being enough.** If a 20k grid still
   stutters through UniFFI, the fallback is the current arrangement — shell
   holds the structs — which costs ADR 0003 R6 and with it the only structural
   enforcement of ADR 0001 R4.
3. **The slot vocabulary not surviving a second toolkit.** Phase 3 is where it
   holds or gets revised. Budget the revision.
4. **Review capacity.** The honest one. See below.

---

## 8. Sizing, honestly

| Work | Size |
|---|---|
| `localcore` + four app cores | 15–25k |
| GTK core loops ×4 over `shell-kit` | 12–18k |
| SwiftUI rebound to view models | ~20k touched |
| localhealth Go→Rust (no XML adapter) | ~7k |
| `localcore-geo` + data pipeline | 1.5–2.5k |
| Five migrations with fixtures | 2–3k |
| Design tokens + dark palettes ×4 | small code, real design time |

Call it 55–75k lines written or rewritten, all of it reviewed. At a sustained
1,000 reviewed lines a week — aggressive for evenings and weekends — that is
**14–18 months**, and it assumes ADR 0004 survives Phase 3 mostly intact,
which risk 3 says it will not.

What bought the reduction from r1's 75–100k: core-loop parity with a written
gap list (largest single saving), ADR 0008 retiring the XML adapter and its
differential harness, and `shell-kit` making shells 2–4 progressively cheaper.
What added to it: `localcore-geo`, the five migrations, and the token work —
all of which existed in r1 too, just not on the page.

Each phase leaves something shippable. Phases 0–2 change no user-visible
behaviour except removing cloud placeholders and network geocoding, so the
first year's risk is concentrated in Phase 3, which is the cheapest app.

---

## 9. What I would do first

**A, B, 3.0, and 3.1 are done.** Next engineering move is 3.2
(tokens + R14 codegen) — you author the dark values — then 3.3
`shell-kit-gtk`. No GTK in 3.1.

**You, now**

1. Keep the four **root** workflows green on tip.
2. **Phase 3:** redirect READMEs on the old standalone remotes.
3. Author dark token values when 3.2 starts (not an agent guess).
4. Local 20k e2e can be run anytime
   (`apps/gallery/scripts/e2e_20k.sh`; `LOCALGALLERY_E2E_RECORD=1`
   rewrites the golden). Promoting that suite to a GitHub job is
   **Phase 5**.
5. Phase 4/5/6 backlog may run in parallel; it does not block C.
   The gazetteer pack is `allCountries` class `P`.
