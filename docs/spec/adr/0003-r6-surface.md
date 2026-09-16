# ADR 0003 R6 — shell boundary surface

- Status: Accepted
- Date: 2026-09-12
- Revised: 2026-09-13 (`contacts-ffi` record inventory is green); 2026-09-14 (Milestone C: rows produced in `contacts-core`); 2026-09-16 (typed Contacts drafts replace serialized vCard reads and writes; Gallery generation-scoped structures, explicit host ports, and typed commands; iOS Contacts binds display rows and deletes the Swift vCard twins)
- Parent: [0003-app-core.md](0003-app-core.md) R6, [0004-ui-spec-and-shells.md](0004-ui-spec-and-shells.md) R4

## Scope

What may cross from an app core to a shell, and how a `uniffi::Record`
is judged. This is the type-system guard in ADR 0003 R6. It is written
now so every later vertical designs against it.

`gallery-ffi` and `contacts-ffi` are green for display records, explicit
structure, typed commands, and narrowly scoped host ports. `TextRow`,
`FieldRow`, and typed `ConflictRow` are
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
- **command DTOs** — explicit editable fields, action arguments, or compact
  scalar outcomes of one named command
- **host-port DTOs** — the minimum structured values needed by a platform
  service such as Contacts, filesystem scan/metadata, persistence, image
  decode, or media playback

A bare `Vec<String>` of ids is structure (ADR 0003 R4) and is allowed.
A struct that contains a list is not a display record: no ADR 0004 item kind
carries a list. A structure DTO may contain lists of section/action metadata
and item ids, but never formatted item rows. Lists of formatted rows are the
collection R4 forbids marshaling in one shot.

**No core domain entity crosses**, under any name or serialization. A host
descriptor may carry the minimum filesystem, metadata, pixel, or persistence
values a platform service owns, but it is not a domain object and cannot
carry core behavior. In particular, a full library/generation snapshot does
not cross merely to call another core subsystem; those calls reuse an opaque
retained handle. Explicit DTOs are preferable to opaque serialization because
their purpose and fields are reviewable.

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

## Gallery inventory

The legacy Gallery record names are no longer exported. The replacement
surface names its boundary role:

- `Scanned*Host`, `Host*`, `SidecarHostView`, and
  `SnapshotHostDocument` are filesystem/media host-port values.
- `*Command*` values are typed action inputs or compact outcomes.
- `*Structure`, `ViewStructure`, and `ViewSection` carry opaque ids,
  generations, section metadata, and action availability.
- `GalleryMediaItem` and `GalleryTextRow` are bounded display content.

Normal memory generation no longer accepts `MemoryGenerationInputs`: it takes
an opaque `LibraryIndex` handle plus bounded platform context and reuses the
retained photo table. Photo/tag grids read structure first and content only
through generation-checked windows. `conformance/r6/expected.txt` therefore
contains no Gallery record.

## Conformance

`conformance/r6/check.py` enumerates `uniffi::Record` and other
exported types. The default Gallery run is strict green.
`--expect-violations` succeeds only when the
violation set equals `expected.txt`, so a new domain Record cannot
hide. `--self-test` exercises the parser and the slot taxonomy.
`contacts-ffi` is a second `--src` and its display/command inventory must stay
green. Semantic review additionally checks exported string payloads and
command/host DTO purpose; the parser cannot establish those properties.

Any new Gallery violation must fail strict CI rather than enter a permanent
expected-red inventory.
