# ADR 0001: Layered architecture and boundaries

- Status: Accepted
- Date: 2026-09-11
- Revised: 2026-09-11 (r2); 2026-09-13 (`shell-kit-gtk`); 2026-09-14 (`contacts-gtk`); 2026-09-14 (Milestone C); 2026-09-16 (responsibilities, not packages); 2026-09-16 (Gallery crate consumers)

## Scope

The responsibilities every app must assign, and what may cross each ownership
boundary. These names describe roles in the architecture, not directories,
crates, targets, or a mandatory package count.

## Requirements

**R1.** The architecture separates six responsibilities:

- **Shell** — per platform and app. Binds the UI spec to native widgets and
  owns host integration. SwiftUI on iOS; GTK4/libadwaita on Linux and Comet.
- **Shared shell bindings** — an optional per-platform extraction, made only
  for bindings proven reusable by at least two apps. It depends on the slot
  vocabulary and no app core or domain type. User input leaves it as an
  opaque action identifier through a callback installed by the shell.
- **UI specification** — per app. Screens, slots, actions, navigation, and
  design tokens. Data, not code (ADR 0004).
- **App core** — per app. Parsing, mutation, enrichment, domain policy, and
  the view models shells bind to (ADR 0003).
- **Local core** — shared by all apps. Locate, walk, index, project,
  reconcile, queue, atomic write, and stable ids (ADR 0002).
- **Platform integration** — per platform, reached through narrow ports
  declared by an app core. Security scopes, media playback, system contacts,
  and share sheets.

An implementation MAY co-locate several responsibilities in one package or
split one responsibility across packages, provided ownership and dependency
rules remain visible. A shell may hold a reusable-looking binding until a
second app proves it reusable; extracting a `shell-kit` is an implementation
decision, not evidence that reuse exists.

**R2.** Dependencies between the R1 responsibilities point downward only,
whether the code is co-located or packaged separately. Code owning the local
core responsibility MUST NOT depend on an app core. An app core MUST NOT
depend on another app core. A shell MUST NOT path-depend on a `localcore-*`
crate; everything it needs is re-exposed by its app core. An extracted
`shell-kit` MUST NOT depend on any app core or on `localcore`: it is reachable
from a shell and reaches nothing but the slot vocabulary.

**R3.** Code owning the local-core and app-core responsibilities is written
in Rust and MUST compile for every target platform with the same source.
Platform-conditional compilation in that code is permitted only for
implementations of platform-integration ports.

**R4.** A shell MUST contain no domain logic. Specifically, a shell MUST NOT
decide ordering, filtering, grouping, string formatting, action availability,
validation, or error classification. Those are `app-core` answers (ADR 0003
R5).

R4 is enforced structurally where the type system can carry it (ADR 0003 R4:
only ids, strings, booleans and pre-ordered id lists cross to a shell) and by
human review where it cannot. It is not fully checkable by CI, and the
project budgets review time for it rather than pretending otherwise
(ADR 0007 R16).

**R5.** Host integration is reached through **ports**: traits declared by the
app core and implemented by the shell. A port MUST be narrow enough to state
in one sentence. Ports carry data, never platform objects.

**R6.** iOS shells reach their app core through UniFFI. GTK shells link the
app-core crate directly. The **operations** MUST be identical in both
(list, search, save, delete, resolve a Syncthing group), **except for
the host surfaces enumerated in ADR 0007 R15**. Display records are
produced in the app core in both cases. A capability available on one
platform and not the other, and not on that list, is a specification
defect, not a platform difference.

**R7.** Two UI toolkits exist: SwiftUI and GTK4/libadwaita. The Mecha Comet
runs the GTK shell with an adaptive layout (ADR 0004 R8), not a third
toolkit. Introducing a third toolkit is an amendment to this document.

**R8.** No app depends on another application being installed. A desktop
companion (digikam, khard, kew) MAY read and write the same folder, and
formats MUST remain compatible with such tools, but no feature requires one.

**R9.** New shared Rust code is rooted in **two primary Cargo workspaces**:

- `core/` — `localcore` and the four app cores. No member may depend on a UI
  toolkit, directly or transitively, which is what makes ADR 0002 R13's
  dependency check a single command over one lockfile.
- `shells/` — the Linux shells and the GTK `shell-kit`.
  `shell-kit-gtk` landed in Phase 3.3 (vocabulary only; no app core).
  `contacts-gtk` landed in Phase 3.5 (kit + `contacts-core`).

Gallery's existing core and Linux workspaces remain separate until its
vertical migrates. The repository therefore currently tracks four lockfiles;
the two-workspace shape is a destination, not a present-tense invariant.

R9 is a repository packaging rule independent of R1. It does not require one
crate or directory for each responsibility.

Feature unification is per workspace, so a GTK feature flag cannot reach an
app core and `cargo test` in `core/` runs on a machine with no GUI libraries
installed.

## Conformance

- The dependency graph is acyclic and downward. A build that inverts any
  edge in R2 fails.
- No conformance check infers the architecture from a package count; review
  assigns each piece of code to an R1 responsibility and checks its edges.
- `cargo build` for each app core succeeds for iOS and Linux targets from
  unmodified sources.
- A `shell-kit` manifest, when one exists, names no app core and no domain
  crate.
- No shell source file contains a sort comparator, a date or number
  formatter, a predicate over domain records, or an enablement rule.
- Every trait a shell implements for its app core fits R5's one-sentence
  test, and passes no platform type across the boundary.
- The set of operations exposed to SwiftUI and to GTK is identical modulo
  ADR 0007 R15, and every difference resolves to a row on that table.
- `core/` builds and tests with no GUI development package installed.

## Rationale

The layering is chosen so that the expensive things are written once. Domain
logic and correctness live below the platform boundary, where one
implementation serves every shell and one test suite covers it. Shells hold
only what is genuinely per-platform, which is rendering and host integration.

R4 is the load-bearing requirement. A shell that computes anything is a shell
that must be re-verified per platform, and with two toolkits and four apps
that is eight places for the same bug.

Shared shell bindings are a destination for demonstrated reuse, not a
mandatory package created from an inventory. `shell-kit-gtk` is provisional after contacts:
music is the first opportunity to show whether its APIs remove real app-shell
code without reducing native behavior to placeholder widgets.

Phase 5B also measured Gallery's path-dependency consumers before revisiting
its crate count. `gallery-model` and `gallery-vfs` have seven and six
production consumers; `gallery-meta`, `gallery-ml`, `gallery-index`,
`gallery-scan`, and `gallery-session` each serve multiple independently built
consumers. `gallery-memories` has one direct production consumer, but that
count alone demonstrates neither duplicated policy nor a harmful edge.
`gallery-ffi` is an external adapter, so zero Cargo path consumers is expected
and is not evidence that it belongs inside another crate. No cycle, feature
leak, duplicated ownership, or consumer simplification was demonstrated.
Consequently no Gallery crates are merged. The machine-readable inventory is
`docs/spec/evidence/gallery-phase5b-2026-09-16.json`; crate count remains
non-normative.

R9 (r2) records a split that already existed by accident — gallery's `linux/`
has always carried its own lockfile — and makes it deliberate, because the
alternative unifies `ort` and `gtk4-sys` features across the same graph.

Milestone C is why R2 forbids a shell path-dep on `localcore-*` and why
R6 says display records are produced in the app core: `contacts-gtk`
3.5 formatted rows itself and reached `localcore-vfs` directly. The
operations were the same; the answers were not.
