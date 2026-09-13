# shells

Linux workspace (ADR 0001 R9). GTK lives here so `core/` never sees a
UI toolkit.

- `shell-kit-gtk` — one libadwaita binding per ADR 0004 R4 kind.
  Depends on the slot vocabulary (`localcore-ui`) and on no app core.

The contacts GTK app is Phase 3.5. It will link this kit and
`contacts-core`.

```
cd shells
cargo test --locked --workspace --all-targets
```
