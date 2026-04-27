import Foundation
import Testing
@testable import LocalContacts

@Suite("BookmarkManager")
struct BookmarkManagerTests {

    private func makeDefaults() -> UserDefaults {
        // Per-test suite — never touches UserDefaults.standard.
        let suiteName = "LocalContactsTests-Bookmark-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defaults.removePersistentDomain(forName: suiteName)
        return defaults
    }

    private func makeTempFolder() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("LocalContactsTests-Bookmark-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    @Test("saveBookmark then loadBookmark returns an equivalent URL")
    func saveAndLoad() throws {
        let defaults = makeDefaults()
        let folder = try makeTempFolder()
        defer { try? FileManager.default.removeItem(at: folder) }

        let manager = BookmarkManager(defaults: defaults)
        try manager.saveBookmark(for: folder)
        let resolved = try #require(manager.loadBookmark())

        // Bookmarks may resolve to a different URL representation (e.g.
        // `/private/var/...` instead of `/var/...`), so compare by
        // standardized file path rather than raw URL equality.
        #expect(resolved.standardizedFileURL.path == folder.standardizedFileURL.path)
    }

    @Test("hasBookmark reflects current persistence state")
    func hasBookmark() throws {
        let defaults = makeDefaults()
        let folder = try makeTempFolder()
        defer { try? FileManager.default.removeItem(at: folder) }

        let manager = BookmarkManager(defaults: defaults)
        #expect(!manager.hasBookmark)

        try manager.saveBookmark(for: folder)
        #expect(manager.hasBookmark)

        manager.clearBookmark()
        #expect(!manager.hasBookmark)
        #expect(manager.loadBookmark() == nil)
    }

    @Test("loadBookmark returns nil when stored data is corrupt")
    func loadCorrupt() {
        let defaults = makeDefaults()
        // Write garbage under the bookmark key — must not crash; must return nil.
        // Reference the manager's key constant so a rename doesn't silently
        // turn this into a "no key set → nil" test.
        defaults.set(Data([0x00, 0x01, 0x02, 0x03]), forKey: BookmarkManager.bookmarkKey)
        let manager = BookmarkManager(defaults: defaults)
        #expect(manager.loadBookmark() == nil)
    }
}
