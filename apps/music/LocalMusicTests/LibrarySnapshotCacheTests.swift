import Foundation
import Testing
@testable import LocalMusic

struct LibrarySnapshotCacheTests {
    @Test func cachePathIsUnderCachesAndDocumentsLibraryJSONIsNotAuthority() throws {
        let temp = FileManager.default.temporaryDirectory
            .appendingPathComponent("LibrarySnapshotCacheTests-\(UUID().uuidString)", isDirectory: true)
        let caches = temp.appendingPathComponent("Caches", isDirectory: true)
        let documents = temp.appendingPathComponent("Documents", isDirectory: true)
        try FileManager.default.createDirectory(at: caches, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: documents, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: temp) }

        let obsolete = LibrarySnapshotCache.obsoleteDocumentsLibraryURL(documentsURL: documents)
        try #"[{"title":"not-authority"}]"#.write(to: obsolete, atomically: true, encoding: .utf8)

        let cache = LibrarySnapshotCache(
            cachesDirectory: caches.appendingPathComponent("localmusic", isDirectory: true)
        )
        let url = cache.fileURL(root: "/Music/Library")

        #expect(url.path.hasPrefix(cache.directoryURL.path))
        #expect(url.path.contains("/Caches/"))
        #expect(!url.path.contains("/Documents/"))
        #expect(url.lastPathComponent.hasPrefix("library-"))
        #expect(url.lastPathComponent.hasSuffix(".json"))
        #expect(url.lastPathComponent != "library.json")
        #expect(url.path != obsolete.path)
        #expect(FileManager.default.fileExists(atPath: obsolete.path))
        #expect(!FileManager.default.fileExists(atPath: url.path))

        let other = cache.fileURL(root: "/Music/Other")
        #expect(url.lastPathComponent != other.lastPathComponent)
    }
}
