import Foundation

/// `@unchecked Sendable` because every stored property is immutable after init
/// and all mutation runs through the serial `ioQueue` (or `UserDefaults`,
/// which is itself thread-safe).
final class PersistenceManager: @unchecked Sendable {

    static let shared = PersistenceManager()

    private let ioQueue = DispatchQueue(label: "com.localmusic.persistence",
                                        qos: .userInitiated)
    private let documentsURL: URL
    private let defaults: UserDefaults

    /// Default initializer points at the real `Documents/` and
    /// `UserDefaults.standard`. Tests inject a temp directory and a private
    /// suite to keep state isolated.
    init(documentsURL: URL? = nil, userDefaults: UserDefaults = .standard) {
        if let documentsURL {
            self.documentsURL = documentsURL
        } else {
            self.documentsURL = FileManager.default
                .urls(for: .documentDirectory, in: .userDomainMask)[0]
        }
        self.defaults = userDefaults
    }

    private var libraryURL: URL {
        documentsURL.appendingPathComponent("library.json")
    }

    // MARK: - Folder Bookmark

    func saveFolderBookmark(_ url: URL) {
        do {
            let bookmarkData = try url.bookmarkData(
                options: [],
                includingResourceValuesForKeys: nil,
                relativeTo: nil
            )
            defaults.set(bookmarkData, forKey: "folderBookmark")
        } catch {
            Log.persistence.error("Failed to save folder bookmark: \(error.localizedDescription)")
        }
    }

    func loadFolderBookmark() -> URL? {
        guard let data = defaults.data(forKey: "folderBookmark") else { return nil }
        var isStale = false
        do {
            let url = try URL(
                resolvingBookmarkData: data,
                options: [],
                relativeTo: nil,
                bookmarkDataIsStale: &isStale
            )
            if isStale {
                saveFolderBookmark(url)
            }
            return url
        } catch {
            Log.persistence.error("Failed to resolve folder bookmark: \(error.localizedDescription)")
            return nil
        }
    }

    // MARK: - Last Synced

    func saveLastSynced(_ date: Date) {
        defaults.set(date, forKey: "lastSynced")
    }

    func loadLastSynced() -> Date? {
        defaults.object(forKey: "lastSynced") as? Date
    }

    // MARK: - One-time legacy compatibility

    /// `library.json` is no longer read as a library projection. On the first
    /// session-backed launch only, embedded artwork/lyrics are salvaged into
    /// the host caches and the obsolete projection is removed before core
    /// performs an authoritative folder rescan.
    func migrateLegacyLibraryIfNeeded() async -> Int {
        let url = libraryURL
        return await withCheckedContinuation { continuation in
            ioQueue.async {
                let key = "musicSessionLegacyLibraryMigrated"
                guard !self.defaults.bool(forKey: key) else {
                    continuation.resume(returning: 0)
                    return
                }

                var migrated = 0
                if let data = try? Data(contentsOf: url),
                   let entries = try? JSONDecoder().decode([LibraryEntry].self, from: data) {
                    for entry in entries {
                        Self.migratePayloads(entry)
                    }
                    migrated = entries.count
                }
                if FileManager.default.fileExists(atPath: url.path) {
                    do {
                        try FileManager.default.removeItem(at: url)
                    } catch {
                        Log.persistence.error(
                            "Failed to remove obsolete library.json: \(error.localizedDescription)"
                        )
                    }
                }
                self.defaults.set(true, forKey: key)
                continuation.resume(returning: migrated)
            }
        }
    }

    private static func migratePayloads(_ entry: LibraryEntry) {
        if let bytes = entry.artworkData, !bytes.isEmpty {
            ArtworkCache.storeSync(bytes, for: entry.url)
        }
        if entry.lyrics != nil || entry.syncedLyrics != nil {
            let lyrics = TrackLyrics(unsynced: entry.lyrics, synced: entry.syncedLyrics)
            if !lyrics.isEmpty {
                LyricsCache.storeSync(lyrics, for: entry.url)
            }
        }
    }

    /// Wire shape that accepts both legacy and slim track JSON.
    private struct LibraryEntry: Decodable {
        let id: UUID?
        let url: URL
        let title: String
        let artist: String
        let album: String
        let duration: Double
        let hasArtwork: Bool?
        let hasLyrics: Bool?
        let artworkData: Data?
        let lyrics: String?
        let syncedLyrics: [SyncedLyricLine]?
    }

}
