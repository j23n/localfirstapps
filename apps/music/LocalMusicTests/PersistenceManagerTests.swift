import Foundation
import Testing
@testable import LocalMusic

@MainActor
@Suite(.serialized)
final class PersistenceManagerTests {
    private let tempDir: URL
    private let defaultsName: String
    private let defaults: UserDefaults

    init() throws {
        CacheTestLock.acquire()
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("PMTests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        defaultsName = "com.localmusic.tests.\(UUID().uuidString)"
        defaults = try #require(UserDefaults(suiteName: defaultsName))
        ArtworkCache.directoryOverride = tempDir.appendingPathComponent("Artwork")
        LyricsCache.directoryOverride = tempDir.appendingPathComponent("Lyrics")
    }

    deinit {
        ArtworkCache.directoryOverride = nil
        LyricsCache.directoryOverride = nil
        try? FileManager.default.removeItem(at: tempDir)
        CacheTestLock.release()
    }

    @Test func lastSyncedRoundTrip() {
        let persistence = makePersistence()
        #expect(persistence.loadLastSynced() == nil)
        let date = Date(timeIntervalSince1970: 1_700_000_000)
        persistence.saveLastSynced(date)
        #expect(persistence.loadLastSynced() == date)
    }

    @Test func folderBookmarkRoundTrip() throws {
        let folder = tempDir.appendingPathComponent("Music", isDirectory: true)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let persistence = makePersistence()
        persistence.saveFolderBookmark(folder)
        let resolved = try #require(persistence.loadFolderBookmark())
        #expect(resolved.standardized.path == folder.standardized.path)
    }

    @Test func legacyProjectionMigratesPayloadsThenIsRemoved() async throws {
        let trackURL = URL(fileURLWithPath: "/legacy/song.mp3")
        let artwork = Data(repeating: 0xAB, count: 64)
        let json = """
        [{
          "url": "\(trackURL.absoluteString)",
          "title": "Legacy",
          "artist": "Artist",
          "album": "Album",
          "duration": 42,
          "artworkData": "\(artwork.base64EncodedString())",
          "lyrics": "line"
        }]
        """
        try writeLibraryJSON(json)
        let persistence = makePersistence()

        #expect(await persistence.migrateLegacyLibraryIfNeeded() == 1)
        #expect(!FileManager.default.fileExists(atPath: libraryURL.path))
        #expect(ArtworkCache.hasArtwork(for: trackURL))
        #expect(LyricsCache.hasLyrics(for: trackURL))
        #expect(try Data(contentsOf: ArtworkCache.fileURL(for: trackURL)) == artwork)
    }

    @Test func legacyProjectionIsConsumedOnlyOnce() async throws {
        let persistence = makePersistence()
        try writeLibraryJSON("[]")
        #expect(await persistence.migrateLegacyLibraryIfNeeded() == 0)
        try writeLibraryJSON("""
        [{"url":"file:///late.mp3","title":"Late","artist":"","album":"","duration":0}]
        """)
        #expect(await persistence.migrateLegacyLibraryIfNeeded() == 0)
        #expect(FileManager.default.fileExists(atPath: libraryURL.path))
    }

    @Test func corruptProjectionIsRemovedAndNeverAuthoritative() async throws {
        try writeLibraryJSON("{not valid json")
        let persistence = makePersistence()
        #expect(await persistence.migrateLegacyLibraryIfNeeded() == 0)
        #expect(!FileManager.default.fileExists(atPath: libraryURL.path))
    }

    private var libraryURL: URL {
        tempDir.appendingPathComponent("library.json")
    }

    private func makePersistence() -> PersistenceManager {
        PersistenceManager(documentsURL: tempDir, userDefaults: defaults)
    }

    private func writeLibraryJSON(_ contents: String) throws {
        try contents.write(to: libraryURL, atomically: true, encoding: .utf8)
    }
}
