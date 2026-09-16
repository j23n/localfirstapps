# music-core

Headless localmusic app core. Audio and playlist files in the selected folder
are Tier 1 authority. The in-memory track, playlist, search, sort, and section
tables are disposable projections.

The core:

- walks the folder through `localcore-walk`, classifies the existing Swift
  audio and playlist extension sets, and never assigns conflict copies ids;
- derives NFC path ids through `localcore-id`;
- accepts metadata through a narrow media host-port DTO and owns all display
  projection policy;
- parses M3U/M3U8/PLS while retaining unknown lines, and atomically writes a
  canonical representation through `localcore-vfs`;
- accepts only typed playlist operations, not serialized playlist payloads;
- plans and explicitly applies ADR 0005 R8-R11 M3U conflict resolution.

Playlist gestures append low-rate operation events below
`.music/log/<device>/`. At an intentionally generous 100 edits/day and about
300 bytes/event, expected growth is roughly 11 MB/year, so no compaction is
needed. The device id itself remains a per-device exception. Media playback
position is Tier 2 but is not migrated in this foundation; existing iOS call
sites remain unchanged.
