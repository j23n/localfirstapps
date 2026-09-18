# ADR 0004: UI specification, slot vocabulary, and shells

- Status: Accepted
- Date: 2026-09-11
- Revised: 2026-09-11 (r2); 2026-09-13 (Phase 3.2–3.3); 2026-09-14 (Phase 3.5); 2026-09-14 (Milestone C); 2026-09-16 (GTK Contacts completion); 2026-09-16 (GTK Music second consumer); 2026-09-16 (chart-row); 2026-09-16 (GTK design pass 2a/2b reuse); 2026-09-16 (Swift kit measured seam); 2026-09-17 (R10/R11 display typeface; GTK 5.7 promotions); 2026-09-17 (GTK/Gallery 5.9 close-out); 2026-09-17 (progress affordance)

## Scope

How screens are declared once and rendered natively twice, how brand survives
that, and what a shell is allowed to be.

## Requirements

### The specification

**R1.** Each app carries a **UI spec**: a declarative, platform-neutral
description of its screens, checked into the app's repository. It is a
**build-time** input (R14). Shells MUST NOT parse it at runtime. App-core
tests MAY read it.

**R2.** The spec is **semantic**. It declares what a screen contains and what
it offers. It MUST NOT declare geometry, spacing, sizing, fonts, arrangement,
or any other visual property. Those are shell decisions, made natively.

**R3.** For each screen the spec declares: an identifier; a screen kind from
R4; its sections and the slot kind of their items; the actions it offers and
where they appear; its search, filter and sort affordances; its selection
behaviour; and the navigation each action or item triggers.

### The vocabulary

**R4.** The slot vocabulary is **closed**. Every shell MUST provide a binding
for every kind. Adding a kind is an amendment to this document.

*Screen kinds*

| Kind | Meaning |
|---|---|
| `list` | ordered rows, optionally sectioned |
| `grid` | media items in a reflowing grid |
| `detail` | one record, read-only fields and actions |
| `form` | one record, editable fields, save/cancel semantics |
| `viewer` | full-bleed media with overlaid chrome |
| `settings` | grouped settings, per ADR 0007 |

*Item kinds*

| Kind | Carries |
|---|---|
| `text-row` | title, optional subtitle, optional trailing value, optional leading symbol |
| `media-item` | thumbnail reference, optional label, optional badge |
| `field-row` | label, value, optional editability |
| `toggle-row` | label, on/off state |
| `action-row` | label, role (`normal`, `destructive`), enabled state |
| `nav-row` | label, optional trailing value, destination |
| `progress-row` | label, optional detail, determinate fraction or indeterminate, optional cancel |
| `status-row` | message, severity (`info`, `warning`, `error`) |
| `chart-row` | title, optional subtitle/unit, preformatted latest value, and a display-unit series the shell sparks natively |

*Screen affordances*

| Kind | Meaning |
|---|---|
| `search` | free-text query over the screen's content |
| `filter` | a named set of predicates, multi-select |
| `sort` | a named set of orderings, single-select |
| `selection` | multi-select mode offering a set of actions |
| `primary-action` | the one prominent action |
| `overflow` | secondary actions behind a menu |
| `banner` | transient screen-level status |
| `progress` | chrome-level ongoing work: phase label, optional detail/fraction, optional cancel |
| `confirm` | a confirmation gate carrying a question and a destructive label |

*Navigation intents*

| Kind | Meaning |
|---|---|
| `push` | a deeper screen in the current context |
| `sheet` | a modal task the user completes or abandons |
| `replace` | swap the current root |

**R5.** A screen MUST be expressible using only these kinds. A screen that is
not is either a design that needs revising or a genuine gap that amends R4;
it is never a one-off widget in one shell.

### Shells

**R6.** Every kind used by a screen MUST have a native binding on that
platform. A binding proven reusable by a second app belongs in that
platform's `shell-kit` (ADR 0001 R1), which depends on this vocabulary and on
no app core. A generated enum arm proves inventory coverage; it does not by
itself prove that a production-quality reusable binding exists.

As of the GTK Gallery 5.7 slice, `shell-kit-gtk` has three product
consumers. Contacts ∩ Music stays 25 shared of 28 Contacts and 29 Music
bindings (`ChromeProgress` and `ProgressRow` are shared). Gallery is the second consumer of
`media_item` (with Music) and `chip_bar` (with Contacts). Flush /
`GtkSectionModel` stay unpromoted.
`shell-kit-swift` is measured for the Settings/list/filter/confirm/progress
seam (Contacts + Music, pinned by `scripts/check.py`). Form/field/status
have Contacts production only. The package as a whole stays provisional
for grid, viewer, media, selection, and sort until a second
consumer exists (Gallery).

**R7.** An R4 **kind** omitted from a vocabulary consumer MUST fail the build
through a non-exhaustive match. Tests and review still verify that the
matched implementation has the data and behavior the kind requires.

A spec **screen** with no view on a platform that hosts it is a **product
gap**, recorded on the implementation plan. It is not a missing kind.
Host-only screens (ADR 0007 R15) have no view on the other platform.
`apple-conflict` on Linux is that case.

**R8.** Shells render with **native controls and native navigation**. iOS
uses SwiftUI navigation, sheets, and system controls; GTK shells use
libadwaita navigation and controls. A shell MUST NOT imitate another
platform's chrome. The Mecha Comet is the **same GTK binary**, not a
distinct toolkit. `--comet` selects the compact default size (540×620).
Chrome MUST follow window width: at or below 550 CSS pixels the shell
uses bottom navigation, whether it was launched with `--comet` or a
laptop window was resized.

**R9.** A shell owns, and is the only layer that owns: rendering; gesture and
input handling; host integration behind app-core ports; window, scene and
lifecycle management; and accessibility.

### Generation

**R14.** Code is generated from the spec for exactly three things, and the
list is closed:

| Generated | Into | Why |
|---|---|---|
| slot-kind, screen-kind, affordance and nav-intent enums | Rust and Swift | makes R7 a compile error |
| screen identifiers | Rust and Swift | a shell can match; unused ids are a gap, not a compile error until view models exist (ADR 0003 R4) |
| design tokens (R11) | each platform's native colour/metric form | makes R11's "no literal" check trivial |

Nothing else is generated. Layout, widgets, bindings and navigation are
hand-written per platform. A shell MUST NOT read the spec at runtime: the
spec is a build-time input, and a shell that interprets it is the UI framework
this document exists to prevent.

### Brand

**R10.** Brand is carried by what is shared and portable: the name, the icon,
the accent colour, the vocabulary (ADR 0007 R1), the information architecture
in the spec, the copy, and the behavioural promise of no accounts and no
network. When an app's token table has `[fonts] display`, brand also includes
that named display typeface. It is NOT carried by control styling.

**R11.** **Design tokens are data.** One token set per app — accent and
supporting colours, semantic text and surface roles, spacing steps, corner
radii — defined once and emitted into each platform's native form. Shells
consume the emitted form. A colour literal in a shell source file that is not
a generated token is a defect. The token table MAY include one optional
display typeface `{ family, style, file }`. Only that app's shell loads the
file. Only the documented class may set `font-family` (Gallery
`.memory-title`).

**R12.** Where a platform offers a semantic system colour or material that
fits, shells SHOULD use it in preference to a token. Tokens exist for the
cases where the system has no opinion, chiefly the accent and the app's own
surfaces.

**R13.** Screen **titles** are authored in the spec. The app core owns
semantic facts, stable message/action keys, formatted domain values and
action availability. User-facing prose belongs in per-app localization
resources; shells MUST branch on typed actions or dispositions, never on
visible copy. Platform-idiomatic differences are confined to control labels
the platform itself owns.

## Conformance

- Every screen in every app resolves to kinds drawn only from R4.
- Each kind a shell uses has a native binding. Generated exhaustive matches
  prove vocabulary coverage, while tests prove required data and behavior.
- GTK's domain-neutral bindings live in `shell-kit-gtk`. `contacts-gtk` and
  `music-gtk` publish distinct binding inventories; their test-pinned
  intersection is 25 of 28 Contacts / 29 Music after `ChromeProgress`. Gallery ∩
  Music includes `MediaItem`; Gallery ∩ Contacts includes `ChipBar`.
- Newsreader and `.memory-title` appear only in `gallery-gtk`.
  `shell-kit-gtk` `data/style.css` has neither.
- No shell source contains a hard-coded colour outside generated tokens
  or a platform semantic colour (R12).
- Shell control flow uses typed actions/dispositions rather than display copy.
- The Comet build is the GTK binary; chrome follows width as well as
  `--comet`.
- A new *kind* appears on GTK with no new widget code in the app shell.
  A new *screen* still needs per-platform assembly.
- No shell links a spec parser or reads a spec file at runtime.
- Generated sources are reproducible: regenerating in CI produces no diff
  (`python3 scripts/gen_r14.py --check`).

### Milestone C (Phase 3.6)

Held both contacts shells against this document after 3.5. The closed
vocabulary **survived**: no new R4 kind was required for the C-loop
(folder, list, search, detail, edit, save, Syncthing group). What did
not survive is the stronger reading of R7/R14 — that every spec screen
fails the build until a view exists.

| Finding | Disposition |
|---|---|
| R4 kinds were enough for contacts | No amendment needed for this slice; gallery/media workloads remain untested. |
| `shell-kit-gtk` exhaustively names kinds; iOS has no kit | Enum coverage is not reuse evidence. Both platforms wait for a second app before the kit boundary is considered stable. |
| `ContactsScreen` was generated and unused by both view trees | R7/R14: kinds fail the build; unbound screens are a gap list. GTK now routes every hosted screen; `apple-conflict` is iOS-only (ADR 0007 R15). |
| GTK 3.5 formatted rows in the shell | Moved list/detail/tag/conflict rows, the full `ContactEditDraft`, and logged typed actions into `contacts-core`. FFI copies the same rows and command DTOs onto UniFFI. |
| `--comet` was a fixed size, not adaptive | R8 now requires chrome to follow width (550 px). |
| iOS uses `Color.accentColor` (asset catalog) | R12. Generated `accentDark` is for GTK CSS and any Swift that does not go through the catalog. |
| GTK edit form was six fields | Closed by binding every field in the core-owned draft. |
| List filter / selection / tags | Implemented in GTK with core-owned rows and typed commands. |
| ADR 0003 R4 windowed view models | Not built for contacts. Lists are small. Remains mandatory for gallery (Phase 5). |

### GTK Contacts completion (2026-09-16)

The GTK shell now routes every Contacts screen it hosts, including
`tag-management` and `logs`; `apple-conflict` remains iOS-only. The contact
list implements tag filtering, multi-selection, bulk tag assignment, and bulk
delete. Its editor binds the full core-owned draft, including repeated labeled
values, structured addresses, yearless birthdays, categories, and JPEG photo
selection. These changes close the GTK product gaps recorded in the Milestone C
table without adding an R4 kind or moving app-specific widgets into
`shell-kit-gtk`.

### GTK Music second-consumer evidence (2026-09-16)

Music routes all nine generated Linux screens and uses `music-core` display
rows and typed commands for folder projection, search/sort, playlist
creation/editing, and M3U conflict decisions. Its headless path test performs
Folder open → list → playlist create/add/reorder save → explicit ordered
conflict choice. GStreamer playback and MPRIS are shell host ports; the
deterministic test adapter moves no playback domain state through UniFFI.

The second consumer exposed three useful boundary facts:

| Finding | Evidence / disposition |
|---|---|
| Local diagnostics were duplicated shell behavior | Moved the bounded, non-persistent logger into the kit and made both products consume it. |
| Sort/filter needed an option-bearing native control | Added a domain-neutral choice dropdown; both products consume it. |
| The provisional media item dropped `thumbnail_ref` | The binding now carries semantic/file references and renders a native action row. Only Music currently consumes it, so cross-product reuse is not claimed. |
| Measured common surface | `measure_reuse(contacts_gtk::KIT_BINDINGS, music_gtk::KIT_BINDINGS)` reports 21 shared of 22 Contacts and 23 Music bindings after the GTK design-pass 2a–2c chrome, builders, and row polish; the assertion fails on inventory drift. |
| Native/runtime confidence | CI compiles and links GTK plus GStreamer and runs headless workflow tests. Audio output, MPRIS media-key interoperability, Flatpak folder portals, and physical Comet layout remain manual and unmeasured. |

### Swift kit measured seam (2026-09-16)

`shell-kit-swift` has two iOS product consumers. `scripts/check.py`
pins the two-app intersection and fails if a claimed shared binding
loses a consumer or a new public kit type appears without an inventory
entry.

| Finding | Evidence / disposition |
|---|---|
| Settings / list / search | Both apps: `ShellSettings`, `ShellList`, `ShellTextRow` / `ShellActionRow` / `ShellNavRow`, `.shellSearch`. |
| Filter / confirm | Both apps: `ShellFilterMenu` on Logs; `.shellConfirmation` on Contacts Settings+detail and Music playlist delete. |
| Form / field | Contacts detail+edit production only. Music has no matching form. Not two-app reuse. |
| Status / chart | `ShellStatusRow` is Contacts Settings only. `ShellChartRow` has no production consumer (Health is Phase 6). |
| Progress | Both apps: `ShellProgressChip` (chrome after 500 ms) and `ShellProgressRow` (Settings, immediate). |
| Unclaimed | grid / viewer / media / selection / sort / primary / overflow / banner stay `appOwned`. |
| Native/runtime confidence | Linux `check.py` is the gate. `swift test` and the iOS apps need `macos-26` / local Xcode. |

This promotes the Settings/list/filter/confirm seam. It does not
promote the package, and it says nothing about media, grids, or
large collections.

### GTK Gallery promotions + font (2026-09-17)

Gallery is the second production consumer of Music's `media_item` and
Contacts' chips. That promotes those two kit bindings. Evidence:

| Binding | Consumers | Notes |
|---|---|---|
| `media_item` | Music (48px default) + Gallery folders (64px, trailing count, `navigates`) | `.thumb` + `thumb_radius` in the kit stylesheet |
| `chip_bar` | Contacts detail (Display) + Gallery photos tags (Removable) | `AdwWrapBox`, pill buttons |
| Flush / `GtkSectionModel` | Music only | Not promoted |
| Newsreader Italic | Gallery `.memory-title` only | ADR 0004 R10/R11 display typeface |

`face-review` stays unbound. Leftover `ui` stays off.

### GTK/Gallery close-out (2026-09-17)

Three GTK apps consume `shell-kit-gtk`. Reuse is **pairwise**: Contacts ∩
Music, Gallery ∩ Music (`media_item`), Gallery ∩ Contacts (`chip_bar`).
That is not three-app reuse for tiles or the mini-player.

The Photos year rail is `gallery-gtk` chrome derived from existing
`photo_structure` month sections. It is not an R4 kind and not a kit
binding. Leftover `apps/gallery/linux` `ui` stays until the owner agrees
to remove it.

## Rationale

Two forces pull against each other: one product, and native feel on each
platform. They are reconciled by noticing that they operate at different
levels. What a screen *is* — its content, its actions, its order — is the
product, and is identical everywhere. How that screen is *drawn* is the
platform's business, and imitating a foreign platform is the one thing that
reliably reads as cheap.

The closed vocabulary in R4 is a review guard rail. Kept small, the spec is a
cheap description that makes omissions in generated consumers a build error.
It does not make incomplete widget behavior a compile error. Allowed to grow toward
geometry, it becomes a UI framework, and a UI framework maintained by one
person alongside four apps will consume the project. R2 and R4 exist to make
that failure mode structurally hard rather than merely discouraged.

R14 (r2) resolves a contradiction in r1, which required a missing binding to
fail *the build* while the implementation plan forbade codegen outright.
Neither half was wrong; they simply could not both hold. Three enums and a
token table keep vocabulary use explicit and reproducible. Widget capability
still needs tests and review. Everything past that is the framework, and the
list is closed so that "just one more generated thing" is an amendment rather
than an afternoon.

Milestone C (Phase 3.6) is the first time this document met a second toolkit,
but only through one app. Contacts needed no new kind. That is useful evidence
for this vertical, not proof that the vocabulary or `shell-kit` abstractions
serve media, grids, progress, or large collections. Music and a thin gallery
probe provide the next evidence.
