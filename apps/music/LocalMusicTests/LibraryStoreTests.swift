import Foundation
import Testing
@testable import LocalMusic

@MainActor
final class LibraryStoreTests {
    private let tempDir: URL
    private let defaults: UserDefaults

    init() throws {
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("LibraryStoreTests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        defaults = try #require(
            UserDefaults(suiteName: "com.localmusic.store-tests.\(UUID().uuidString)")
        )
    }

    deinit {
        try? FileManager.default.removeItem(at: tempDir)
    }

    @Test func sessionWalkBuildsTrackAdapterWithStableIDParity() async throws {
        try touch("nested/Café.mp3", makeDirectories: true)
        let store = makeStore()
        await store._testOpenFolder(tempDir)

        let track = try #require(store.tracks.first)
        #expect(track.title == "Café")
        #expect(track.coreID == Track.stableID(for: track.url).uuidString.lowercased())
        #expect(store.displayTracks.map(\.id) == store.tracks.map(\.id))
        #expect(store.settingsInfoRows.map(\.id) == ["tracks", "playlists", "sync-conflicts"])
        #expect(store.settingsInfoRows.first?.trailing == "1 track")
    }

    @Test func searchSortAndSectionsComeFromCoreProjection() async throws {
        try touch("zebra.mp3")
        try touch("Alpha.mp3")
        try touch("apple.mp3")
        let store = makeStore()
        await store._testOpenFolder(tempDir)

        #expect(store.displayTracks.map(\.title) == ["Alpha", "apple", "zebra"])
        #expect(store.sections.map(\.title) == ["A", "Z"])

        store.searchText = "APP"
        await store._testWaitForApply()
        #expect(store.displayTracks.map(\.title) == ["apple"])
        #expect(store.contentState == .content)
        #expect(store.searchHits.contains { $0.kind == .track && $0.title == "apple" })

        store.searchText = "missing"
        await store._testWaitForApply()
        #expect(store.displayTracks.isEmpty)
        #expect(store.contentState == .noMatches)
        #expect(store.searchHits.isEmpty)
    }

    @Test func typedPlaylistCreateAddRemoveAndDelete() async throws {
        try touch("one.mp3")
        let store = makeStore()
        await store._testOpenFolder(tempDir)
        let track = try #require(store.tracks.first)

        let created = await store.createPlaylist(name: "Mix")
        let playlist = try #require(created)
        #expect(playlist.name == "Mix")
        #expect(FileManager.default.fileExists(
            atPath: tempDir.appendingPathComponent("Mix.m3u").path
        ))

        await store.addTrack(track, to: playlist.id)
        let added = try #require(store.playlist(id: playlist.id))
        #expect(added.entries.count == 1)
        #expect(added.entries.first?.trackID == track.coreID)
        #expect(added.entries.first?.sourceURL?.standardized.path == track.url.standardized.path)

        await store.removePlaylistEntries(
            playlistID: playlist.id,
            offsets: IndexSet(integer: 0)
        )
        #expect(store.playlist(id: playlist.id)?.entries.isEmpty == true)

        let index = try #require(store.playlists.firstIndex { $0.id == playlist.id })
        await store.deletePlaylists(at: IndexSet(integer: index))
        #expect(store.playlist(id: playlist.id) == nil)
        #expect(!FileManager.default.fileExists(
            atPath: tempDir.appendingPathComponent("Mix.m3u").path
        ))
    }

    @Test func typedPlaylistMovePreservesRequestedOrder() async throws {
        for name in ["a.mp3", "b.mp3", "c.mp3"] {
            try touch(name)
        }
        let store = makeStore()
        await store._testOpenFolder(tempDir)
        let created = await store.createPlaylist(name: "Order")
        let playlist = try #require(created)
        for track in store.tracks {
            await store.addTrack(track, to: playlist.id)
        }
        let before = try #require(store.playlist(id: playlist.id))
        #expect(before.entries.map(\.title) == ["a", "b", "c"])

        await store.movePlaylistEntry(
            playlistID: playlist.id,
            from: IndexSet(integer: 0),
            to: 3
        )
        #expect(store.playlist(id: playlist.id)?.entries.map(\.title) == ["b", "c", "a"])
    }

    @Test func playlistDetailKeepsMissingAndUnsupportedRows() async throws {
        try touch("known.mp3")
        try """
        #EXTM3U
        known.mp3
        absent.mp3
        https://example.com/live
        """.write(
            to: tempDir.appendingPathComponent("Foreign.m3u"),
            atomically: true,
            encoding: .utf8
        )
        let store = makeStore()
        await store._testOpenFolder(tempDir)
        let playlist = try #require(store.playlists.first)

        #expect(playlist.entries.count == 3)
        #expect(playlist.entries[0].trackID != nil)
        #expect(playlist.entries[1].trailing == "File not found")
        #expect(playlist.entries[2].subtitle == "Unsupported playlist entry")
    }

    @Test func stalePlaylistEditIsRefusedAndRefreshed() async throws {
        try touch("song.mp3")
        let store = makeStore()
        await store._testOpenFolder(tempDir)
        let track = try #require(store.tracks.first)
        let created = await store.createPlaylist(name: "Stale")
        let playlist = try #require(created)
        try "#EXTM3U\nexternal.mp3\n".write(
            to: tempDir.appendingPathComponent("Stale.m3u"),
            atomically: true,
            encoding: .utf8
        )

        await store.addTrack(track, to: playlist.id)
        #expect(store.errorMessage?.contains("changed on disk") == true)
        #expect(store.playlist(id: playlist.id)?.entries.first?.title == "external")
    }

    @Test func syncConflictRowsAndResolutionUseTypedSession() async throws {
        try "#EXTM3U\nbase.mp3\nleft.mp3\n".write(
            to: tempDir.appendingPathComponent("mix.m3u"),
            atomically: true,
            encoding: .utf8
        )
        try "#EXTM3U\nbase.mp3\nleft.mp3\nright.mp3\n".write(
            to: tempDir.appendingPathComponent(
                "mix.sync-conflict-20200901-120000-PHONE01.m3u"
            ),
            atomically: true,
            encoding: .utf8
        )
        let store = makeStore()
        await store._testOpenFolder(tempDir)
        let group = try #require(store.syncConflictGroups.first)
        #expect(group.disposition == .auto)

        try await store.resolveConflict(groupID: group.id, selectedSource: nil)
        #expect(store.syncConflictGroups.isEmpty)
        let content = try String(
            contentsOf: tempDir.appendingPathComponent("mix.m3u"),
            encoding: .utf8
        )
        #expect(content.contains("right.mp3"))
    }

    @Test func emptyFolderClearsWhileInaccessibleFolderKeepsProjection() async throws {
        try touch("cached.mp3")
        let store = makeStore()
        await store._testOpenFolder(tempDir)
        #expect(store.tracks.count == 1)

        try FileManager.default.removeItem(at: tempDir.appendingPathComponent("cached.mp3"))
        await store.rescan()
        #expect(store.tracks.isEmpty)
        #expect(store.contentState == .emptyFolder)

        try touch("again.mp3")
        await store.rescan()
        #expect(store.tracks.count == 1)
        let missing = tempDir.appendingPathComponent("missing", isDirectory: true)
        await store._testOpenFolder(missing)
        #expect(store.tracks.count == 1)
        #expect(store.errorMessage != nil)
    }

    private func makeStore() -> LibraryStore {
        let persistence = PersistenceManager(
            documentsURL: tempDir.appendingPathComponent("documents"),
            userDefaults: defaults
        )
        let snapshotCache = LibrarySnapshotCache(
            cachesDirectory: tempDir.appendingPathComponent("caches/localmusic", isDirectory: true)
        )
        return LibraryStore(
            persistence: persistence,
            snapshotCache: snapshotCache,
            defaults: defaults
        )
    }

    private func touch(_ relativePath: String, makeDirectories: Bool = false) throws {
        let url = tempDir.appendingPathComponent(relativePath)
        if makeDirectories {
            try FileManager.default.createDirectory(
                at: url.deletingLastPathComponent(),
                withIntermediateDirectories: true
            )
        }
        FileManager.default.createFile(atPath: url.path, contents: Data())
    }
}
