# ADR 0003 R6 — shell boundary surface

- Status: Accepted (design; gallery FFI is not rewritten here)
- Date: 2026-09-12
- Revised: 2026-09-13 (`contacts-ffi` record inventory is green); 2026-09-14 (Milestone C: rows produced in `contacts-core`); 2026-09-16 (typed Contacts drafts replace serialized vCard reads and writes; Gallery generation-scoped structure DTOs)
- Parent: [0003-app-core.md](0003-app-core.md) R6, [0004-ui-spec-and-shells.md](0004-ui-spec-and-shells.md) R4

## Scope

What may cross from an app core to a shell, and how a `uniffi::Record`
is judged. This is the type-system guard in ADR 0003 R6. It is written
now so every later vertical designs against it.

It is **not** a gallery FFI rewrite. `gallery-ffi` stays as it is. The
conformance check starts red and pins that red so a new Record cannot
hide behind the known ones. `contacts-ffi` is green for display records and
explicit command DTOs. `TextRow`, `FieldRow`, and typed `ConflictRow` are
produced in `contacts-core`; full `ContactEditDraft` and
`SaveContactCommand` values carry only editable intent. Normal reads and
writes no longer cross as serialized vCards. `export_vcard_text` remains an
explicit export operation. CI runs the same checker over
`core/contacts-ffi/src` without `--expect-violations`.

## What may cross

Only these values cross to a shell:

- **ids** — opaque keys the shell holds and hands back
- **strings** — already formatted, already chosen
- **booleans**
- **numbers**
- **enumerated variants**
- **lists of those**
- **display records** — structs whose every field is a display-ready
  value for *exactly one* item kind in ADR 0004 R4
- **structure DTOs** — generation-scoped section ids, ordered item ids, and
  action availability; never item content
- **command DTOs** — explicit editable fields or action arguments sent back
  to the core
- **host-port DTOs** — the minimum structured values needed by a platform
  service such as Contacts or media playback

A bare `Vec<String>` of ids is structure (ADR 0003 R4) and is allowed.
A struct that contains a list is not a display record: no ADR 0004 item kind
carries a list. A structure DTO may contain lists of section/action metadata
and item ids, but never formatted item rows. Lists of formatted rows are the
collection R4 forbids marshaling in one shot.

**No domain entity crosses**, under any name or serialization. A type an app
core keys domain logic on — a photo, contact/card, folder, memory, face,
cluster, sidecar, scan outcome, or generation-inputs snapshot — does not
appear in the exported surface as a Record, JSON string, vCard string, or
other whole-domain payload. Explicit DTOs are preferable to opaque
serialization because their purpose and fields are reviewable.

## Display-record taxonomy

A display record is a struct that a shell can bind to **one** slot
without choosing a field, formatting a field, or learning a domain
rule. Every field is optional or required exactly as that slot
declares. An `id` MAY appear on any display record so the shell can
key the row; it is not a domain key the shell interprets.

| Slot kind | Required fields | Optional fields |
|---|---|---|
| `text-row` | `title` | `id`, `subtitle`, `trailing`, `trailing_value`, `leading_symbol`, `symbol`, typed action/disposition |
| `media-item` | one of `thumbnail`, `thumbnail_ref`, `thumbnail_id` | `id`, `label`, `accessibility_label`, `badge`, and the unused thumbnail aliases |
| `field-row` | `label`, `value` | `id`, `editable`, `editability` |
| `toggle-row` | `label`, and one of `on`, `on_off`, `state` | `id` |
| `action-row` | `label`, `role`, `enabled` | `id` |
| `nav-row` | `label`, `destination` | `id`, `trailing`, `trailing_value` |
| `progress-row` | `label`, and one of `fraction`, `determinate`, `indeterminate` | `id`, `cancel`, and the unused progress aliases |
| `status-row` | `message`, `severity` | `id` |

Field types on a display record are only: strings, booleans, numbers,
enumerated variants, and `Option` of those. A nested Record is a
domain (or another slot) smuggled inside this one. A `Vec` is a list,
and no slot kind has a list field.

A Record that fails either test — field types, or the one-slot field
set — is a violation. Telemetry structs of numbers (`ScanTimings`,
`FaceStats`) are still violations: they are not a slot. Two ids in a
struct (`FaceMergeDecision`) are still a violation: they are not a
slot. `MemoryRecord` having a `title` does not make it a `text-row`.

Enums may cross. Objects (`LibraryIndex`, `ScannerSession`) are
handles, not records. Errors are ADR 0003 R8, not this taxonomy.

Structure, command, and host-port records carry an explicit
`R6 role: structure DTO`, `R6 role: command DTO`, or
`R6 role: host-port DTO` documentation marker. The checker permits primitive,
enum, list, and nested exported DTO fields for those roles while continuing
to reject an unmarked record that is not one display slot. Structure DTO
lists are ids/metadata only, never display records. The marker states purpose;
semantic review still verifies that the DTO is not a renamed domain entity.

## Known-red inventory — `gallery-ffi`

Every current `#[derive(uniffi::Record)]` in
`apps/gallery/core/gallery-ffi/src/**/*.rs` fails the test. The
mechanical pin is `conformance/r6/expected.txt`. Grouped by why they
exist, not by how close they are to green:

**Scan / photo domain.** `ScanPhoto` is the photo record the rest of
the surface is built around. `ScanTag`, `ScanRegion`, `ScanLocality`
(the enum is allowed; the records that carry it are not),
`ScanFolderNode`, `ScanContentVersion`, `ScanSidecarRow`,
`ScanTimings`, `ScanOutcomeRecord`, `ScanRequest`, `SnapshotRecord`,
`WallClock`, `ImageMetadataRecord`,
`SidecarParseRecord`, `SidecarViewRecord`.

**Library / memory domain.** `MemoryGenerationInputs` ships the
library into the engine. `MemoryRecord`, `ScheduledMemoryRecord`,
`MemoryLeafFolder`, `MemoryContact`, `MemoryPersonLink`,
`MemoryDateEntry`, `TagSuggestionRecord`, `LibraryIndexSummary`,
`LibraryTagSuggestions`. M2 person-log projection: `PersonStateRecord`,
`PersonKeyedString`.

**Face / cluster domain.** `FaceRef`, `ClusterSummary`,
`FaceAssignmentRecord`, `FacePhotoRecord`, `FaceMergeCandidate`,
`FaceMergeDecision`, `MergeProposal`, `SplitResult`, `FaceRunSummary`,
`FaceStats`, `FaceLibraryStats`, `ReclusterSummary`,
`SidecarWriteReport`.

**Places, pixels, packs, tagging counters.** `PlaceWrite`,
`HeicPixels`, `ModelPackInfo`, `PackResolution`, `TaggingStats`,
`TaggingRunSummary`.

That is the honest state. A later commit that deletes or reshapes a
row updates `expected.txt`. A later commit that adds a Record updates
`expected.txt` too, unless the new type is a genuine display record
for one slot kind — then it is green and must not be listed.

## Conformance

`conformance/r6/check.py` enumerates `uniffi::Record` and other
exported types. The default gallery-ffi run is red while any listed
Record remains. `--expect-violations` succeeds only when the
violation set equals `expected.txt`, so a new domain Record cannot
hide. `--self-test` exercises the parser and the slot taxonomy.
`contacts-ffi` is a second `--src` and its display/command inventory must stay
green. Semantic review additionally checks exported string payloads and
command/host DTO purpose; the parser cannot establish those properties.

The check going green is the gallery (and then each later app) FFI
rewrite, not this document.
