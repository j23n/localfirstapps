# ADR 0007: Product conventions

- Status: Accepted
- Date: 2026-09-11
- Revised: 2026-09-11 (r2); 2026-09-11 (native Linux delivery); 2026-09-11 (0.2: CONVENTIONS.md deleted); 2026-09-16 (domain logs separated from local diagnostics)

## Scope

The cross-app conventions that remain conventions: words, screen shapes, host
surfaces, testing, logging, distribution. Anything here that carries
behaviour is specified elsewhere and referenced.

This document is the **sole** home for cross-app convention. The former
`CONVENTIONS.md` is retired: every section of it either moved into an ADR
that owns the behaviour, or into this document, or was dropped because it
described one toolkit's idioms (see Rationale).

## Requirements

### Vocabulary

**R1.** These terms are used exactly, in code, in copy, and in
documentation. This table is the only one; a second vocabulary list anywhere
in the repository is a defect.

| Term | Means | Not |
|---|---|---|
| **Folder** | the user-selected directory the app reads | library, path, source, location |
| **Reload** | re-scan the folder and refresh state | refresh, sync |
| **Sync** | two-way reconciliation with a system store (e.g. system contacts) | any plain reload |
| **Sync Conflict** | two devices edited one file; a conflict copy exists (ADR 0005 R7) | duplicate, collision |
| **Conflict** | the local file and an external system disagree | discrepancy |
| **Projection** | rebuildable derived state | database, cache-as-authority |
| **Shell** | the per-platform UI and host integration layer | frontend, client |
| **Capability** | an expensive derived-data producer (ADR 0006) | feature, engine |
| **Settings** | the settings screen | preferences, options |

### Screens

**R2.** Every app has a Settings screen with this order: the **Folder**
section first; domain sections in the middle; an **Info** section last
carrying item counts and the app version. Counts in Info are the single
statement of how large the folder is.

**R3.** Every app presents an empty state that distinguishes *no folder
chosen* from *folder chosen and empty* from *nothing matches the current
filter*. A cold start MUST NOT show "nothing found" over a folder that has
not finished loading (ADR 0003 R4).

**R4.** Destructive actions are confirmed, name what will be lost, and are
reachable only from where the affected thing is shown.

**R5.** Every long operation shows determinate progress where a total is
knowable, and is cancellable.

### Identity

**R6.** Each app has its own name, icon and accent colour, and shares
everything else in ADR 0004 R10. Bundle and application identifiers follow
one reverse-DNS pattern per app across platforms:

| App | Bundle ID | Prefix |
|---|---|---|
| localgallery | `com.localgallery.app` | `com.localgallery` |
| localcontacts | `com.localcontacts.app` | `com.localcontacts` |
| localmusic | `com.localmusic.app` | `com.localmusic` |
| localhealth | `com.localhealth.app` | `com.localhealth` |

Extension and test targets append a sub-id (`com.localgallery.app.widgets`).
Linux desktop identifiers use the same prefix.

### Host surfaces

**R15.** ADR 0001 R6 requires the operations exposed to both shells to be
identical. This table is the closed list of exceptions — surfaces the host
owns, where a platform difference is expected rather than a defect:

| Surface | iOS | Linux / Comet |
|---|---|---|
| Home-screen widgets | WidgetKit | none |
| Media-key and now-playing transport | MPRemoteCommandCenter | MPRIS over D-Bus |
| System address-book sync | Contacts framework | none — files only |
| Share | share sheet | file save |
| Folder grant | security-scoped bookmark | a host path; Flatpak portal behaviour is unmeasured |
| Background scheduling | BGTaskScheduler | none — foreground only |
| Crash and diagnostics capture | local opt-in file, explicit user share | local opt-in file, explicit user share |
| Bulk ingestion of a system health store | HealthKit (ADR 0008) | none |

Anything not on this table is subject to ADR 0001 R6 without exception.
Adding a row is an amendment to this document, and each row MUST name the
app-core port (ADR 0001 R5) through which the surface is reached.

A surface being absent on a platform MUST degrade a feature, never disable
the app: an app with no widget host simply has no widgets.

### Testing

**R7.** Domain rules are asserted once, in the app core's test suite
(ADR 0003 R10). A rule asserted in a shell test is a rule in the wrong layer.

**R8.** Cross-implementation agreement is pinned by **shared fixtures**: a
committed input and expected output, read by every implementation that claims
to produce it. Identity vectors, scan outcomes, merge results, and capability
outputs are all fixture-pinned (ADR 0002, ADR 0005, ADR 0006).

**R9.** Fixtures are anonymised and committed. Real personal exports, real
photo libraries, and real contact files are never committed. Where a
prerequisite real input is missing, the work stops and asks rather than
synthesising a plausible one.

**R10.** Each shell carries one end-to-end path test: choose a folder, list,
open an item, perform the app's primary action.

**R16.** **Requirements that CI cannot check are named, and reviewed by a
person.** ADR 0001 R4, ADR 0002 R13's build-time exception scope, ADR 0003
R5, ADR 0004 R8, ADR 0004 R9 and R7 above are not mechanically checkable
beyond their structural guards. Each carries a review question in the
pull-request template, answered by the author in prose. A conformance harness
reduces review load; it does not remove it, and a plan that assumes otherwise
is mis-costed.

### Logging and diagnostics

**R11.** **Synced domain logs and local diagnostics are different state.**
Tier-2 event logs record domain operations needed for projection and
reconciliation (ADR 0005 R4–R18). They live in the synced folder, contain
only schema-defined domain events, and are not diagnostic capture.

Diagnostics use one local logging interface across layers. Capture is
opt-in, per-device, and stored outside the synced folder; disabling or
clearing it cannot alter domain state. Nothing writes diagnostics to stdout
outside tests. Identifying strings — paths, names, contact details — pass
through redaction at the call site.

**R17.** Diagnostics never leave the device by any automatic path. There is
no MetricKit integration, crash-reporting framework, telemetry, usage
analytics, background upload, or automatic attachment on any platform. A
diagnostic or crash file exists only after local opt-in and leaves only when
the user explicitly shares it (ADR 0006 R9).

### Distribution

**R12.** Each app builds for iOS, Linux desktop, and the Mecha Comet from one
source tree, and each build is reproducible from a clean checkout with a
documented command per platform. The command is in the app's README and is
exercised by CI.

**R13.** Third-party components are pinned by version and verified by digest
where the source permits it. This includes model packs (ADR 0006 R12) and any
bundled dataset.

**R14.** Licences of all distributed components are enumerated in the
repository, and every one permits redistribution (ADR 0006 R13).

**R18.** Each app's README follows one shape: *Why · Features · Requirements ·
Build · Setup · License*. Build carries R12's per-platform commands verbatim.

### Language

**R19.** **The family ships in one language, and formatting does not follow
the device locale.** Dates, numbers, item counts and country names come from
tables in an app core (ADR 0003 R5), not from a platform locale service.

This is a consequence of R5 rather than a separate choice, and it is already
live: localgallery moved subtitle composition and country naming into core
`en_US` tables, accepting a visible behaviour change, because a memory's
title is part of its identity in the conformance fixtures. Localisation would
mean either bundling ICU in every core or moving composition back into the
shells, which unpins those fixtures and re-opens ADR 0001 R4. Neither is
worth it for a single-user family, and an implementer should not reach for a
locale API expecting it to be welcome.

## Conformance

- A search for the disallowed words in R1 across code and copy returns
  nothing outside this table, and no second vocabulary table exists.
- Every app's Settings screen matches R2's order.
- Each of the three empty states is reachable and distinct in every app.
- Every difference between the two shells' exposed operations resolves to a
  row in R15, and every row names its port.
- No domain rule is asserted in a shell test suite.
- Every fixture-pinned output is read by more than one implementation.
- No committed fixture contains real personal data.
- The pull-request template carries a question for each requirement named in
  R16.
- Synced event logs contain schema-defined domain events only; diagnostics
  never appear under the selected folder.
- Diagnostic capture is local and opt-in, and clearing it leaves replayed
  domain state unchanged.
- No source links MetricKit, a crash-reporting or analytics framework, or an
  automatic diagnostic-upload path.
- Each app has a documented build command per platform that works from a
  clean checkout, and each README follows R18's shape.
- No source in an app core or a shell calls a platform locale, date-format,
  or number-format service.
- The licence inventory covers every distributed component.

## Rationale

These are the conventions that survived as conventions because they carry no
behaviour a compiler could check. Everything that does — diagnostic logging,
domain event replay, identity, scanning, formatting — moved into `localcore`
or an app core, because the family has already demonstrated what shape-only
rules do over time: three apps, three drifted implementations of the same
logger, each defensible on its own.

R8 is the mechanism that keeps two shells and four apps honest without a
person checking. A fixture read by exactly one implementation tests that
implementation; a fixture read by several tests that they agree, which is the
property actually at risk.

**Retiring `CONVENTIONS.md` (r2, completed Phase 0.2).** It was 818 lines of
SwiftUI guidance, and six of its sections contradicted these ADRs outright.
A live contradiction in the document that most shapes unsupervised agent
output is expensive in a way a stale comment is not. Every section is
dispositioned here; the file is deleted.

| § | Title | Disposition |
|---|---|---|
| 1 | Per-app status snapshot | **Dropped.** Inventory, stale on arrival. |
| 2 | Project layout | **Dropped.** `Models/` `Services/` `Views/` is one toolkit. Layout is ADR 0001 R1/R9. |
| 3 | Build settings | **Dropped.** iOS/Xcode idioms. Per-platform commands are R12. |
| 4 | State management | **Dropped.** A SwiftUI `@Observable` Store in the shell contradicts ADR 0001 R4 and ADR 0003. |
| 5 | Folder access | **Moved.** Bookmark as a per-device exception: ADR 0005 R5. Folder grant: R15. The UIKit picker dance is one toolkit and is dropped. |
| 6 | Logging | **Moved.** Local diagnostics use one interface and stay separate from synced domain events: R11. Per-app `os.Logger` namespaces contradict R11. |
| 7 | Settings sheet | **Moved.** Section order: R2. SwiftUI chrome (`.inline`, `.confirmationAction`) dropped. |
| 8 | App shell & navigation | **Dropped.** `TabView` / `NavigationStack` are one toolkit. Navigation intents are ADR 0004 R4. |
| 9 | Stable IDs | **Moved.** ADR 0002 R4, which adds NFC. Hashing path bytes as they arrive contradicted R4. |
| 10 | Design tokens | **Moved.** ADR 0004 R11/R12. |
| 11 | UIKit appearance | **Dropped.** One toolkit. |
| 12 | File I/O | **Moved.** Atomic write is ADR 0002 R3 (temp file plus rename). `Data.write(.atomic)` was a different mechanism and contradicted R3. |
| 13 | Vocabulary | **Dropped.** A second vocabulary table, which R1 itself flags as a defect. |
| 14 | Testing | **Moved.** R7–R10 and R16. The Swift Testing / XCTest prescription is one toolkit and is dropped. |
| 15 | Continuous integration | **Dropped.** Two workflows per app repo contradicts the monorepo `gate` job (plan 0.1). "Exercised by CI" remains R12. |
| 16 | README template | **Moved.** R18. |
| 17 | Bundle identifiers | **Moved.** R6. |
| 18 | Canonical location | **Dropped.** This document is the location. |

The six contradictions: §13 vs R1; §4 vs ADR 0001 R4 / ADR 0003; §9 vs ADR 0002 R4 (NFC); §12 vs ADR 0002 R3; §6 vs R11; §15 vs the monorepo gate.

R15 (r2) closes a hole that made ADR 0001 R6 unusable in both directions.
Read strictly it made iOS-only widgets a specification defect; read loosely
it bounded nothing, and "platform integration" would have become the label
for whatever was inconvenient. A closed, amendable table is the same
mechanism the slot vocabulary uses, for the same reason.

R16 (r2) says out loud what the conformance harness cannot do. Roughly two
thirds of the requirements in this specification are mechanically checkable,
and they are overwhelmingly *absence* rules — no socket crate, no colour
literal, no file-provider API. The ones an implementer under time pressure
actually breaks are the positive ones about where a decision lives, and the
existing GTK shell — written against a conventions document that forbade
exactly this — carries 27 comparator sites and 42 formatting sites today.
ADR 0003 R6 removes the raw material for most of that; what remains is
review, and review has a cost that belongs in the plan.
