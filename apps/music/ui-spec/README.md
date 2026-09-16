# music UI spec

Semantic screen inventory for localmusic (ADR 0004). The R14 generator reads
this file and emits identifiers; shells do not assemble their views from it.
It declares no geometry and does not prove that a screen is implemented.

```sh
python3 scripts/gen_r14.py   # emits MusicScreen in Rust + Swift
```

The inventory covers folder selection, library browsing, playlists, playback,
settings, diagnostics, and Syncthing playlist conflict resolution. Playback
controls call a platform media port (AVFoundation on iOS; gstreamer/MPRIS on
Linux). `sync-conflict-group` is the ADR 0005 R8-R11 surface for `.m3u` and
`.m3u8` conflict groups.
