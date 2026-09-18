# gallery-gtk

LocalGallery kit shell (Phase 5.9 year rail). Binary `localgallery`,
application id `com.j23n.LocalGallery`. View models come from
`gallery-ffi` (`default-features = false`, `ml` on by default). Host
config, XDG thumbs, folder watch, and scan/ops come from leftover
`localgallery` with leftover GTK (`ui`) off. Default `ml` compiles
ONNX Runtime; Scan Photos tags and finds faces when a pack is
installed (Settings → Download ML models). The Folders tab
is an in-place explorer: sidebar tree, path breadcrumbs, and a full-width
cover grid. Drill-in does not push pages or bump FFI folder windows.
Photos tag filters are removable `chip_bar`
chips from search hits. Memories rail titles use Newsreader Italic
(`.memory-title`).
The Photos tab overlays a trailing year rail on `photos_scroll` when
`years_from_structure` finds more than one year (month sections on
`photo_structure`, not leftover grouping). Drill-in folder / memory /
person / album grids have no rail.

```
cd shells
cargo run -p gallery-gtk
cargo run -p gallery-gtk -- --comet
cargo run -p gallery-gtk -- --folder /path/to/photos
cargo run -p gallery-gtk -- --route photos
cargo run -p gallery-gtk -- --bench --folder /path/to/photos
```

## Debug traces (`LOCALFILES_DEBUG`)

Shared crate `localcore-trace`. `[lf main]` is the GTK thread (jank);
`[lf work]` is scan / memories / decode. Same env as Music and Contacts.

```
LOCALFILES_DEBUG=1 cargo run -p gallery-gtk -- --folder /path/to/photos
LOCALFILES_DEBUG=2 cargo run -p gallery-gtk -- --folder /path/to/photos
RUST_LOG=lf=debug cargo run -p gallery-gtk -- --folder /path/to/photos
```

`1` is summaries and spans ≥5 ms. `2` is every bind / XDG hit.
`LOCALGALLERY_DEBUG` is still accepted as an alias. Startup prints lanes
(foreground apply_catalog + refill, background scan / snapshot hydrate
/ decode, idle Places/EXIF queues). A matching leftover JSON snapshot
paints People before the walk; only stale rows are enriched. Watch for
`snapshot hit`, `EMPTY picture`, and `collections empty hub`.
JPEG/PNG display decode applies EXIF Orientation; viewer opens show a
grid thumb first. Movies play inline (`GtkVideo` / GStreamer) after a
tap on the poster; leaving the page pauses the stream.
Viewer overflow also offers Share (save files), Move, and Delete for the
current item.

Choosing a folder switches to the library chrome immediately (progress
row, empty grid). `index.build` / host map run on the scan worker.
`--bench --folder` times that worker plus GTK install/`refill_all`, then
quits. Snapshot persist (same v20 JSON iOS uses) is a worker and is
skipped in `--bench`.
`scripts/gtk-perf.sh smoke` runs that under headless mutter on eight tiny
JPEGs. `scripts/gtk-perf.sh 20k` reuses the generated e2e tree: ignored
`e2e_catalog` (Session + `ViewList`, no display) then mutter `--bench`.
Without mutter the GTK half skips and exits 0. CI / `cargo test` do not
run mutter. Core 20k stays `apps/gallery/scripts/e2e_20k.sh`.

`--comet` is 540×620. Chrome follows width (bottom switcher at or below
550). Settings is the primary-menu dialog (Folder, Scan, Diagnostics,
Info last), not a tab. Root switcher: Folders · Collections · Photos.
Primary menu **Select** turns on grid checks. While selecting, a top bar
offers Cancel, Select All, and Deselect All (Escape also cancels). The
bottom bar has Share (file save), Move (library subfolder), and Delete.
Right-click or long-press a tile to Open, Share, Move, Delete, or
Select that photo.

`--route` accepts every `screens.toml` id except `face-review` (unbound).
`sync-conflict-group` is the Syncthing sheet. Grids use `GtkGridView` /
`GtkListView` over a `gio::ListModel` that pages ≤256. Memories are a
shell cache of `MemoryGenerator` (not a `collection_structure` section).

The leftover reference UI is still `localgallery-reference` in
`apps/gallery/linux`. Removal is owner-only; leftover `ui` stays off.
