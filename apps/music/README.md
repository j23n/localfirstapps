# LocalMusic

Lives at `apps/music` in the localfiles monorepo. Commands below are
from that directory.

A music player for locally stored audio files on iOS and Linux. Point it at a folder
whose files already have local bytes and it becomes your library — no
streaming service required. The folder may be provider-backed, but the app
does not request downloads or materialise placeholders.

## Features

- **Folder-based library** — pick any folder via the system document picker; tracks are scanned recursively
- **Rich metadata** — reads title, artist, album, artwork, and duration from file tags
- **Lyrics** — displays embedded unsynced and synced (SYLT) lyrics with auto-scrolling
- **Playlists** — discovers `.m3u` / `.m3u8` / `.pls` files and lets you create and edit your own
- **Now Playing** — full-screen artwork with ambient background color, seek bar, shuffle, and repeat modes
- **Lock screen & Control Center** — playback controls and now-playing info via `MPNowPlayingInfoCenter`
- **Search** — filter your library by title, artist, or album
- **Supported formats** — MP3, M4A, AAC, WAV, AIFF, FLAC, CAF, Opus

## Requirements

- Xcode 15+
- iOS 17.0+
- [XcodeGen](https://github.com/yonaskolb/XcodeGen)
- Linux: GTK 4.14+, libadwaita 1.5+, GStreamer 1.0, and Rust 1.97

## Build

```bash
# Install XcodeGen if you don't have it
brew install xcodegen

# Generate the Xcode project
xcodegen

# Open in Xcode
open LocalMusic.xcodeproj
```

Then build and run on a simulator or device (iOS 17+).

Linux laptop / Mecha Comet:

```bash
cd ../../shells
cargo run -p music-gtk --features gstreamer-playback
cargo run -p music-gtk --features gstreamer-playback -- --comet
```

## Architecture

The app is a single-target SwiftUI project with a tab-based layout (Library, Now Playing, Playlists). Key components:

| File | Role |
|---|---|
| `LocalMusicApp` | App entry point; sets up the tab view and injects the shared player |
| `AudioPlayerManager` | `ObservableObject` wrapping `AVPlayer`; owns playback state, queue, shuffle/repeat logic, and lock-screen integration |
| `MusicCoreClient` | Serial adapter over generated `MusicSession` list/window/detail APIs and typed playlist/conflict commands |
| `MetadataLoader` | AVFoundation host port for tag, artwork, duration, and lyrics enrichment requested by core |
| `PersistenceManager` | Persists the security-scoped folder bookmark and performs one-time `library.json` payload migration |
| `LibraryView` | Displays all tracks with search, pull-to-refresh, and context menu for adding to playlists |
| `NowPlayingView` | Full-screen player with artwork, synced lyrics overlay, seek bar, and transport controls |
| `PlaylistsView` | Lists discovered and user-created playlists; supports creation and deletion |
| `DocumentPicker` | `UIViewControllerRepresentable` wrapper around `UIDocumentPickerViewController` for folder selection |

Folder bytes are authoritative through `MusicSession`; the Swift `Track` and
`Playlist` values are thin UI/playback projections. AVFoundation playback,
`MPNowPlayingInfoCenter`, artwork/lyrics caches, bookmarks, and the document
picker remain iOS host ports. The obsolete `library.json` projection is
consumed once only to salvage legacy artwork/lyrics, then removed and rescanned.

The Linux shell lives in `shells/music-gtk`, links `music-core` directly, and
keeps GStreamer playback plus MPRIS D-Bus behind shell host ports. Its
playlist UI consumes typed commands and display rows rather than playlist
serialization.

## AI disclaimer

Please see [docs/AI_DISCLAIMER.md].

## License

[MPL 2.0](LICENSE)
