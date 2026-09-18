# shells

Linux workspace (ADR 0001 R9). GTK lives here so `core/` never sees a
UI toolkit.

- `shell-kit-gtk` — one libadwaita binding per ADR 0004 R4 kind.
  Depends on the slot vocabulary (`localcore-ui`) and on no app core.
- `shell-kit-swift` — SwiftUI bindings used by Contacts and Music
  (Settings, list search, Logs, confirm). Settings/list/filter/confirm
  are a measured two-app seam; form/field/status are Contacts-only.
  Generated vocabulary comes from R14; the package depends on no domain
  module. Grid/viewer/media stay app-owned.
- `contacts-gtk` — LocalContacts laptop / Comet shell. Links the kit
  and `contacts-core` (display rows live in the core). `--comet` is
  540×620; chrome follows width (bottom nav at or below 550).
- `music-gtk` — LocalMusic laptop / Comet shell. Links the kit and
  `music-core`; GStreamer and MPRIS remain host ports. Headless tests inject
  a deterministic transport.
- `gallery-gtk` — LocalGallery kit shell (Phase 5.9 year rail). Binary
  `localgallery`, id `com.j23n.LocalGallery`. Path-depends on
  `gallery-ffi` + leftover `localgallery`, both `default-features =
  false`, with gallery-gtk default `ml` on. Workspace pins:
  `image = "=0.25.10"`, `uniffi` 0.32. Default `ml` downloads `ort`.
  Settings is a primary-menu dialog; leftover `ui` stays
  off. Folders, collections, viewer, photo-info, and the memories
  rail are routed. Second consumer of kit `media_item` (folder 64px
  covers) and `chip_bar` (photos tags). Newsreader Italic loads for
  `.memory-title` only. Flush is not promoted. Face-review stays unbound.

```
cd shells
cargo test --locked --workspace --all-targets
cargo tree -d
cargo run -p contacts-gtk
cargo run -p contacts-gtk -- --comet
cargo run -p music-gtk --features gstreamer-playback
cargo run -p music-gtk --features gstreamer-playback -- --comet
cargo run -p gallery-gtk
cargo run -p gallery-gtk -- --comet
cargo run -p gallery-gtk -- --bench --folder /path/to/photos
```

`scripts/gtk-perf.sh smoke|20k` times the Gallery catalog path under
headless mutter when it is installed; without mutter the GTK half
skips and exits 0. 20k also runs ignored `e2e_catalog` (Session +
`ViewList`, no display). Core 20k stays `apps/gallery/scripts/e2e_20k.sh`.

Debug traces (`core/localcore-trace`): `[lf main]` is the thread that
called `init` (UI jank if a line is slow); `[lf work]` is every other
thread.

```
LOCALFILES_DEBUG=1 cargo run -p gallery-gtk
LOCALFILES_DEBUG=2 cargo run -p music-gtk
RUST_LOG=lf=debug cargo run -p contacts-gtk
```

On macOS:

```
swift test --package-path shells/shell-kit-swift
```

On Linux, SwiftUI package compilation is unavailable; run:

```
python3 shells/shell-kit-swift/scripts/check.py
```
