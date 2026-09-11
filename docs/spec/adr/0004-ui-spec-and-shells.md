# ADR 0004: UI specification, slot vocabulary, and shells

- Status: Accepted
- Date: 2026-09-11
- Revised: 2026-09-11 (r2)

## Scope

How screens are declared once and rendered natively twice, how brand survives
that, and what a shell is allowed to be.

## Requirements

### The specification

**R1.** Each app carries a **UI spec**: a declarative, platform-neutral
description of its screens, checked into the app's repository and read by
both shells and by the app core's tests.

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
| `progress-row` | label, determinate fraction or indeterminate, optional cancel |
| `status-row` | message, severity (`info`, `warning`, `error`) |

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

**R6.** A shell MUST provide exactly one native binding per kind in R4. Each
binding is written once per platform and reused by all four apps, and it lives
in that platform's `shell-kit` (ADR 0001 R1), which depends on this
vocabulary and on no app core.

**R7.** A screen declared in a spec with no binding available on a platform
MUST fail the build for that platform. Silent omission is not permitted.

This is achieved by R14's generated enums: an unhandled slot kind is a
non-exhaustive match in Rust and a non-exhaustive switch in Swift, both of
which are compile errors. No runtime interpretation of the spec is involved.

**R8.** Shells render with **native controls and native navigation**. iOS
uses SwiftUI navigation, sheets, and system controls; GTK shells use
libadwaita navigation and controls. A shell MUST NOT imitate another
platform's chrome, and the Mecha Comet is an adaptive layout of the GTK shell
(compact window, bottom navigation), not a distinct toolkit.

**R9.** A shell owns, and is the only layer that owns: rendering; gesture and
input handling; host integration behind app-core ports; window, scene and
lifecycle management; and accessibility.

### Generation

**R14.** Code is generated from the spec for exactly three things, and the
list is closed:

| Generated | Into | Why |
|---|---|---|
| slot-kind, screen-kind, affordance and nav-intent enums | Rust and Swift | makes R7 a compile error |
| screen identifiers | Rust and Swift | a spec screen with no view model fails to build |
| design tokens (R11) | each platform's native colour/metric form | makes R11's "no literal" check trivial |

Nothing else is generated. Layout, widgets, bindings and navigation are
hand-written per platform. A shell MUST NOT read the spec at runtime: the
spec is a build-time input, and a shell that interprets it is the UI framework
this document exists to prevent.

### Brand

**R10.** Brand is carried by what is shared and portable: the name, the icon,
the accent colour, the vocabulary (ADR 0007 R1), the information architecture
in the spec, the copy, and the behavioural promise of no accounts and no
network. It is NOT carried by control styling.

**R11.** **Design tokens are data.** One token set per app — accent and
supporting colours, semantic text and surface roles, spacing steps, corner
radii — defined once and emitted into each platform's native form. Shells
consume the emitted form. A colour literal in a shell source file that is not
a generated token is a defect.

**R12.** Where a platform offers a semantic system colour or material that
fits, shells SHOULD use it in preference to a token. Tokens exist for the
cases where the system has no opinion, chiefly the accent and the app's own
surfaces.

**R13.** Copy is authored once, in the spec or the app core, and is identical
across platforms. Platform-idiomatic differences are confined to control
labels the platform itself owns.

## Conformance

- Every screen in every app resolves to kinds drawn only from R4.
- Each shell binds every kind in R4; a missing binding fails the build.
- Each binding is implemented once per platform and used by all four apps.
- No shell source contains a hard-coded colour outside generated tokens.
- The same user-visible strings appear in both shells for the same screen.
- The Comet build is the GTK shell with an adaptive layout, sharing its
  bindings.
- A screen added to a spec that reuses existing kinds appears on both
  platforms with no new *widget* code; its view model and its screen
  assembly are still written per platform.
- Removing a slot-kind binding from a shell fails that shell's build.
- No shell links a spec parser or reads a spec file at runtime.
- Generated sources are reproducible: regenerating in CI produces no diff.

## Rationale

Two forces pull against each other: one product, and native feel on each
platform. They are reconciled by noticing that they operate at different
levels. What a screen *is* — its content, its actions, its order — is the
product, and is identical everywhere. How that screen is *drawn* is the
platform's business, and imitating a foreign platform is the one thing that
reliably reads as cheap.

The closed vocabulary in R4 is the guard rail. Kept small, the spec is a
cheap description that makes drift a build error. Allowed to grow toward
geometry, it becomes a UI framework, and a UI framework maintained by one
person alongside four apps will consume the project. R2 and R4 exist to make
that failure mode structurally hard rather than merely discouraged.

R14 (r2) resolves a contradiction in r1, which required a missing binding to
fail *the build* while the implementation plan forbade codegen outright.
Neither half was wrong; they simply could not both hold. Three enums and a
token table are about fifty lines of build script and buy a compile error at
exactly the boundary that matters. Everything past that is the framework, and
the list is closed so that "just one more generated thing" is an amendment
rather than an afternoon.
