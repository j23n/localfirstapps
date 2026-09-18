# localfiles — implementation plan (r2)

Four apps, three platforms, one engineer with an agent fleet, evenings and
weekends. Written against the specification in `docs/spec/`.

This file is the living backlog. When a review or design pass changes what
is true, update the coverage table, the register, and the phase that owns
the work. Do not leave findings only in a side document. The GTK design
language and kit sequence live in [`GTK-DESIGN-PLAN.md`](GTK-DESIGN-PLAN.md);
the inventory and sequencing consequences of that pass are recorded here.

The GTK design pass is **landed** for Contacts, Music, and the Gallery
kit sequence (5.1–5.7 + 5.9 year rail / ADR+docs). Leftover Gallery GTK
stays 0.8 until the owner agrees to remove it. Phase 0 crate bump is done
(`shells/` gtk4 0.11 / libadwaita 0.9). Phase 1 generator is done
(libadwaita accent-fg, authored Gallery dark surfaces, leftover
`--accent` alias of `--accent-bg-color`). `init_style` is done: apps
call `init_style(TOKEN_CSS)` only; kit `data/style.css` maps
`accent_bg_color` from `--accent-bg-color` (do not treat `--accent` as
the API) and does not clobber `accent_color`. Colour-literal check
covers `shells/**/*.css` and
`shells/**/*.rs`. Phase 0 screenshot harness is in
(`shell-kit-gtk::snapshot`, debug `--route` / `--snapshot` / `--size` /
`--folder`, `scripts/gtk-snapshots.sh`). **Phase 2a structure is
landed:** `adaptive_shell` / `page` / `clamped` / sized `sheet`;
Contacts and Music swapped off `apply_chrome` (Add stays on the shared
header). **Phase 2b typed builders are landed:** `ListScreen`,
`SettingsScreen`, `FormSheet`, `empty_state`, `selection_bar`; Contacts
`contact-list` / `settings` / `tag-management` / `logs` / `contact-edit`
and Music `playlist-list` / `settings` assemble through them. **Phase 2c
row polish is landed:** `Leading` on `text_row`, dim numeric suffixes,
status-row icons, `action_button_row`, `scope_toggle` for
`Filter::Scope`, `header_action` / `inline_primary` / `overflow(menu)`;
Contacts list rows pass avatar initials and letter section keys. Reuse is
25 shared of 28 Contacts / 29 Music bindings after chrome progress (`ChipBar` is
Contacts + Gallery). Gallery ∩ Music includes `MediaItem`.

Host review 2026-09-16 plus full GNOME HIG pass is **implemented**
in the kit and both GTK apps: primary menu → Settings dialog;
Contacts list\|detail split; Music 4-tab browse switcher (Songs ·
Artists · Albums · Playlists) with Now Playing as the wide stage
and a compact mini-player; conflict field diff; `lofty` metadata and
embedded artwork; HIG search (global, type-to-search, Ctrl+F). Rebuild
`localmusic` on the host with `--features gstreamer-playback` for actual
play. Details in
[`GTK-DESIGN-PLAN.md`](GTK-DESIGN-PLAN.md).
Contacts “before” shots live in
[`docs/screenshots/gtk-before/`](screenshots/gtk-before/). Further
mutter captures stay optional. Ubuntu 26.04
(resolute) floor versus Fedora 44 is GTK 4.22.2, libadwaita 1.9.0, Pango
1.57.0. The `Apps / GTK shells compile` and `Apps / GTK shells test`
jobs run on `ubuntu-26.04`;
`ubuntu-24.04` is GTK 4.14 and cannot build the `v4_22` crates.
L6 ToggleGroup / WrapBox fallbacks are not required.

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
| Nothing about model licences | `PACK_VARIANT=full\|tagging` existed *because* the face embedder was research/non-commercial. The spike found an Apache-2.0 **candidate** (SFace + YuNet), but did not validate crop alignment, target-device performance, or personal-library clustering. Final selection remains Phase 5 evidence. |
| `git subtree add` "preserving history" | **Verified: it does not.** `git log <path>` returns 1 commit where the original has 15; `--follow` returns 0. `filter-repo` then `merge --allow-unrelated-histories` returns all 15. |
| Path-based agent routing | **27% of gallery's last 30 commits touch both Swift and `core/*.rs`**, and they are the architecturally significant ones. Routing by path routes file edits inside a work item, not work items. |

Two more that r1 had no entry for at all: **four user-data migrations**, and
**design tokens with a dark palette that does not exist** (`Design.swift` is a
light-only literal palette; libadwaita follows the system dark preference).

---

## 2. The shape of the problem

Coverage today — 9 of 12 cells (Linux Music, Contacts, and Gallery are
kit shells; leftover Gallery GTK stays as `localgallery-reference`;
Health has no shell):

| | iOS | Fedora | Comet |
|---|---|---|---|
| localgallery | 40.5k Swift | `gallery-gtk` (kit); leftover `apps/gallery/linux` | `--comet` |
| localcontacts | 7.2k Swift | `contacts-gtk` (kit) | `--comet` |
| localmusic | 7.1k Swift | `music-gtk` (kit; playback needs `gstreamer-playback`) | `--comet` |
| localhealth | — | Go CLI + `health-core` / `health-ffi`; no shell | — |

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
| **A** | **Service retirement** — monorepo, named cloud/diagnostic services removed, spec in tree, harness established | the ADRs themselves against real directories, including semantic leftovers | the old codebase, smaller but not semantically clean |
| **B** | **`localcore` is real** — extracted, gallery running on it unchanged, headless harness green | the new architecture, against working code | the new core. The old shape is gone from `core/`. |
| **C** | **First shell vertical** — contacts core loop on iOS, Fedora and Comet | ADR 0004's vocabulary against one product slice. `shell-kit-gtk` was provisional at C; Music GTK is now the second consumer (measured reuse). Design/density remain a later pass. | one app, end to end |
| **D** | **Per app** — music, gallery, health core loops | the gap list, with a working app in hand | one app at a time |

**A remains an important review point, but its original “clean slate” label
overclaimed the result.** Named provider services were removed while
`PhotoLocality`, `DownloadStatus`, QuickLook placeholder handling and related
FFI state remained. Review the semantic model rather than treating a
framework-spelling grep as proof.

### The migration register

Five, and ADR 0005 R19 requires each to ship with a pre-change fixture and a
survival assertion. They are listed here in one place because r1 scheduled
none of them and they are the only work in this plan that can lose a user's
state.

| | Migration | Lands | Status |
|---|---|---|---|
| **M1** | Stable ids re-key to NFC | Phase 2 changed the id function; id-key migration landed; path-keyed leftovers **5.8-m1-swift-keys** | thumbs disk already `{stableID}.jpg`; snapshot / widget / in-memory keys still path-shaped |
| **M2** | Tier-2 `UserDefaults` → event log | Phase 2 added replay + dual-write; **person keys cut over** | after attach, `.gallery/log/<dev>/` is authority; five person keys are not written back; memories snapshots still UserDefaults (**5.8-m2-memories**) |
| **M3** | Face-cluster re-key on a model swap | Phase 2 preflight fixture; Phase 5B rejected swap; evidence **5.8-m3-evidence** | fixture proves XMP survives a cache reset, not a model migration; `PACK_VARIANT` stays |
| **M4** | `LibrarySnapshot` sidecar identity | Phase 1 | **checked — not a migration.** See Phase 1. |
| **M5** | Apple Health dated cutover | Phase 6 | real but trivial; a bounded first query, nothing rewritten |

### Milestone A exit criteria

1. Monorepo exists, responsibilities assigned per ADR 0001 R1 and workspaces
   rooted per R9; originals are archived read-only.
2. The pure deletions have landed (Phase 1) — ~6,500 lines, no replacements.
3. `CONVENTIONS.md` retired, all 18 sections dispositioned.
4. At the close of Milestone A, the dependency-graph check runs and is
   **red**, with its allowlist written. It becomes green at Milestone B.
5. All three spike notes record their evidence status (`docs/spec/spikes/`):
   SFace + YuNet is licence-compatible but otherwise unvalidated; Flatpak
   portal behaviour is unmeasured; cross-ISA drift is unmeasured.
6. All four apps build and test exactly as before.

---

## 5. The phases

### Phase 0 — Foundations and the three spikes

No app behaviour changes. Nothing here blocks on a Mac.

**0.1 Monorepo — done.** Each app was rewritten with
`git filter-repo --to-subdirectory-filter` and merged
`--allow-unrelated-histories`. Directory names are the short ones:

`apps/{gallery,contacts,music,health}`

SHAs changed; `git log apps/<name>` keeps the original commits. Former
standalone remotes are outside this tree, so redirect work cannot be
performed here. The concrete in-tree guidance is each app README naming its
canonical `apps/<name>` location and the root workflows that supersede
nested standalone workflows.

Current tree (`core/` is extracted; `shells/` has the kit plus Contacts and Music GTK):

```
.agents/            agent instructions and work-item routing
docs/               index.html style.css                (Pages source)
  spec/             the eight ADRs + spike evidence notes
  IMPLEMENTATION-PLAN.md
  GTK-DESIGN-PLAN.md
docker/
mac/                bootstrap.sh — Xcode CLT, rustup pin, XcodeGen
conformance/        graph check (ADR 0002 R13) is green; R6 is expected-red
core/               localcore-{vfs,walk,id,conflict,queue,log,blob,geo,ui}
                    + contacts-core / contacts-ffi
shells/             shell-kit-gtk + shell-kit-swift + contacts-gtk + music-gtk (`--comet`)
design/tokens/      per-app tables; sourced dark accents (3.5)
docs/spec/ui/       R4 vocabulary.toml
apps/contacts/ui-spec/  real contacts screens
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
apps/*/             per-app files and SwiftUI shells (kept at current app roots)
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
`libgallery_ffi.so`) and `apps/gallery/scripts/generate_bindings.sh` as the
Xcode-free refresh. `openssl-devel` is now a required agent-image
package — `ort` → `ureq` → `native-tls` on the host build.

**0.5 The conformance harness, graph check first — done.**
`conformance/graph/check.py` walks both `core/Cargo.lock` and
`apps/gallery/core/Cargo.lock` against `conformance/graph/allowlist.toml`.
The current reviewed build-time exception is `ort` / `ort-sys`, with
`ORT_LIB_LOCATION` as the offline override. The graph was red through
Milestone A; Phase 2 deleted `gallery-geo`, and the current check is
**green**. Gallery FFI windowing later emptied
`conformance/r6/expected.txt`; the 20k GitHub job is a required
`rust.yml` `gallery-core` step (`apps/gallery/scripts/e2e_20k.sh`).

**0.6 Three spikes — documented.** Evidence notes live in `docs/spec/spikes/`;
their evidence is not equally complete.

| Spike | Answer | Consequence |
|---|---|---|
| **Face licence** | SFace + YuNet is an Apache-2.0 candidate; product alignment, clustering quality and device cost were not measured. | Keep one-pack intent provisional. Validate against representative libraries before any selection. |
| **Flatpak portal** | The current build is native; none of the proposed portal measurements ran. | Flatpak suitability remains unmeasured, neither approved nor banned. |
| **Cross-ISA ε** | No cross-ISA fixture was run; the note selected conventional hysteresis policy. | R16 uses a retention band; adequacy as an ISA margin remains a hypothesis. |

> **Historical Phase 0 gate:** all four apps build and test exactly as before,
> from the new layout, with no behaviour change. The graph check is red and
> its allowlist is written. All three spike notes record what was and was not
> measured. The graph's current green state is the later Milestone B result.

Size: M. Almost entirely agent work.

---

### Phase 1 — Cleanup (Milestone A)

**Executed as deletions only.** The architecture review found that this was
too mechanical: named services disappeared while parts of their state model
remained. The phase still reduced extraction scope, but did not produce the
semantic clean slate its gate claimed.

Recorded before the cut (2026-09-12):

- **M4 wire — preferred.** `downloadStatus` is `#[serde(default)]` and
  omitted when `local`; `contentIdentifier` stays optional for decode.
  Snapshot stays v20. The committed `library_snapshot_v20.json` is the
  pre-change fixture; it must still decode.
- **Gate scope.** Production Swift/Go: no `FileProvider` / `NSFileProvider*`
  / `ubiquitousItem*` / `MetricKit` / `MXMetric*`. Rust
  `Vfs::probe_provider` and generated `VfsProviderAttrs` stay until Phase 2
  lifts Vfs. iOS uses local defaults, but placeholder/download concepts
  remain in gallery models, QuickLook behavior, FFI and tests.
- **Health web UI follow-up is complete.** `archive serve` left the product
  at Phase 1 (ADR 0006 R9). The useful screen, chart-data, provenance, and
  deterministic fixture contracts are now curated at `apps/health/ui-spec/`;
  the non-building server, templates, rendered goldens, Chart.js, and font are
  deleted. ADR 0004 R4 later gained `chart-row` (native kit sparkline). That
  is not a Health shell and is not a Health accent.

| Delete / move | Makes true |
|---|---|
| `PhotoMaterializer`, `CloudStorageService`, `FileProviderDetector`, `RemoteBadge`, `CoreProviderProbe`, Cloud Storage settings, materialize/cloud APIs | Removes app-initiated materialisation; legacy placeholder/download fields remain compatibility debt |
| `CrashDiagnosticsService` ×3 (MetricKit) and its Settings chrome | ADR 0006 R9, ADR 0007 R17 |
| `archive serve` (loopback); static UI contracts retained separately | ADR 0006 R9 |

**What cannot move here, and why.** Three deletions are replacement-gated;
pulling them forward ships a regression:

| Deferred to | Deletion | Blocked on |
|---|---|---|
| **done** | `nominatim_lookup` FFI + Linux call sites + `GeocodingService` | iOS Places loop is already `run_places`; `localcore-geo` shipped. Orchestrator collapse is **5.8-ios-analysis**. |
| **keep (iOS port)** | `ImageIOHeicDecoder` (~100) | stays as the iOS `HostHeicDecoder` port; Linux leftover adapter is **5.8.3 landed** |
| **done (3.1)** | — | `contacts-core` wires `localcore-conflict` (R8–R11). |

**Not deletions — r1 mislabelled these.** `PhotoExporter` (125) re-encodes for
share-sheet export; it is not a decoder and no ADR retires it. `EXIFService`
(80) is the info panel's data source and owns `exifDateFormatter`, whose
missing `timeZone` is deliberate. Both are "replace with a named replacement"
rows, and neither replacement is named yet.

**Amend, do not retire, `apps/gallery/docs/adr/0002-scan-freshness.md`.** It carries four
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

> **Recorded outcome at the close of Milestone A:** named provider/MetricKit services and
> host API linkages are gone, and the v20 fixture still decodes. Semantic
> placeholder/download state remains and is explicit debt; the source checker
> proves retired host API linkage only. The graph
> check is red with exactly one un-allowlisted entry (`gallery-geo`), which is
> the honest historical state until Phase 2 made it green. **`LibrarySnapshot`
> is still v20**, and a
> fixture written by the pre-deletion build decodes on the post-deletion build
> with no rescan.

Size: M, and genuinely negative. **This is the ADR review point.**

---

### Phase 2 — `localcore`, exercised against gallery (Milestone B)

**The API is exercised by the app most likely to break it, not declared
stable.** r1 proved "core + two
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
| **M2** | Tier-2 `UserDefaults` → event log | ADR 0005 R5/R13/R14. Phase 2 added replay + dual-write for the five person keys. Person-key authority later cut over to `.gallery/log/<dev>/`; memories snapshots remain UserDefaults (**5.8-m2-memories**). |
| **M3** | Face-cluster re-key | Survival fixture is B. Candidate measurements and any evidence-backed swap are Phase 5. |

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
> - M1–M3 **preflight/regression fixtures** pass. M2 **person-key**
>   authority is cut over (`.gallery/log/<dev>/` after attach). M1
>   path-keyed leftovers, M2 memories snapshots, and M3 evidence /
>   any later swap remain **5.8** (named slices).
> - 20k: `scan_tree` exists. `e2e_20k.sh` records scan / enrich /
>   index / memories against `e2e_baselines/` and is a required
>   `rust.yml` `gallery-core` step (no tree in-repo).
> - Glue: `gazetteer_lookup` → sidecar (FFI + session + Swift); FFI scan
>   excludes `.sync-conflict`. PersonLog attach and `StableUUIDVectorTests`
>   are in `LocalGalleryTests` (gallery iOS job).

**Backlog** (left B; one register). Deferral assigns a phase —
nothing is "whenever," optional, or release-notes-only.
`shell-kit-gtk` began provisionally in Phase 3 (3.3). Music GTK is the
second consumer of the Contacts/Music kit; Gallery is the second
consumer of `media_item` and `chip_bar` (5.7). Measured reuse is 25
shared of 28 Contacts / 29 Music bindings after chrome progress. Enum exhaustiveness was
never reuse evidence. Tokens and a
record-inventory-green contacts FFI landed in 3.2 / 3.1; serialized-vCard
boundary debt remains. Rows marked 4/5/6 may start once
gallery iOS is green; they still land in that phase and do not
block C.

| Item | Phase | Notes |
|---|---|---|
| R8–R11 merge for contacts `.vcf` | **done (3.1)** | `contacts-core` + `fixtures/r8/`. Not a port of Apple `ContactMerge`. |
| Delete contacts/music `SyncConflict` twins | **done (3.1)** | One shared `localcore-conflict/support/SyncConflict.swift`. Tests read the grammar files. |
| Queue / scan cache keys: NFC or `StableId` | **done (3.4 leftover + 5.8-nfc-emit)** | `localcore-queue` NFC-normalises the path key on enqueue and lookup. Contacts still has no queue. Gallery scan-cache **lookup** and **emit** (added/modified/removed + FFI cache insert) are NFC. `failed_directory_paths` stay NFD on purpose. |
| `localcore-log` / blob through `Vfs` | **done (3.4 leftover)** | `append` / `create_dir_all` on `Vfs`. Path wrappers keep health/gallery callers. |
| Open event-type set | **done (3.4 leftover)** | Envelope stays. `valid_type` is shape-only; `known_type` is not a gate. |
| iOS contacts FS → FFI + Syncthing sheet | **done (3.4)** | `ContactsSession`; Apple CN sheet unchanged. |
| Do not copy PeopleStore dual-write | **done (3.1)** | Held. vCards on disk are the authority. |
| Token tables + R14 vocab / token codegen | **done (3.2)** | Tables + generator. Dark companions landed in 3.5. |
| Contacts UI spec + screen-id codegen | **done (3.3)** | `apps/contacts/ui-spec/screens.toml` is a semantic inventory. Generated ids do not assemble or prove screens. |
| Health web-reference curation | **done (pre-6)** | `apps/health/ui-spec/` retains screen/data contracts and deterministic fixtures; HTTP/frontend files are deleted. No Health shell or invented accent. |
| `chart-row` R4 kind + kit sparkline | **done (pre-6)** | ADR 0004 amendment + `ChartRowData`. Domain-neutral; Health GTK is still Phase 6. |
| `health-core` / `health-ffi` | **done (pre-6)** | Projection over `localcore-log` / `localcore-blob`; typed FFI. Go remains the writer. Apple `export.xml` is not parsed (ADR 0008). |
| `shell-kit-gtk` (one binding per R4 kind) | **measured reuse (4 + 5.7 + progress)** | Contacts + Music: 25 shared of 28 / 29 bindings (`measure_reuse`; `ChromeProgress` and `ProgressRow` are shared). `ChipBar` is Contacts + Gallery. HIG chrome (`PrimaryMenu`, `PreferencesDialog`, `AboutDialog`, `SearchBar`) is shared. Contacts dropped `AdaptiveShell`. Music dropped `SplitListDetail` (browse is not a list\|detail split). Gallery ∩ Music includes `MediaItem`. |
| Dark token palettes ×4 | **done (3.5 + D4)** | Sourced dark accents from each iOS `AccentColor.colorset` (contacts, gallery, music). Gallery dark *surfaces* are the authored D4 exception (`gallery.toml`). Health has no catalog; not invented. |
| Milestone C review (ADR 0004) | **done (3.6)** | Contacts needed no new kind. That validates the inventory for this slice, not the family-wide UI architecture. Shared contact display/action state continues moving into `contacts-core`. |
| `shell-kit-swift` | **measured reuse (Settings/list/filter/confirm/progress)** | 4.s1–4.s4 landed 2026-09-16; chrome `progress` + `progress-row` added 2026-09-17. Two-app pin in `check.py` (10 shared bindings). Form/field/status are Contacts production only. Grid/viewer/media stay app-owned. |
| Contacts tags / logs / full GTK fields | **done** | Routed on `contacts-gtk`. iOS tags, logs, and display-row binding landed in the contacts wrap-up. |
| GTK 4.22 list viewport | **done (4)** | GTK 4.22 wraps `ScrolledWindow` children in `Viewport`. `list_box_page()` keeps the `ListBox` handle; `.child().and_downcast::<ListBox>()` panics. |
| GTK design pass | **done (Music + Gallery kit)** | 2a–2c + HIG chrome + search + artwork; Music is Songs · Artists · Albums · Playlists with a persistent Now Playing column (mini-player when compact). Gallery kit sequence 5.1–5.7 + 5.9 year rail landed. Host rebuild of `localmusic` still needs `--features gstreamer-playback`. Reuse 25 shared of 28 Contacts / 29 Music. See [`GTK-DESIGN-PLAN.md`](GTK-DESIGN-PLAN.md). |
| Music UI spec | **done (4)** | `apps/music/ui-spec/screens.toml`. |
| Music GTK shell | **done (4)** | `music-gtk` over `music-core`. Playback is the `gstreamer-playback` feature. MPRIS and `--comet` are present. Density/polish is the design pass, not a second music rewrite. |
| iOS contacts views parse vCard text | **done** | List/search/detail/edit/export/Syncthing preview bind `TextRow` / `SearchHit` / `FieldRow` / `ContactEditDraft` / `ConflictPreview`. Swift `VCardParser` / `VCardWriter` deleted. Apple CN remains a host port. |
| `allCountries` class `P` + NE admin-0 pack | **done (B)** | Shipped (`pack_geo.py --fetch`). Rebuild if the dump updates. |
| R8–R11 for music `.m3u` | **done (4)** | `music-core` + `fixtures/r8/`. Playlist write through `localcore-vfs`. |
| Unify iOS onto `run_places` | **5.8-ios-analysis** | Places **loop** is already `run_places` (`CorePlaces` → `PlacesSession`). Remaining: `LibraryAnalysis` → `AnalysisSession`. Do not write a second Places loop. |
| M2 UserDefaults cutover | **person keys landed** | After attach, `.gallery/log/<dev>/` is authority; five person keys are not written back. Do not reopen dual-write. Memories snapshots still UserDefaults (**5.8-m2-memories**). |
| M1 thumb / widget / `library_cache` rewrite | **5.8-m1-swift-keys** | Thumbs **disk** already id-keyed. Path-keyed leftovers: in-memory thumbs, face-crop keys, widget folder map, snapshot path spelling. |
| Queue places / thumbs / EXIF | **5.8** (places landed 5.8.2) | Places uses `places_work` via `localcore-queue` (`Queue::new` + `ensure_table`). Thumbs / EXIF stay a **no-op until a deferred loop exists**. |
| M3 candidate validation / possible pack swap | **5.8-m3-evidence** | Phase 5B rejected the swap. Measure representative clustering and target-device cost first. Keep `PACK_VARIANT` unless evidence supports a later **5.8-m3-select**. |
| `ImageIOHeicDecoder` | **5.8.3 landed** | Linux leftover adapter is installed. Stays as the iOS `HostHeicDecoder` port. Do not delete the Swift type. |
| R8–R11 for gallery `.xmp` | **5.8.4 landed** | `ConflictSession` wraps `merge_sidecar_conflicts` with VFS `write_atomic` + `remove`. Leftovers: **5.8-xmp-ui** (iOS/GTK sheet), **5.8-xmp-image** (image groups). |
| gallery-ffi R6 rewrite (45 Records) | **in progress (5)** | Photo/tag/folder/people/collection windows, shells pins, kit skeleton (5.5), remaining kit screens (5.6), and 5.9 year rail / ADR+docs landed. Remaining: **5.8-ios-windows** / **5.8-ios-analysis** / **5.8-xmp-ui** / **5.8-xmp-image**. 20k CI is landed. |
| iOS callers for location windows | **5.8-ios-windows** | `folder_*` / `people_*` / `collection_*` FFI landed in 5.2. Swift still uses the old paths. |
| Memories section on `collection_structure` | **landed (5.6)** | Index has no stored `MemoryStructure` id list. Shell caches `MemoryGenerator` / `generate_memories` and drills in with `set_photo_ids_view`. |
| leftover binary rename | **landed (5.5)** | Leftover binary is `localgallery-reference` / `com.j23n.LocalGallery.Reference`. Kit binary is `localgallery` / `com.j23n.LocalGallery`. |
| `gallery-gtk` `ml` feature | **not added (5.5)** | Pin crate has none, so `--all-features` does not download `ort`. Add `ml = ["gallery-ffi/ml", "localgallery/ml"]` when Scan Photos needs ONNX. |
| year scrubber | **landed (5.9)** | Photos-tab rail on `gallery-gtk`. Years from `photo_structure` month sections via `years_from_structure` (same `ViewStructure` as `photos_flat`). Not a year FFI window; not leftover grouping; not a kit kind. |
| leftover GTK removal | **5.9 owner — owner has not agreed** | After 5.6 kit parity. Not automatic. Keep `localgallery-reference` buildable until the owner agrees. |
| `docs/screenshots/gtk-after/` | **gap (5.9)** | mutter is absent. No after-shots were captured or invented. README does not embed missing GTK shots. |
| 20k-tree as a CI gate | **landed (`rust.yml`)** | Required `gallery-core` step runs `apps/gallery/scripts/e2e_20k.sh` (`#[ignore]`, structural golden under `e2e_baselines/`; no tree in-repo). Do not invent a second job. |
| Gallery GTK 20k UI timings | **landed (local)** | `localgallery --bench --folder` + `scripts/gtk-perf.sh smoke|20k`. 20k also runs ignored `gallery-gtk` `e2e_catalog` (no display). Skip GTK half without mutter. Not a `rust.yml` job. leftover_open snapshot persist is a worker; `--bench` leftover_open_ms must stay ~0. |
| HealthKit ingest / FIT / native Health shells | **6** | iOS HealthKit + Linux FIT watch. Do not port Apple `export.xml`. Do not invent a Health accent. |
| Health Go `internal/log` / `internal/blobs` delete | **6** | After the Rust projection reproduces a real archive. |

Size: L. All Linux-container work.

---

### Phase 3 — `shell-kit` and localcontacts, the first *shell* vertical (Milestone C)

Scope corrected again after the architecture review: this proves the
**contacts core loop and Syncthing vCard merge** across two toolkits. It
exercises ADR 0004's vocabulary but does not prove family-wide `shell-kit`
reuse, executable screens, or a complete R6 boundary. Phase 2 exercised
`localcore`; it did not freeze those APIs.

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
   delete, delete-vs-modify keeps data). Queue/log leftovers landed
   with 3.4. Dual-write not copied. The two Swift
   `SyncConflict` twins are one shared file under
   `localcore-conflict/support/`; tests read the grammar fixtures.
   UniFFI declarations use display records (`TextRow` / `FieldRow`);
   `cargo test` is the gate. iOS later bound the same display records;
   the Swift vCard twins are gone. No GTK.
3. **3.2 Tokens + R14 codegen** — **done.** Light-only tables in
   `design/tokens/`. `scripts/gen_r14.py` emits R4 kinds and tokens.
   Gallery `Design.swift` aliases `GalleryTokens`. Dark companions
   landed in 3.5.
4. **3.3 `shell-kit-gtk`** — **provisional at C; second consumer later.**
   `shells/` workspace. It names one libadwaita binding per R4 kind and
   depends on no app core or `localcore`. Exhaustive enums prove
   vocabulary inventory, not that every binding is reusable or complete.
   The contacts UI spec lists semantic screens and R14 emits
   `ContactsScreen`; neither view tree consumes those ids. Music GTK
   later became the second consumer (measured reuse). Design/density
   remains the GTK design pass.
5. **3.4 iOS shell over the core.** **done.** `ContactsStore` load/save/delete
   go through `ContactsSession`. List, search, detail, edit, export, and the
   Syncthing sheet bind core display/command DTOs. `CNSyncService` stays a
   port. Apple CN sheet stays. Folder log at `.contacts/log/<dev>/`
   (`contact_saved` / `contact_deleted` / `group_resolved`). Queue
   keys NFC; log/blob through `Vfs`; event types are open.
6. **3.5 GTK + Comet.** **done.** `shells/contacts-gtk` over
   `shell-kit-gtk` + `contacts-core` (no UniFFI). Same binary,
   `--comet` (540×620 + bottom nav). Host filesystem is a path.
   This tree has no Flatpak manifest; portal suitability is unmeasured.
   Share is a file save. Sourced dark accents in the
   token tables; generator emits `ACCENT_DARK` / `accentDark` /
   `prefers-color-scheme: dark`.
7. **3.6 Milestone C review.** **done.** No new kind was needed for
   the contacts loop; unbound screens remain a gap list. Display rows,
   typed conflict disposition, draft mapping and logged actions live in
   `contacts-core`. Comet chrome follows allocated width. This is a
   contacts result, not proof that the vocabulary or kit is settled.

> **Gate (Milestone C):** localcontacts' **core loop** on iOS, Fedora
> and Comet — choose a folder, list, search, view, edit, save, resolve
> a **Syncthing `.vcf` group** (R8–R11). Apple Contacts sync remains
> iOS-only. ADR 0004 was reviewed against both shells.
> `contacts-ffi` is syntax-green for the record checker. iOS no longer
> reparses vCard text for views; Apple CN remains the host port.
> Gallery FFI windowing later emptied `conformance/r6/expected.txt`.
> Gaps that do not reopen C: Swift `shell-kit` remainder (Phase 4
> 4.s1–4.s4). The first Settings/search slice landed after C. GTK
> tags / logs / full edit fields also landed after C.

Size: L. 3.1 was Linux-container. 3.4 needs `macos-26`.

---

### Phase 4 — localmusic

Same shape, reusing `shell-kit`. Additions: the media port (AVFoundation /
gstreamer), MPRIS on Linux (ADR 0007 R15), playlist writing through
`localcore-vfs`'s atomic write, and **R8–R11** on Syncthing `.m3u`
pairs (same grammar as contacts `.vcf`).

**Landed (Music).** `music-core`, `music-ffi`, `apps/music/ui-spec/screens.toml`,
and `shells/music-gtk` (kit consumer; `--comet`; MPRIS). R8–R11 on
`.m3u` is fixture-backed. Playback is the optional `gstreamer-playback`
feature. GTK reuse after chrome progress is 25 shared of 28
Contacts / 29 Music bindings.

**Landed (4.s0 — Swift kit first slice, 2026-09-16).**
`shells/shell-kit-swift` exists. R14 emits `Generated/Kinds.swift`.
Linux `scripts/check.py` keeps the generated copy in sync, requires an
exhaustive disposition for every kind, and rejects non-SwiftUI imports
and `Color(red:)`. The package imports no app or local core. Apps pass
`ShellTokens` (card radius only); SwiftUI tint stays each app's
asset-catalog accent.

Four app-owned screens consume it: Contacts/Music Settings and the
Contacts/Music list search modifiers. 22 direct invocations replace
local settings chrome, rows, confirmation, and search. Shared today:

| Binding | Contacts | Music |
|---|---|---|
| `ShellSettings` / `ShellList` | yes | yes |
| `ShellTextRow` / `ShellActionRow` / `ShellNavRow` | yes | yes |
| `shellSearch` | yes | yes |
| `ShellStatusRow` | yes | no |
| `shellConfirmation` / `ShellFilterMenu` / `ShellList` | yes | yes |
| `ShellForm` / `ShellFieldRow` | yes (detail + edit) | no |
| `ShellChartRow` | kit + tests only (Health is Phase 6) | kit + tests only |

`KindCoverage` marks list/form/settings, the text/field/action/nav/status/chart
rows, and search/filter/confirm as `shared`; grid/detail/viewer,
media/toggle/progress, and sort/selection/primary/overflow/banner stay
`appOwned`. Navigation intents are `nativeComposition`. ADR 0004
promotes the Settings/list/filter/confirm seam; the package stays
provisional for media/grid/viewer.

Least portable Music logic (~750 lines) and the most views, so shell
work dominates — which is exactly what `shell-kit` should now be
absorbing. Music GTK was cheaper than Contacts as a *behaviour* shell;
it was not cheaper as a *designed* shell. That signal is answered.
4.s1–4.s4 closed the Swift kit remainder. Further Swift extraction
waits on Gallery (media/grid/viewer), not a second music rewrite.

Hosts without `gstreamer-1.0` stay on the mock transport. Rebuild
`localmusic` on the host with `--features gstreamer-playback` for
actual play; that is host verification, not remaining kit work.

#### Landed — `shell-kit-swift` (4.s1–4.s4, 2026-09-16)

Same lesson as GTK: do not extract a widget before a second production
consumer, and do not grow R4 to paper over app-owned chrome.

**4.s1 Field / form production (Contacts).** `ContactDetailView`
read-only `FieldRow`s (except birthday + age) go through
`ShellFieldRow`; tel/email/url stay `Link` wrappers. Delete uses
`.shellConfirmation`. `ContactEditView` is `ShellForm`; simple name
and organization strings are editable `ShellFieldRow`s. Repeaters,
birthday, notes, tags, and photo stay app-owned. `ContactEditDraft`
stays in the app. Static helpers used by unit tests (`initials`,
`structuredName`, field grouping, search highlight) stay
`nonisolated` so `@MainActor` kit views do not isolate them.

**4.s2 Filter + confirm, second consumer.** Music playlist delete uses
`.shellConfirmation`. Both `LogsView`s use `ShellFilterMenu` (empty
selection = all levels). Contacts tag chips stay chips. Music
Settings scanning stays `ProgressView` (progress is `appOwned`); no
`ShellStatusRow` there.

**4.s3 Shared diagnostics list chrome.** Both Logs screens assemble
through `ShellList` + `ShellTextRow` + `.shellSearch`. Follow-tail,
copy, share, and Contacts repeat-count trailing stay app-owned.
`LogStore` did not move.

**4.s4 Measured intersection.** `scripts/check.py` fails if a claimed
two-app binding loses a consumer, or if a new public kit type appears
without an inventory entry. Shared today (both apps):

`ShellSettings`, `ShellList`, `ShellTextRow`, `ShellActionRow`,
`ShellNavRow`, `ShellFilterMenu`, `.shellSearch`, `.shellConfirmation`.

Contacts-only production: `ShellForm`, `ShellFieldRow`,
`ShellStatusRow`. `ShellChartRow` stays kit-only until Health.
Grid / viewer / media / progress / selection / sort / primary /
overflow / banner stay **app-owned**. Provisional drops for the
Settings/list/filter/confirm seam; it does not drop for the package.

**Explicitly not in Phase 4.**

| Leave | Why |
|---|---|
| `media-item`, `grid`, `viewer` | Music library / Now Playing / artwork are host composition. Gallery is the second consumer (Phase 5). |
| `progress-row`, `selection`, `sort`, `primary-action`, `overflow`, `banner` | One-app or toolbar chrome. Extract when a second screen uses the same shape. |
| `toggle-row` | No current production screen needs it. |
| Contacts `ContactCard` avatars, section letters, search highlight | App-owned list density. `ShellTextRow` is the Settings/search seam, not the contact roster. |
| Apple `ConflictResolutionSheet` | Host port (ADR 0007 R15). Syncthing preview already binds `ConflictPreview`. |
| Music Settings "Last Synced" / About copy | ADR 0007 Settings polish, not kit work. |
| Host `gstreamer-playback` rebuild | GTK design leftover; verify on Fedora, do not reopen a music rewrite. |
| New R4 kinds | Contacts and Music needed none. Gallery proposes none until its spec is written. |
| Generated screen assembly | R14 still emits ids/kinds/tokens only. |

> **Gate (Phase 4 close):** Music core loop stays green (GTK headless
> flow + iOS `macos-26`). Swift kit: 4.s1–4.s4 landed; Linux
> `check.py` green; claimed shared bindings have two production
> consumers; ADR 0004 records the intersection; grid/viewer/media
> remain unclaimed. C does not reopen.

Size: M. 4.s1–4.s3 were Mac Swift slices; 4.s4 is the Linux
`check.py` pin plus the ADR paragraph. Host `xcodebuild` was not
run here.

---

### Phase 5 — localgallery

**Two Linux UIs.** `apps/gallery/linux` is the leftover hand-built GTK
app (own lockfile, no `ui-spec`, no `shell-kit-gtk`). Keep it buildable
and frozen as a reference. The Phase 5 GTK destination is a new
`shells/gallery-gtk` over the kit plus `apps/gallery/ui-spec/screens.toml`.
Do not keep investing design in `apps/gallery/linux/src/ui`. Sequence
and two-consumer rules: [`GTK-DESIGN-PLAN.md`](GTK-DESIGN-PLAN.md).

**Core loop, not full parity** — with a numbered gap list in the spec.

| In the Linux core loop | Explicitly not (gap list) |
|---|---|
| browse, folder tree, search | slideshow ambient music (`SlideshowMusic`, AVAudioEngine) |
| viewer, zoom, video playback | MP4 slideshow export (`SlideshowVideoRenderer`, AVAssetWriter) |
| tags, tag grid | widgets (ADR 0007 R15) |
| people **read**, person pages | face review UI |
| places, memories rail | Live Photos — no Linux equivalent exists |
| scan, tagging, face scan | |

`face-review` is named in `screens.toml` so the vocabulary is checked,
the same way Contacts names `apple-conflict`. Neither Linux shell routes
it. Slideshow, widgets, and Live Photos stay off the spec as host-only
gaps, not R4 kinds.

Plus: residual portable Swift into core; FFI stays on ADR 0003 R4
structure/window (this is where the 20k grid either scrolls or does
not); media port for video thumbnails.

Deferred gallery rows still land here as **named 5.8 slices**:
**5.8-ios-windows**, **5.8-ios-analysis** (Places loop is already
`run_places`), queues for thumbs /
EXIF (Places `places_work` landed; thumbs / EXIF stay a **no-op until a
deferred loop exists**), XMP FFI (**5.8.4 landed**; leftovers
**5.8-xmp-ui** / **5.8-xmp-image**), Linux
`HostHeicDecoder` adapter (**5.8.3 landed**; `ImageIOHeicDecoder` stays
the iOS port), M1 path-keyed leftovers
(thumbs disk already id-keyed), **5.8-m3-evidence**, and
**5.8-m2-memories**. Person-key M2, the 20k `rust.yml` job, and
**5.8-nfc-emit** (added/modified/removed + FFI cache insert are NFC;
`failed_directory_paths` stay NFD on purpose) are **landed** — do not
reopen dual-write. `PACK_VARIANT` remains until evidence supports
retiring it.

That gap list is the difference between roughly 8k and 30k lines of GTK.

#### Sequence (do not skip gates)

Windows and the spec come first. `gallery-gtk` must not invent paging,
folder grouping, or collection grouping. Photo grids reuse
`photo_window` / `set_photo_view` / `set_photo_ids_view`. New windows
are for *locations* (folders, people, collection rails), not a second
photo table.

| Slice | Gate | What lands | Must not |
|---|---|---|---|
| **5.1 Spec + R14** | **landed** | `apps/gallery/ui-spec/screens.toml` (16 screens); `GalleryScreen` in `core/localcore-ui` and `Generated/Screens.swift`; `gen_r14.py --check` green | new R4 kinds; face-review as a required GTK route |
| **5.2 Location windows** | **landed** | `LibraryIndex` has generation-checked `folder_*` / `people_*` / `collection_*` structure+window APIs; folders via `set_folders`; leftover `host.rs` calls shared `gallery-index` grouping. iOS callers are **5.8-ios-windows**. Memories rail is a **5.6** shell cache (not an index section). Default `gallery-ffi` (`ml`) still needs `openssl-devel`. | shipping `PhotoFolder` by value; GTK-only grouping; a second unbounded photo list |
| **5.3 Workspace pins** | **landed** | `shells/` pins `image = "=0.25.10"` and `uniffi` 0.32. `gallery-gtk` is a pin crate over `gallery-ffi` + leftover `localgallery` (`default-features = false`). `gallery-ffi` gained `ml` (default on; shells turns it off) so `ort` is absent from the default shells graph. One `image` / `uniffi` in `cargo tree -d`; no `gallery-view` split (`staticlib`/`cdylib` still fine as a rust dep). Kit UI landed in **5.5**; the `ml` feature is still not added. | relaxing pins; putting view models in the leftover GTK crate |
| **5.4 Freeze leftover** | **landed** (rename landed in **5.5**) | `apps/gallery/linux` README + `src/ui/mod.rs` mark the UI frozen. Leftover binary is `localgallery-reference` / `com.j23n.LocalGallery.Reference`. Removal is **5.9 owner — owner has not agreed**. | new features in leftover UI; deleting the reference before kit parity |
| **5.5 `gallery-gtk` skeleton** | **landed** | Kit UI on the pin crate: binary `localgallery`, id `com.j23n.LocalGallery`. Folder-picker, Settings dialog, logs, root switcher Folders · Collections · Photos. Photos tab binds existing `photo_structure` / `photo_window` / `set_photo_view`. Host only: config, XDG thumbs, folder watch, scan/ops via leftover `localgallery` `default-features = false`. Leftover binary is `localgallery-reference` / `com.j23n.LocalGallery.Reference`. No `ml` feature — Scan Photos without a pack is a progress/status row (models optional; no ONNX). | FlowBox grids; querying leftover UI-era photo lists; Settings as a tab; enabling leftover `ui` |
| **5.6 Remaining screens** | **landed** | folders/folder, collections + memory/person/album/event pushes, viewer + photo-info, scan progress. Memories: shell caches last `MemoryGenerator` id list (index may cache it; GTK does not cluster) and drills in with `set_photo_ids_view`. `GtkGridView`/`GtkListView` over a `gio::ListModel` that pages ≤256. Stale generation → re-read structure, never patch across generations. Viewer pushes the visible nav and stale refill targets the visible grid. | building missing windows in the shell; year scrubber (**landed 5.9**) |
| **5.7 Promotions + font** | **landed** | Gallery is the second consumer of Music `media_item` (48px default; folders 64px) and Contacts `chip_bar` (Display + Removable). `grid_gutter` / `thumb_radius` tokens. Newsreader Italic + ADR 0004 R10/R11 typeface amendment (D5). Kit `.thumb` only; `.memory-title` only in `gallery-gtk`. Flush not promoted. | promoting Flush unless Gallery uses the same section adapter; inventing tile kinds |
| **5.8 Core remainder** | parallel after 5.2; not a GTK rewrite | Named leftovers only (see below). Person-state M2 and the 20k `rust.yml` job are **landed**. | copying PeopleStore dual-write; inventing a Health accent; retiring `PACK_VARIANT` without evidence; claiming Places or 20k CI are unstarted |
| **5.9 Close-out** | **landed** (ADR + docs + year rail) | ADR 0004 GTK/Gallery reuse paragraph: three GTK apps consume the kit; reuse is **pairwise**. Year rail is `gallery-gtk` chrome (`years_from_structure` from month sections), not a kit kind. `docs/screenshots/gtk-after/` is a **written gap** (mutter absent; no placeholder PNGs). Leftover GTK removal is **5.9 owner — owner has not agreed**. Phase 5 kit sequence landed; Phase 5 as a whole stays open because named 5.8 leftovers remain. | claiming three-app reuse for tiles or the mini-player; deleting leftover without an owner yes |

#### 5.8 named leftovers

Person-state M2 and the 20k GitHub job are **landed**. Do not reopen
dual-write. Do not invent a second 20k job. Remaining work stays
named (one slice, one PR). Leftover GTK removal is **5.9 owner —
owner has not agreed**, not a 5.8 slice.

| Slice | What is still true |
|---|---|
| **5.8-ios-windows** | FFI `folder_*` / `people_*` / `collection_*` landed in 5.2. Swift still uses the old paths. |
| **5.8-ios-analysis** | Places loop is already `run_places`. Remaining: `LibraryAnalysis` → `AnalysisSession`. |
| **5.8-nfc-emit** | **landed.** Scan-cache **lookup** and **emit** (added/modified/removed + FFI cache insert) are NFC. `failed_directory_paths` stay NFD on purpose. |
| Queue thumbs / EXIF | Tagging, faces, and Places (`places_work`) use `localcore-queue`. Thumbs / EXIF stay a **no-op until a deferred loop exists**. |
| **5.8.4** XMP conflict FFI | **landed.** `ConflictSession` + `is_conflict_name`; resolve is VFS write/delete around `merge_sidecar_conflicts`. |
| **5.8-xmp-ui** | iOS sheet (no `SyncConflictGroupSheet` today). `gallery-gtk` keeps `sync-conflict-group` unrouted. Leftover `apps/gallery/linux/src/ui` stays frozen. |
| **5.8-xmp-image** | Image-file groups (`photo.heic` + `photo.sync-conflict-*.heic`): R7 visibility + which copy to keep. No merge exists. |
| Decoder seam | **5.8.3 landed.** Leftover `LinuxHeicDecoder` installs the host door. `ImageIOHeicDecoder` stays the iOS port. Pixels remain software HEVC. |
| **5.8-m1-swift-keys** | Thumbs **disk** already `{stableID}.jpg`. In-memory thumbs, face-crop keys, widget folder map, and snapshot path spelling remain path-keyed. |
| **5.8-m3-evidence** | Phase 5B rejected the pack swap. `PACK_VARIANT` stays. Measure personal-library / arm64 / device cost; do not swap ONNX. |
| **5.8-m2-memories** | Five person keys are not in UserDefaults after attach. Memories snapshots (`hiddenMemories`, `seenMemoryIDs`, `surfacedClusters`, `birthdayMemoriesEnabled`, `memoriesGeneratedDay`) still are. |
| **ops-trace** | Shared `core/localcore-trace` (`tracing` + optional subscriber). GTK binaries call `init`. Cores emit `lf` spans. |
| **trace-ios** | Swift/OSLog subscriber (or rust `init` from the iOS host). Cores already emit; iOS stays silent until this. |

Landed in this remainder (do not re-implement):

- M2 person keys: after attach, `.gallery/log/<dev>/` is authority
  (`testAttachMigratesThenRelaunchProjectsWithoutUserDefaults`).
- 20k: required `rust.yml` `gallery-core` step
  (`apps/gallery/scripts/e2e_20k.sh`).
- **5.8-nfc-emit**: added/modified/removed + FFI cache insert are NFC.
  `failed_directory_paths` stay NFD on purpose.
- **5.8.2** Places queue: `places_work` via `Queue::new` + `ensure_table`
  (same `gallery-cache.sqlite` when the geo cache sits beside it).
- **5.8.3** Linux leftover `HostHeicDecoder`: `LinuxHeicDecoder` /
  `linux_heic_decoder`, threaded through `AnalysisRequest`. Pixels stay
  software HEVC. Thumbs / EXIF queues stay a no-op until a deferred loop
  exists.
- **5.8.4** XMP conflict FFI: `ConflictSession` (`conflict_rows` /
  `conflict_preview` / `resolve_group`) + `is_conflict_name`. Merge stays
  `gallery-meta::merge_sidecar_conflicts`. Image groups and the
  sync-conflict-group sheet stay **5.8-xmp-image** / **5.8-xmp-ui**.

#### 5.1 screen inventory (R4 kinds only)

Mirror Contacts/Music: semantic ids, kinds, sections, affordances.
Comments record chrome (primary menu → Settings), not geometry.

| id | kind | Linux route | Window |
|---|---|---|---|
| `folder-picker` | detail | 5.5 | none (host folder chooser) |
| `folders` | list | 5.6 | `folder_structure` / `folder_window` |
| `folder` | grid | 5.6 | folder children + `set_photo_ids_view` |
| `photos` | grid | 5.5 | existing `photo_*` / `set_photo_view` + `tag_*` |
| `collections` | list | 5.6 | `collection_structure` / `collection_window` |
| `memory` | grid | 5.6 | memory ids → `set_photo_ids_view` |
| `people` | grid | 5.6 | `people_structure` / `people_window` |
| `person` | grid | 5.6 | `photo_ids_for_tag` → `set_photo_ids_view` |
| `events` | list | 5.6 | collection section or event rows |
| `album` | grid | 5.6 | tag/album ids → `set_photo_ids_view` |
| `viewer` | viewer | 5.6 | photo id + host decode |
| `photo-info` | detail | 5.6 | scanner metadata + tags/people |
| `settings` | settings | 5.5 | Folder, Scan (`progress-row`), Diagnostics, Info last |
| `logs` | list | 5.5 | kit `ListScreen` |
| `sync-conflict-group` | detail | **5.8-xmp-ui** | FFI `ConflictSession` landed (5.8.4); sheet unrouted |
| `face-review` | list | **unbound gap** | none until the core-loop table changes |

#### 5.2 window rules

Already landed: `ViewStructure` / `ViewError` / `MAX_VIEW_WINDOW`,
`photo_structure` / `photo_window` / `set_photo_view` /
`set_photo_ids_view`, `tag_structure` / `tag_window`.

**Landed (5.2).** Same generation, same bound, same stale refusal:

1. **Folders.** Scanner already returns a flat `ScannedFolderHost` list
   (`parent_index`, `photo_start`, `photo_count`) so the recursive
   `PhotoFolder` never crosses the wire twice. `LibraryIndex` takes
   that flat tree via `set_folders` and exposes
   `folder_structure(parent_id)` + `folder_window(...)` as
   `GalleryTextRow` (name, count). Photo cells of a folder go through
   `set_photo_ids_view`, not a new media window. Do not return
   `PhotoFolder` from FFI.
2. **People.** `LibraryIndex` already caches the `People/…` suggestion
   list. `people_structure` / `people_window` are `GalleryTextRow`
   (name, count). Person photos: `photo_ids_for_tag` +
   `set_photo_ids_view`.
3. **Collections hub.** Leftover `collection_groups` /
   `leaf_tags` / event-folder grouping live in shared `gallery-index`
   (leftover `host.rs` re-exports). `collection_structure` sections are
   People / Events / Albums (and other tag namespaces). Memories are a
   **5.6** shell cache of `generate_memories` / `MemoryGenerator`, not
   an index section. Rows are ids + `GalleryTextRow`. GTK does not
   cluster.

iOS callers for the new windows land in **5.8-ios-windows**. The FFI
is shared (not a GTK-only projection); Swift still uses the
pre-window paths until that slice. Do not add a second grouping
algorithm on iOS.

#### 5.5–5.6 shell rules

- Root tabs: Folders, Collections, Photos (that order). Settings is the
  primary-menu dialog, not a tab.
- Compact/wide layout: [`GTK-DESIGN-PLAN.md`](GTK-DESIGN-PLAN.md)
  Phase 5 table.
- Thumbnails: resolve `thumbnail_ref` in factory `bind`, cancel in
  `unbind`. Host XDG cache stays in leftover `localgallery` lib.
- Reuse after 5.7: Gallery ∩ Music includes `MediaItem`; Gallery ∩
  Contacts includes `ChipBar`. Flush is still Music-only.
- Year rail (5.9): Photos tab only. Overlay on `photos_scroll`. Years
  from `photo_structure` month sections via `years_from_structure`.
  Same `ViewStructure` replaces the model and the rail. Not on
  folder/memory/person/album grids. Not a kit binding.

Size: L–XL. Each slice is one work item: one fixture or `--check`, one
review question.

---

### Phase 6 — localhealth

Full Rust port, both shells — and **materially smaller than r1**, because
ADR 0008 retires the riskiest component instead of porting it.

**Started.** `health-core` (disposable SQLite projection, portable-v1,
no `export.xml` parser) and `health-ffi` exist. `chart-row` is in the
vocabulary and the GTK kit. The Go `archive` CLI remains the writer.
There is no Health shell and no Health accent — do not invent one so
the GTK design pass has a fourth color.

1. `health-core` over `localcore-log` / `localcore-blob` (Phase 2).
   Projection to SQLite. **Done as a crate**; cutover still waits on
   reproducing a real archive.
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
   The static IA and data-contract brief is `apps/health/ui-spec/`.
   `chart-row` is the R4 amendment (native kit sparkline, not a Chart.js
   port). Health GTK/iOS shells are still this phase. The GTK design pass
   does not include them.
7. Go tree deleted once the projection reproduces a real archive. The
   curated static UI reference stays until that amendment is written.

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

| Work | Environment | Verified by |
|---|---|---|
| `core/**`, `shells/**`, `apps/gallery/core/**`, `apps/gallery/linux/**` | Fedora container | the relevant `cargo test` workspace |
| `apps/health/**` | Fedora container | `CGO_ENABLED=1 GOFLAGS=-mod=vendor go test ./...` |
| `docs/**`, `conformance/**` | Fedora container | conformance checks and textual consistency searches |
| FFI surface change | container, then Mac | Linux `swift build` shim (0.4), then `macos-26` |
| app Swift | Mac | `macos-26` by default; Mac VM interactively when more than one round is needed |

**Task shape:** one requirement, one fixture, one PR. "Make `contacts-core`
satisfy ADR 0005 R7, here is the fixture directory, the check must go red to
green." Reviewable in minutes because the check is the review — for the 62
requirements where that is true. For the rest, the PR template asks the
review question (ADR 0007 R16) and you read the diff.

---

## 7. Risks, ranked

1. **Face-model candidate quality.** Licensing makes SFace + YuNet eligible
   for evaluation, but alignment, personal-library clustering, migration
   outcome, and target-device cost remain unmeasured. `PACK_VARIANT` stays
   until that evidence exists.
2. **ADR 0003 R4's windowed boundary not being enough.** If a 20k grid still
   stutters through UniFFI, the fallback is the current arrangement — shell
   holds the structs — which costs ADR 0003 R6 and with it the only structural
   enforcement of ADR 0001 R4.
3. **The slot vocabulary not surviving a second toolkit.** Phase 3 held for
   Contacts GTK. Music GTK reused the same kinds without a new one.
   SwiftUI `shell-kit` now has a measured Settings/list/filter/confirm
   seam on both iOS apps; form/field/status are Contacts-only;
   grid/viewer/media stay unclaimed. The GTK design pass is a
   density/chrome problem, not a missing-kind problem — do not grow R4
   to paper over clamp and header-bar bugs.
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

Each phase leaves something shippable. Phases 0–2 removed cloud integration
and network geocoding; legacy placeholder/download compatibility fields
remained explicit debt. The first year's larger product risk is concentrated
in Phase 3, which is the cheapest app.

---

## 9. What I would do first

**A, B, Phase 3 (through 3.6 / Milestone C), the contacts iOS wrap-up,
the Music GTK core loop, and the Swift kit remainder (4.s0–4.s4) are
done.** `health-core` / `health-ffi` / `chart-row` started Phase 6
without a Health shell. Next engineering moves, in parallel:

- **GTK design pass** — **Music stage landed.** Primary menu,
  preferences/about, Contacts split+detail+conflict, global search,
  Music browse switcher + Now Playing column + cover art. Host rebuild
  of `localmusic` still needs `--features gstreamer-playback`. See
  [`GTK-DESIGN-PLAN.md`](GTK-DESIGN-PLAN.md).
- **Phase 4 remainder** — **done (4.s1–4.s4).** Swift kit seam is
  measured for Settings/list/filter/confirm. Do not extract
  media/grid/viewer here; that is Gallery.
- **Phase 5** — kit sequence 5.1–5.7 + 5.9 year rail / ADR+docs landed.
  Phase 5 as a whole stays open because named 5.8 leftovers remain:
  **5.8-ios-windows**, **5.8-ios-analysis** (Places is already
  `run_places`), thumbs/EXIF queues (no-op until a deferred loop exists),
  **5.8-xmp-ui**, **5.8-xmp-image**, **5.8-m1-swift-keys**,
  **5.8-m3-evidence**, **5.8-m2-memories**. Person-state M2, the 20k
  `rust.yml` job, **5.8-nfc-emit**, XMP FFI (5.8.4), and the Linux HEIC
  adapter (5.8.3) are landed. Year scrubber **landed (5.9)**. Leftover
  GTK removal is **5.9 owner — owner has not agreed**.
  `docs/screenshots/gtk-after/` is a written gap (mutter absent; no
  placeholder PNGs). The `ml` feature was not added. Memories rail is a
  5.6 shell cache. Do not restyle `apps/gallery/linux`.
- **Phase 6 remainder** — HealthKit, FIT, native shells, retire Go.
  Do not invent a Health accent.

**You, now**

1. Host-test **`gallery-gtk`** (`localgallery` / `com.j23n.LocalGallery`)
   on a real photo folder. That is the kit core loop. Keep leftover
   `localgallery-reference` for side-by-side only; do not restyle it.
2. Keep the four **root** workflows green on tip.
3. The 20k e2e is a required `rust.yml` `gallery-core` step
   (`apps/gallery/scripts/e2e_20k.sh`; `LOCALGALLERY_E2E_RECORD=1`
   rewrites the golden). Run it locally the same way.
4. Remaining Phase 5 work is the **named 5.8 leftovers** (mostly iOS /
   evidence) plus leftover GTK removal when the owner agrees.
   Phase 6 is Health. The gazetteer pack is `allCountries` class `P`.
5. When a review or host run changes what is true (leftover vs kit
   Gallery, GTK 4.22 viewport, two-consumer reuse, chart-row, …),
   edit this file in the same change as the finding.
