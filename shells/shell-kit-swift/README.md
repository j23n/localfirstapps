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
imports and `Color(red:)`, and verifies the two current consumers pass their
generated token metric.

## Provisional promotion measurements

Measured for the first Contacts/Music slice on 2026-09-16:

- four app-owned screens consume the package (Contacts/Music Settings and
  list search); no screen body or domain model moved into the kit;
- 22 direct component invocations replace local settings chrome, rows,
  confirmation, and search modifiers;
- `ShellSettings`, `ShellTextRow`, `ShellActionRow`, `ShellNavRow`, and
  `shellSearch` each have both Contacts and Music call sites;
  `ShellStatusRow` and `shellConfirmation` currently have Contacts call sites
  only;
- the Settings wrapper also exercises `ShellList` in both apps;
- field/form/filter are present with data/behavior coverage, but production
  call-site migration is deferred to later narrow slices;
- all 6 screen kinds, 8 item kinds, 8 affordances, 3 navigation intents,
  2 action roles, and 3 status severities have explicit dispositions.

This is evidence for the small Settings/list-row seam only. It is not a claim
of full SwiftUI shell reuse, and it says nothing yet about media, grid,
viewer, progress, selection, or large-collection behavior.
