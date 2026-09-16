# shells

Linux workspace (ADR 0001 R9). GTK lives here so `core/` never sees a
UI toolkit.

- `shell-kit-gtk` — one libadwaita binding per ADR 0004 R4 kind.
  Depends on the slot vocabulary (`localcore-ui`) and on no app core.
- `shell-kit-swift` — provisional SwiftUI bindings used by the Contacts and
  Music Settings screens. Its generated vocabulary comes from R14 and it
  depends on no domain module.
- `contacts-gtk` — LocalContacts laptop / Comet shell. Links the kit
  and `contacts-core` (display rows live in the core). `--comet` is
  540×620; chrome follows width (bottom nav at or below 550).
- `music-gtk` — LocalMusic laptop / Comet shell. Links the kit and
  `music-core`; GStreamer and MPRIS remain host ports. Headless tests inject
  a deterministic transport.

```
cd shells
cargo test --locked --workspace --all-targets
cargo run -p contacts-gtk
cargo run -p contacts-gtk -- --comet
cargo run -p music-gtk --features gstreamer-playback
cargo run -p music-gtk --features gstreamer-playback -- --comet
```

On macOS:

```
swift test --package-path shells/shell-kit-swift
```

On Linux, SwiftUI package compilation is unavailable; run:

```
python3 shells/shell-kit-swift/scripts/check.py
```
