# localfiles — architecture specification

The goal state for the localfiles family. These documents describe what the
system **is**, not how to get there from where the code stands today. They
live in this monorepo at `docs/spec/` and are intended to be sufficient to
begin work without further design decisions.

**Revision r2.** The decisions this revision changed are listed at the end.

## The product, in one sentence

> Each app projects a user-selected folder of files into a browsable,
> editable view, keeps that view correct as other devices change the folder,
> and never contacts a network.

Four apps, four file formats, one machine:

| App | Files | Domain |
|---|---|---|
| localgallery | JPEG / HEIC / PNG + `.xmp` sidecars | photos, people, places, memories |
| localcontacts | `.vcf` | contacts |
| localmusic | audio + `.m3u` | tracks, playlists |
| localhealth | NDJSON event log + blobs | health archive |

Three platforms, two toolkits:

| Platform | Shell |
|---|---|
| iOS | SwiftUI |
| Linux desktop (GNOME) | GTK4 / libadwaita |
| Mecha Comet (Mechanix OS) | the same GTK4 shell, adaptive layout |

Folders are synchronised between devices by Syncthing / SyncTrain. No app
participates in synchronisation; each app is a correct reader and writer of a
folder that changes underneath it.

Files are **on disk or absent** (ADR 0005 R2). No app understands cloud
placeholders, download states, or on-demand materialisation. What the
synchroniser has put in the folder is the whole of what exists.

## Documents

| ADR | Subject |
|---|---|
| [0001](adr/0001-layering.md) | Layered architecture and boundaries |
| [0002](adr/0002-localcore.md) | `localcore` — the folder projection engine |
| [0003](adr/0003-app-core.md) | App cores — domain logic and view models |
| [0003 R6](adr/0003-r6-surface.md) | Display-record surface (`gallery-ffi` known red) |
| [0004](adr/0004-ui-spec-and-shells.md) | UI specification, slot vocabulary, shells — kinds in [`ui/vocabulary.toml`](ui/vocabulary.toml); contacts screens in [`apps/contacts/ui-spec/`](../../apps/contacts/ui-spec/); tokens in [`design/tokens/`](../../design/tokens/) |
| [0005](adr/0005-files-sync-and-state.md) | Files, state tiers, conflicts, reconciliation |
| [0006](adr/0006-derived-data.md) | Derived data, capabilities, model packs |
| [0007](adr/0007-product-conventions.md) | Vocabulary, screens, host surfaces, testing |
| [0008](adr/0008-health-ingestion.md) | Health ingestion |

## How to read these

Requirements use RFC 2119 keywords. **MUST** and **MUST NOT** are conformance
conditions: an app that violates one does not conform. **SHOULD** marks a
strong default that may be departed from with a recorded reason in the
repository. **MAY** is genuinely optional.

Each document ends with a **Conformance** section listing what to check. An
implementation conforms when every check in every document passes.

Requirements are cited as `ADR 0002 R4`.

## The five things most likely to be got wrong

Each of these is a place where the obvious reading of a requirement is the
expensive one.

1. **Structure and content cross the boundary separately** (ADR 0003 R4).
   A view model hands back ordered *ids* and a generation; formatted values
   are fetched one visible window at a time. Returning a formatted collection
   marshals the whole library on every change.
2. **No domain record reaches a shell** (ADR 0003 R6). This is not stylistic:
   it is the only mechanism that makes ADR 0001 R4 mechanically true rather
   than merely required.
3. **Convergence is by recorded decision, not by identical arithmetic**
   (ADR 0006 R15–R17). A capability reads what the file already says, retains
   decisions inside a retention band, defers to a newer pack, and writes
   nothing when nothing changed.
4. **The no-network rule is checked over the dependency graph**
   (ADR 0002 R13), against a one-entry allowlist. A source grep passes a
   crate that opens a socket, which is how a live reverse-geocoding client
   survived inside a no-network codebase.
5. **Changing an identity rule or a derived-data key is a migration**
   (ADR 0005 R19), and a migration is not done until existing state survives
   it with a fixture to prove it.

A sixth, for anyone extending the spec: **a requirement that no check can
express is not thereby weaker**, it is on ADR 0007 R16's list and costs
review time instead. Roughly a third of this document is in that category,
and it is the third that gets broken.

## Recorded spike decisions

Written answers live in `docs/spec/spikes/`.

1. **Face licence — outcome 1.** Embedder is OpenCV Zoo SFace, detector is
   YuNet. ADR 0006 R12 and R13 stand. One pack. Changing the models re-keys
   every face cluster (M3, ADR 0005 R19).
2. **No Flatpak, no portal.** Linux is a native GTK binary over the host
   filesystem. Folder grant is a path; share is a file save.
3. **ISA drift is assumed negligible.** ADR 0006 R16's ε is a conventional
   retention band, not a measured cross-ISA margin.

## What is deliberately absent

No sequencing, no migration steps, no per-repository task lists. A spec that
describes the destination stays true while the work happens; the route is
[`IMPLEMENTATION-PLAN.md`](../IMPLEMENTATION-PLAN.md) and goes stale on
purpose. There is no separate drop-in note.

The one exception is that ADR 0005 R19 requires migrations to *exist* as
first-class work with fixtures. It does not say when they happen.

## What changed at Milestone C (2026-09-14)

Held ADR 0004 against both contacts shells. The closed vocabulary survived.
The amendments are the stronger claims that did not:

| Was | Now | Because |
|---|---|---|
| A spec screen with no view fails the build | An R4 *kind* with no binding fails the build; an unbound *screen* is a gap | C built the core loop, not tags/logs; `ContactsScreen` is unused by both view trees |
| Copy is authored once in the spec | Titles in the spec; C-loop body copy in the app core | R14 does not generate copy; both shells now call `contacts-core` display functions |
| Comet is an adaptive layout | Same GTK binary; `--comet` plus chrome that follows width | 3.5 shipped a flag and a fixed size |
| GTK and UniFFI surfaces "identical" | Same operations; display records produced in `contacts-core` | 3.5 formatted rows in the GTK crate |
| `shell-kit` on every platform | `shell-kit-gtk` exists; `shell-kit-swift` is Phase 4 | iOS contacts views are hand-rolled |

## What changed in r2

| Was | Now | Because |
|---|---|---|
| View models return formatted collections | Structure (ids + generation) and windowed content are separate reads — ADR 0003 R4 | UniFFI copies; localgallery already answers in ids for exactly this reason |
| — | No domain record crosses to a shell — ADR 0003 R6 | ADR 0001 R4 was otherwise unenforceable; the existing GTK shell violates it in six files |
| Five layers | Six, adding `shell-kit` — ADR 0001 R1 | ADR 0004 R6 requires per-platform bindings shared by four apps, and nothing could hold them |
| — | Two Cargo workspaces — ADR 0001 R9 | keeps GTK features out of app cores and makes the graph check one command |
| Results must be bit-identical across devices | Results must converge, via idempotent writes, retention bands, and pack precedence — ADR 0006 R15–R17 | bit-identity across instruction sets is unachievable and was never what prevented conflicts |
| — | Every thresholded decision carries a retention band — ADR 0006 R16 | tagging had one; face detection and auto-tag matching did not; ISA drift is assumed negligible |
| — | Newer pack wins, older defers — ADR 0006 R17 | version skew is the one divergence no determinism rule absorbs |
| "`cargo tree` contains no networking crate" | Graph check against a one-entry allowlist — ADR 0002 R13 | the r1 bullet was unsatisfiable the day it was written |
| Temp prefix unspecified | A `Vfs` parameter, with an ignore rule per app — ADR 0002 R3 | a shared prefix would have four apps writing `.gallery-tmp-` |
| "A missing binding MUST fail the build", with no mechanism | Generated enums, closed to three items — ADR 0004 R14 | the plan forbade codegen; both could not hold |
| Host differences undefined | Closed host-surfaces table — ADR 0007 R15 | ADR 0001 R6 read strictly forbade iOS-only widgets and read loosely bounded nothing |
| — | Uncheckable requirements named and reviewed — ADR 0007 R16 | roughly a third of this spec is not mechanically checkable, and those are the ones that get broken |
| — | Log growth and compaction stated per app — ADR 0005 R18 | an append-only log synced forever had no bound |
| — | Migrations are first-class, with survival fixtures — ADR 0005 R19 | four were implied by r1 and scheduled nowhere |
| `CONVENTIONS.md` alongside the ADRs | Retired; ADR 0007 is the sole home | six of its sections contradicted these documents, including a second vocabulary table |
| Apple Health via streaming XML export | Via HealthKit, batched into blobs — ADR 0008 | the export format discards identity and cannot express deletion |
| Locale handling unstated | One language, core formatting tables — ADR 0007 R19 | already true in localgallery; stating it stops an implementer reaching for ICU |
