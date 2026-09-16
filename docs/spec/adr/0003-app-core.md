# ADR 0003: App cores — domain logic and view models

- Status: Accepted
- Date: 2026-09-11
- Revised: 2026-09-11 (r2); 2026-09-13 (`contacts-core` headless, Phase 3.1); 2026-09-13 (3.4 iOS writes through FFI); 2026-09-14 (Milestone C: shared display surface); 2026-09-16 (typed Contacts edit boundary)

## Scope

The per-app layer: what a file means, what the user may do to it, and what
every screen currently shows. One app core per app, in Rust, shared by both
shells.

## Requirements

**R1.** Four app cores exist: `gallery-core`, `contacts-core`, `music-core`,
`health-core`. Each is the sole owner of its file formats and its domain
rules, and each serves both shells unchanged. `contacts-core` landed
headless in Phase 3.1 (`core/contacts-core`, syntax-green
`contacts-ffi`).
The iOS shell reads and writes typed drafts through `contacts-ffi`; normal
loads no longer parse vCard text. The GTK shell links `contacts-core`
(Phase 3.5). List, detail, conflict, draft, tag, and logged
save/delete/resolve actions are functions on `contacts-core`; FFI copies
display rows and explicit command DTOs onto UniFFI. Save, delete, tag, and
group-resolve actions best-effort append to `.contacts/log/<dev>/` after the
authoritative file mutation. `gallery-core` remains
at `apps/gallery/core`. The other two cores are later phases.

**R2.** An app core owns:

- **Parsing** — file bytes to domain record, and the reverse.
- **Mutation** — writes, preservation-first: the core replaces only what it
  previously wrote and never discards a field it does not understand.
- **Domain policy** — eligibility, ordering, grouping, validation, merge
  rules, and the order operations run in.
- **Enrichment** — derived data, via the substrate in ADR 0006.
- **View models** — R4 and R5.

**R3.** An app core MUST NOT reference a UI toolkit or a platform API. Host
capabilities are reached through ports it declares (ADR 0001 R5).

**R4.** **View models.** For every screen in its UI spec, an app core exposes
a view model. A view model is read in two steps, and the split is mandatory:

*Structure — cheap, whole-screen, read on every change:*

- a **content state**: `loading`, `empty`, `content`, or `error`, where
  `empty` is distinguishable from `not yet loaded`;
- ordered **sections**, each carrying a slot kind from ADR 0004 R4, a title,
  and an ordered list of **item ids**;
- the **actions** available, each with an enabled or disabled state and, when
  disabled, a reason string;
- a **generation**: a monotonically increasing counter, bumped whenever any
  id list or action state changes.

*Content — fetched per visible window:*

- `items(section, range, generation)` returns the already-formatted values
  for that slot kind over that range of the section's id list.
- A call whose `generation` is no longer current returns a typed staleness
  refusal rather than data. The shell re-reads structure and retries.

**A view model MUST NOT return formatted item content for a whole
collection.** Structure crosses the boundary whole; content crosses one
visible window at a time.

Contacts is an exception on size, not on principle: `list_rows` returns
the whole (small) collection. The two-step shape remains mandatory for
gallery (Phase 5). Milestone C did not implement view models.

A window fetch MAY be asynchronous, and a row MAY render as a placeholder
until its values arrive. This is the pattern the family already uses for
thumbnails, where a cell's load is a task keyed on the item and the cell
size; extending it to row text costs nothing new and is what keeps the
boundary off the render path.

**R5.** A view model MUST deliver values ready to display. Ordering,
filtering, grouping, truncation, pluralisation, date and number formatting,
label selection, and enablement are all decided in the app core. A shell
receives strings and renders them.

**R6.** Boundary types are primitives, enumerated variants and explicit DTOs
in one of three roles:

- **display records** carrying values ready for one ADR 0004 slot;
- **command DTOs** carrying user intent back to the core; or
- **host-port DTOs** carrying the minimal structured data a platform service
  requires.

**No domain entity or serialized whole-domain escape hatch crosses a language
boundary.** A type that the app core keys domain logic on does not appear on
the UniFFI wire under another name or as JSON/vCard text. DTOs are allowed
because rich editors and host adapters need structured commands; banning them
would merely encourage opaque strings. A GTK shell that links the crate still
MUST NOT format, sort, or decide enablement.

The display-record taxonomy, the slot-field vocabulary, and the current
`gallery-ffi` inventory are in [0003-r6-surface.md](0003-r6-surface.md).
That document is the design; it does not rewrite the gallery FFI.

**R7.** All user intent enters the app core as a declared **action**. A shell
MUST NOT mutate domain state directly. Actions are total: an action that
cannot proceed returns a typed refusal, never a panic and never silence.

**R8.** Errors crossing to a shell are typed variants carrying a
display-ready message and a flag for whether the user can act on them. A
shell MUST NOT parse an error string to decide behaviour.

**R9.** Long work is started, observed and cancelled through the app core:
start, cancel, progress, completion. Progress is reported on core-owned
threads; hopping to a UI thread is the shell's business. An operation that is
already running MUST refuse a second start rather than queue or race.

**R10.** An app core MUST be exercisable with no shell present. Every
behaviour in R2 is reachable and assertable from a test binary.

**R11.** Where two apps need the same domain concept, it belongs in
`localcore`, not duplicated across app cores. Logging, identity, atomic
write, scanning, indexing and reconciliation are `localcore` (ADR 0002).

## Conformance

- Each app core builds and its full test suite runs on a host with no UI
  toolkit installed.
- Every screen in each app's UI spec resolves to exactly one view model.
- No type crossing the FFI or the GTK binding carries a domain entity or its
  serialized representation; a test enumerates the exported surface and
  asserts R6. The current syntax checker cannot detect serialized escape
  hatches and is therefore necessary but insufficient. Gallery starts red:
  `conformance/r6/check.py` pins today's `gallery-ffi` Records
  ([0003-r6-surface.md](0003-r6-surface.md)).
- No shell source contains a comparator, formatter, predicate over records,
  or enablement rule (ADR 0001 R4, checked from the shell side).
- A windowed `items` call with a stale generation returns the staleness
  refusal, and a shell driven through a mid-scroll rebuild re-reads rather
  than rendering a mismatched row.
- Scrolling a 20,000-item collection end to end crosses the boundary a number
  of times proportional to the windows drawn, not to the collection size.
- Every action returns either success or a typed refusal; a test enumerates
  the actions and asserts totality.
- Content state distinguishes "no records yet" from "no records match".
- Starting an already-running operation returns the busy refusal.
- A single test suite covers each app core and is the only place its domain
  rules are asserted.

## Rationale

This is the requirement that makes eight shells affordable. A shell that
merely binds is a shell with nothing worth testing, so a new platform costs
rendering and host integration rather than a re-implementation of the
product.

R5 goes further than is usual, and deliberately. localgallery already moved
subtitle composition and country naming into core tables rather than platform
locale services, accepting a visible behaviour change to do it. The result is
that a memory's subtitle is identical on a phone and a handheld because there
is one implementation, not two that agree today.

R4's two-step shape (r2) is the correction of a real defect in r1, which had
a view model hand back "ordered sections, each with ordered items … carrying
the already-formatted values." UniFFI copies, so that shape marshals an
entire collection on every change — and localgallery has already paid for
learning this: `CoreLibraryIndex` answers in **ids** because the app holds
the structs, and memoises per-tag queries from one publish to the next
because a boundary crossing per frame is what its scroll-cost findings
forbid. R4 now encodes that arrangement instead of contradicting it. The
generation counter is what makes windowed reads safe: without it a shell can
fetch rows 200–220 of a list that was rebuilt between the structure read and
the content read.

R6 (r2) removes the raw material that lets domain policy leak into shells,
but the record-shape checker alone does not make ADR 0001 R4 mechanically
true. The existing GTK shell, written
against a conventions document that forbade exactly this, carries 27
comparator sites and 42 formatting sites today. Forbidding it again would
produce the same result; removing the raw material does not.

Milestone C put contact display records and typed conflict disposition in
`contacts-core` so UniFFI and GTK control flow cannot drift. Contacts now also
has a full typed edit command with deterministic stale detection; the iOS
store adapter uses it. GTK still holds `Card` for detail/export and its
compact form remains to migrate to the full draft. Windowed view models
remain a Phase 5 gallery requirement.
