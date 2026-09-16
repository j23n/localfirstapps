# localfiles

Four local-first apps. You pick a folder; the app turns those files into a
view you can browse and edit. Nothing talks to a network at runtime. If you
want the same folder on more than one machine, sync it yourself (Syncthing /
SyncTrain).

| App | Files in the folder | Linux |
|---|---|---|
| **LocalGallery** | JPEG / HEIC / PNG, optional `.xmp` | GTK app |
| **LocalContacts** | `.vcf` | GTK app |
| **LocalMusic** | audio, `.m3u` / `.m3u8` / `.pls` | GTK app |
| **LocalHealth** | NDJSON event log + blobs | `archive` CLI (no GUI yet) |

The same GTK binary is the laptop window or, with `--comet`, a 540×620 Mecha
Comet window. Chrome follows width: bottom navigation at or below 550 CSS
pixels. iOS shells for Gallery, Contacts, and Music live in this repo too;
they need a Mac and Xcode.

## Linux requirements

- Rust **1.97.1** (`rustup install 1.97.1`; use `cargo +1.97.1` below)
- GTK 4.22+ and libadwaita 1.9+
- Platform baseline Fedora 44 / Ubuntu 26.04 (GNOME 50)
- A C compiler (`gcc`)
- Go 1.25+ only for the Health CLI (`CGO_ENABLED=1`)
- GStreamer 1.0 plus base/good plugins for Music **playback**
- OpenSSL headers only if you enable Gallery ML (`--features ml`)

Ubuntu 24.04 ships GTK 4.14 and libadwaita 1.5 and is below this floor.

Fedora:

```bash
sudo dnf install gtk4-devel libadwaita-devel pkgconf-pkg-config gcc \
  gstreamer1-devel gstreamer1-plugins-base-devel gstreamer1-plugins-good \
  openssl-devel golang
```

Debian / Ubuntu 26.04:

```bash
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev \
  libgstreamer1.0-dev gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
  libssl-dev
```

Need a graphical session to *run* the GTK apps. `cargo test` does not.

Debug builds of the GTK shells accept `--route <screen-id> --snapshot out.png --size WxH` and optional `--folder`. `scripts/gtk-snapshots.sh contacts` (or `music`) runs the implemented route matrix under headless mutter when it is installed; without mutter the script skips and exits 0. PNGs land in `docs/screenshots/gtk-before/` only when a capture actually writes a file. `cargo test` / CI do not run mutter.

## Host release binaries

From the repo root (this tree is bind-mounted as `~/dev/public/localfirst`
on the laptop):

```bash
cd shells
cargo +1.97.1 test --locked --workspace --all-targets
cargo +1.97.1 build --release --locked -p contacts-gtk
# Playback needs GStreamer devel on the machine (no extra packages if
# `pkg-config --exists gstreamer-1.0` already prints nothing and exits 0):
pkg-config --exists gstreamer-1.0 && echo gst-ok
cargo +1.97.1 build --release --locked -p music-gtk --features gstreamer-playback
```

Binaries: `shells/target/release/localcontacts` and
`shells/target/release/localmusic`. Copy them next to the ones you already
run if you keep a `build/` folder.

`build/localmusic` copied from this environment is **not** a GStreamer
build (the agent image has no `gstreamer-1.0.pc`). Play on the laptop
needs a host rebuild after devel packages are installed:

```bash
pkg-config --exists gstreamer-1.0 && echo gst-ok
cd ~/dev/public/localfirst/shells
cargo +1.97.1 build --release --locked -p music-gtk --features gstreamer-playback
cp -f target/release/localmusic ../build/localmusic
```

If `pkg-config` fails, install `gstreamer1-devel` and
`gstreamer1-plugins-base-devel` (needs admin). Plugins (`good`, `libav`)
are the runtime codecs. Song titles/artists/durations come from `lofty`
and do **not** need GStreamer.

Fedora (admin once): `gstreamer1-devel gstreamer1-plugins-base-devel`
plus `gstreamer1-plugins-good` and a codec set for typical MP3/M4A.

## LocalContacts

```bash
cd shells
cargo +1.97.1 test --locked --workspace --all-targets
cargo +1.97.1 run --locked -p contacts-gtk
cargo +1.97.1 run --locked -p contacts-gtk -- --comet
```

Binary: `shells/target/debug/localcontacts`.

Open a folder of `.vcf` files. Device id and last folder path stay under
`$XDG_CONFIG_HOME/localcontacts/`. Folder events still append at
`{folder}/.contacts/log/<dev>/`.

## LocalMusic

```bash
cd shells
cargo +1.97.1 run --locked -p music-gtk
# real playback (needs GStreamer devel + plugins):
cargo +1.97.1 run --locked -p music-gtk --features gstreamer-playback
cargo +1.97.1 run --locked -p music-gtk --features gstreamer-playback -- --comet
```

Binary: `shells/target/debug/localmusic`.

Open a folder of local audio. Tags are read with `lofty` after the
folder opens. Without `gstreamer-playback` the UI still builds and
runs; transport is a mock so you can exercise lists and playlists
without GStreamer headers. Host settings are
`$XDG_CONFIG_HOME/localmusic/`. Playlists stay in the chosen folder.

## LocalGallery

```bash
cd apps/gallery/linux
cargo +1.97.1 test --locked --no-default-features --all-targets
cargo +1.97.1 run --locked
cargo +1.97.1 run --locked -- --comet
```

Binary: `apps/gallery/linux/target/debug/localgallery`.

Open a photo folder. **Scan Photos** writes `.xmp` sidecars next to images;
image bytes are not rewritten. Tagging and faces need a model pack **and**
`--features ml` — see [apps/gallery/linux/INSTALL.md](apps/gallery/linux/INSTALL.md).
Without a pack, browse and Places still work.

## LocalHealth

There is no Health GTK or iOS shell yet. What you can run on Linux is the
Go CLI and the Rust projection tests.

```bash
cd apps/health
export CGO_ENABLED=1
export GOFLAGS=-mod=vendor
go test ./...
go build -o archive ./cmd/archive
./archive                  # usage
./archive rebuild          # derived/archive.db from log/ + blobs/
./archive query
./archive kinds
./archive fsck
```

`-root` defaults to `$ARCHIVE_ROOT` or `./archive`. `import` hashes a file
into `blobs/` and appends one `blob_import` event. Apple `export.xml` is
not parsed; sample rows come from `observation` / `episode` events or
NDJSON blobs.

```bash
cd core
cargo +1.97.1 test --locked -p health-core -p health-ffi --all-targets
```

## More

Specification: [docs/spec/README.md](docs/spec/README.md).
Per-app notes: [apps/gallery](apps/gallery), [apps/contacts](apps/contacts),
[apps/music](apps/music), [apps/health](apps/health).
Linux shells: [shells/README.md](shells/README.md).
Mac / iOS: [mac/README.md](mac/README.md).
