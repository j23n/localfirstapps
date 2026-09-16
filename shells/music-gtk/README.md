# music-gtk

Native GTK4/libadwaita LocalMusic shell for Linux desktop and Mecha Comet.
It links `music-core` directly; playlist text and Music domain records do not
cross a shell serialization boundary.

## Requirements

GTK 4.14+, libadwaita 1.5+, and Rust 1.97 are required. Production playback
also requires GStreamer 1.0 plus base/good plugins.

## Build

```bash
cd shells
cargo test --locked --workspace --all-targets
cargo run -p music-gtk --features gstreamer-playback
cargo run -p music-gtk --features gstreamer-playback -- --comet
```

The same binary serves laptop and Comet. `--comet` selects a 540×620 initial
size; navigation chrome follows the live width and moves to the bottom at or
below 550 CSS pixels.

## Setup

Choose a Folder with local audio files. Host settings are under
`$XDG_CONFIG_HOME/localmusic/`; synced playlist operation logs remain under
the chosen Folder. MPRIS is exported on the current D-Bus session.

The default feature set deliberately uses an unavailable production adapter,
so headless builders need no GStreamer development package. Tests inject the
deterministic mock. CI compiles and links the real adapter with
`--all-features`, but audio output, media-key integration, Flatpak portal
behavior, embedded metadata/artwork, and physical Comet layout still require
manual validation on suitable hardware.
