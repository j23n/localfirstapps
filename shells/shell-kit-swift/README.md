# shell-kit-swift

Thin native SwiftUI bindings for the ADR 0004 vocabulary. The package owns
display slots and widget composition only: it imports no app core, local core,
Contacts, or Music module, and callbacks return opaque action identifiers to
the app shell.

`Generated/Kinds.swift` is emitted from
`docs/spec/ui/vocabulary.toml` by `scripts/gen_r14.py`. Apps pass authored
metrics through `ShellTokens`; SwiftUI continues to use each app's generated
asset-catalog accent. The kit contains no colour literal or fallback brand
colour.

## Validation

On macOS with Xcode selected:

```sh
cd shells/shell-kit-swift
swift test
```

On Linux, where SwiftUI is unavailable:

```sh
python3 shells/shell-kit-swift/scripts/check.py --self-test
python3 shells/shell-kit-swift/scripts/check.py
python3 scripts/gen_r14.py --check
```

The static check keeps the generated vocabulary copy synchronized, requires
an exhaustive disposition for every generated kind, rejects non-SwiftUI
imports and `Color(red:)`, verifies the two current consumers pass their
generated token metric, and fails if a claimed two-app binding loses a
consumer or a new public kit type appears without an inventory entry.

## Measured seam (2026-09-16)

Two-app production intersection, pinned by `scripts/check.py`:

- `ShellSettings`, `ShellList`, `ShellTextRow`, `ShellActionRow`,
  `ShellNavRow`, `ShellFilterMenu`, `.shellSearch`, `.shellConfirmation`
- Contacts and Music Settings, both list searches, both Logs screens,
  Contacts delete confirm, Music playlist delete

Contacts-only production: `ShellForm` / `ShellFieldRow` (detail + edit),
`ShellStatusRow` (Settings). `ShellChartRow` has no production consumer.

No screen body or domain model moved into the kit. Grid, viewer, media,
progress, selection, sort, primary, overflow, and banner stay app-owned.
This promotes the Settings/list/filter/confirm seam only.
