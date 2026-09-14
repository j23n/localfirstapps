# contacts-gtk

Linux shell for LocalContacts (Phase 3.5). Links `shell-kit-gtk` and
`contacts-core`. No UniFFI. Host filesystem is a path. No Flatpak.

```
cd shells
cargo run -p contacts-gtk
cargo run -p contacts-gtk -- --comet
```

`--comet` is the same binary at 540×620 with bottom navigation.

Per-device state (ADR 0005 R5) lives under
`$XDG_CONFIG_HOME/localcontacts/` (`device-id`, `folder`). The folder
log still syncs at `{folder}/.contacts/log/<dev>/`.
