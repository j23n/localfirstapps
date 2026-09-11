import Foundation

// `UserDefaults` is documented thread-safe but isn't yet annotated Sendable
// in the SDK, so we vouch for it with @unchecked.
struct BookmarkManager: @unchecked Sendable {
    static let bookmarkKey = "LocalContacts_FolderBookmark"
    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    func saveBookmark(for url: URL) throws {
        // Must call startAccessingSecurityScopedResource BEFORE creating bookmark
        _ = url.startAccessingSecurityScopedResource()
        let bookmarkData = try url.bookmarkData(
            options: [],
            includingResourceValuesForKeys: nil,
            relativeTo: nil
        )
        url.stopAccessingSecurityScopedResource()
        defaults.set(bookmarkData, forKey: Self.bookmarkKey)
    }

    func loadBookmark() -> URL? {
        guard let data = defaults.data(forKey: Self.bookmarkKey) else { return nil }
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
        defaults.removeObject(forKey: Self.bookmarkKey)
    }

    var hasBookmark: Bool {
        defaults.data(forKey: Self.bookmarkKey) != nil
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
