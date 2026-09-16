# Tests

Unit and integration tests for LocalMusic. Run via:

```sh
xcodegen
xcodebuild test \
  -scheme LocalMusic \
  -destination 'platform=iOS Simulator,name=iPhone 16'
```

CI on this monorepo is the `music` job in `.github/workflows/apps.yml`.
The nested `.github/workflows/test.yml` is retained only for a standalone
checkout; the root `apps.yml` job is canonical here.

Tests use [Swift Testing](https://developer.apple.com/xcode/swift-testing/)
(`@Test`, `#expect`, `#require`). XCTest assertions and `XCTestCase`
subclasses are not used.

## Layout

| File | Coverage |
|---|---|
| `Fixtures.swift` | Shared `Track` builders for in-memory tests |
| `PlaybackQueueTests.swift` | Queue/shuffle/repeat state machine; `Action` dispatch |
| `MetadataLoaderTests.swift` | AVFoundation host enrichment, core/Swift stable-ID parity, and safe SYLT parsing |
| `TrackTests.swift` | `Track.stableID` determinism + RFC 4122 bits, `RepeatMode` raw values, `TrackLyrics` Codable |
| `LibraryStoreTests.swift` | `MusicSession` folder projection, core search/sort/sections, typed playlist CRUD, stale-token recovery, and Syncthing conflict resolution |
| `HelpersTests.swift` | `Collection[safe:]`, `SyncedLyricsView.activeIndex` binary search |
| `PersistenceManagerTests.swift` | bookmarks/last-sync and one-time legacy `library.json` payload migration/removal |
| `ArtworkCacheTests.swift` | key/path determinism, store/remove, ImageIO downsampling |
| `LyricsCacheTests.swift` | round-trip, empty-deletes-file, async remove, URL standardization |

## Refactor seams

These keep the production code testable. Don't remove without a replacement.

- `PlaybackQueue` (`LocalMusic/Services/PlaybackQueue.swift`) — pure value-type state machine. `AudioPlayerManager` delegates to it and applies a returned `Action`.
- `PersistenceManager.init(documentsURL:userDefaults:)` — tests isolate bookmark state and the one-time compatibility migration.
- `ArtworkCache.directoryOverride` / `LyricsCache.directoryOverride` — `#if DEBUG` only, declared `nonisolated(unsafe)`. Set in `init` / cleared in `deinit`. Suites that touch them carry `@Suite(.serialized)` for in-suite ordering, plus `CacheTestLock.acquire()` / `release()` (in `Fixtures.swift`) for cross-suite mutual exclusion against the other cache-touching suites.
- `LibraryStore._testOpenFolder` / `_testWaitForApply` — `#if DEBUG` only. Drive the real generated session against temporary folders.
- `SyncedLyricsView.activeIndex(in:at:)` — static helper so tests don't need a `View`.

## Follow-ups

Open work, ordered by value:

1. **Folder-scan integration tests** with real audio fixtures. Need a small bundle of MP3/M4A files with embedded ID3v2 + iTunes metadata + USLT/SYLT lyrics, exercised against `MetadataLoader.scanFolder(at:)`. Synthesize via `AVAssetWriter` at test time, or check in.
2. **UI smoke tests** (XCUITest). `UIDocumentPickerViewController` can't be driven from XCUITest, so requires a debug-only `--demo-library` launch arg pointing at a fixture folder bundled with the UI test runner. Cover: onboarding → folder selection → tap row → mini-player → Now Playing tab → transport controls.
3. **Deterministic `_testFlushIO`** on `ArtworkCache` / `LyricsCache`. `remove` currently polls disk for up to 1 s. A `ioQueue.sync {}` helper would remove the flake risk on loaded CI.
4. **DEBUG-tunable debounce** on `LibraryStore.scheduleApply`. The 250 ms sleep makes search-pipeline tests slow; an injectable interval keeps the suite fast.
5. **Cross-suite cache isolation.** `directoryOverride` is shared global state; if cache-touching suites are ever allowed to run in parallel with each other, instance-level injection (or an `xctestplan` that disables parallelization) is needed.
