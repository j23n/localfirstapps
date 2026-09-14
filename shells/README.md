# shells

Linux workspace (ADR 0001 R9). GTK lives here so `core/` never sees a
UI toolkit.

- `shell-kit-gtk` — one libadwaita binding per ADR 0004 R4 kind.
  Depends on the slot vocabulary (`localcore-ui`) and on no app core.
- `contacts-gtk` — LocalContacts laptop / Comet shell. Links the kit
  and `contacts-core`. `--comet` is 540×620 with bottom navigation.

```
cd shells
cargo test --locked --workspace --all-targets
cargo run -p contacts-gtk
cargo run -p contacts-gtk -- --comet
```
