# shells

Linux workspace (ADR 0001 R9). GTK lives here so `core/` never sees a
UI toolkit.

- `shell-kit-gtk` — one libadwaita binding per ADR 0004 R4 kind.
  Depends on the slot vocabulary (`localcore-ui`) and on no app core.
- `contacts-gtk` — LocalContacts laptop / Comet shell. Links the kit
  and `contacts-core` (display rows live in the core). `--comet` is
  540×620; chrome follows width (bottom nav at or below 550).

```
cd shells
cargo test --locked --workspace --all-targets
cargo run -p contacts-gtk
cargo run -p contacts-gtk -- --comet
```
