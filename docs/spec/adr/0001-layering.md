# ADR 0001: Layered architecture and boundaries

- Status: Accepted
- Date: 2026-09-11
- Revised: 2026-09-11 (r2); 2026-09-13 (`shells/shell-kit-gtk` exists)

## Scope

The layers every app is built from, and what may cross each boundary.

## Requirements

**R1.** Every app is composed of exactly six layers:

```
shell/        per platform, per app. Binds the UI spec to native widgets; owns
              host integration. SwiftUI (iOS) and GTK4/libadwaita (Linux, Comet).
shell-kit/    per platform, shared by all apps. One native binding per slot kind
              (ADR 0004 R4), written once and reused four times. Depends on the
              slot vocabulary and nothing else — no app core, no domain type.
              User input leaves it as an opaque action identifier through a
              callback its host shell installs; it never calls an app core.
ui-spec/      per app. Screens, slots, actions, navigation, design tokens.
              Data, not code (ADR 0004).
app-core/     per app. Parsing, mutation, enrichment, domain policy, and the
              view models the shells bind to (ADR 0003).
localcore/    shared by all apps. Locate, walk, index, project, reconcile,
              queue, atomic write, stable ids (ADR 0002).
platform/     per platform, defined by app-core as narrow ports. Security
              scopes, media playback, system contacts, share sheets.
```

**R2.** Dependencies point downward only. `localcore` MUST NOT depend on any
app core. An app core MUST NOT depend on another app core. A shell MUST NOT
depend on `localcore` directly; everything it needs is re-exposed by its app
core. `shell-kit` MUST NOT depend on any app core or on `localcore`: it is
reachable from a shell and reaches nothing but the slot vocabulary.

**R3.** `localcore` and every app core are written in Rust and MUST compile
for every target platform with the same source. Platform-conditional
compilation inside these layers is permitted only for the `platform/` port
implementations.

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
crates directly. The exposed surface MUST be identical in both, **except for
the host surfaces enumerated in ADR 0007 R15**. A capability available on one
platform and not the other, and not on that list, is a specification defect,
not a platform difference.

**R7.** Two UI toolkits exist: SwiftUI and GTK4/libadwaita. The Mecha Comet
runs the GTK shell with an adaptive layout (ADR 0004 R8), not a third
toolkit. Introducing a third toolkit is an amendment to this document.

**R8.** No app depends on another application being installed. A desktop
companion (digikam, khard, kew) MAY read and write the same folder, and
formats MUST remain compatible with such tools, but no feature requires one.

**R9.** The Rust source is **two Cargo workspaces**:

- `core/` — `localcore` and the four app cores. No member may depend on a UI
  toolkit, directly or transitively, which is what makes ADR 0002 R13's
  dependency check a single command over one lockfile.
- `shells/` — the Linux shells and the GTK `shell-kit`.
  `shell-kit-gtk` landed in Phase 3.3 (vocabulary only; no app core).
  The contacts GTK app is Phase 3.5.

Feature unification is per workspace, so a GTK feature flag cannot reach an
app core and `cargo test` in `core/` runs on a machine with no GUI libraries
installed.

## Conformance

- The dependency graph is acyclic and downward. A build that inverts any
  edge in R2 fails.
- `cargo build` for each app core succeeds for iOS and Linux targets from
  unmodified sources.
- `shell-kit`'s manifest names no app core and no domain crate.
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

`shell-kit` (r2) exists because ADR 0004 R6 requires each slot binding to be
written once per platform and reused by all four apps, and the original
five-layer model had nowhere to put such a thing: every layer was either
per-app or below the platform boundary. Without it, R6 was unimplementable
without violating R2.

R9 (r2) records a split that already existed by accident — gallery's `linux/`
has always carried its own lockfile — and makes it deliberate, because the
alternative unifies `ort` and `gtk4-sys` features across the same graph.
