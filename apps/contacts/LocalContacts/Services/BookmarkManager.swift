import Foundation

struct BookmarkManager: Sendable {
    private static let bookmarkKey = "LocalContacts_FolderBookmark"

    func saveBookmark(for url: URL) throws {
        // Must call startAccessingSecurityScopedResource BEFORE creating bookmark
        _ = url.startAccessingSecurityScopedResource()
        let bookmarkData = try url.bookmarkData(
            options: [],
            includingResourceValuesForKeys: nil,
            relativeTo: nil
        )
        url.stopAccessingSecurityScopedResource()
        UserDefaults.standard.set(bookmarkData, forKey: Self.bookmarkKey)
    }

    func loadBookmark() -> URL? {
        guard let data = UserDefaults.standard.data(forKey: Self.bookmarkKey) else { return nil }
        var isStale = false
        guard let url = try? URL(
            resolvingBookmarkData: data,
            options: [],
            relativeTo: nil,
            bookmarkDataIsStale: &isStale
        ) else { return nil }

        if isStale {
            // Re-create bookmark with fresh data
            try? saveBookmark(for: url)
        }

        return url
    }

    func clearBookmark() {
        UserDefaults.standard.removeObject(forKey: Self.bookmarkKey)
    }

    var hasBookmark: Bool {
        UserDefaults.standard.data(forKey: Self.bookmarkKey) != nil
    }
}

actor FolderAccessManager {
    private var activeURL: URL?

    func startAccessing(_ url: URL) {
        stopAccessing()
        _ = url.startAccessingSecurityScopedResource()
        activeURL = url
    }

    func stopAccessing() {
        activeURL?.stopAccessingSecurityScopedResource()
        activeURL = nil
    }

    var currentURL: URL? { activeURL }
}
