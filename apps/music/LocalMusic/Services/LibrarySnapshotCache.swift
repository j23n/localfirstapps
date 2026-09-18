import Foundation

/// Private, disposable warm-start file. Lives under the app Caches directory,
/// never Documents, and never the selected music folder. Documents
/// `library.json` is not read as authority.
struct LibrarySnapshotCache: Sendable {
    static let shared = LibrarySnapshotCache()

    private let cachesDirectory: URL

    init(cachesDirectory: URL? = nil) {
        if let cachesDirectory {
            self.cachesDirectory = cachesDirectory
        } else {
            let root = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            self.cachesDirectory = root.appendingPathComponent("localmusic", isDirectory: true)
        }
    }

    var directoryURL: URL { cachesDirectory }

    func fileURL(root: String) -> URL {
        let path = libraryCachePath(cacheDir: cachesDirectory.path, root: root)
        return URL(fileURLWithPath: path)
    }

    /// Leftover Documents projection. PersistenceManager deletes it; this
    /// cache never treats it as a library.
    static func obsoleteDocumentsLibraryURL(documentsURL: URL) -> URL {
        documentsURL.appendingPathComponent("library.json")
    }
}
