# contacts-gtk

Linux shell for LocalContacts (Phase 3.5 / Milestone C). Links
`shell-kit-gtk` and `contacts-core` (display rows and logged actions
live in the core). No UniFFI. Host filesystem is a path. This tree has no
Flatpak manifest. Phase 5B native folder controls passed, but no live portal
session or persisted grant was available; Flatpak support is not claimed.

```
cd shells
cargo run -p contacts-gtk
cargo run -p contacts-gtk -- --comet
```

`--comet` is the same binary at 540×620. Chrome follows width:
bottom navigation at or below 550 CSS pixels (ADR 0004 R8).

Per-device state (ADR 0005 R5) lives under
`$XDG_CONFIG_HOME/localcontacts/` (`device-id`, `folder`). The folder
log still syncs at `{folder}/.contacts/log/<dev>/`; it is domain event state,
not local diagnostic capture.
