import Foundation
import Observation
import SwiftUI
import ShellKitSwift

enum LibrarySortOption: String, CaseIterable, Identifiable, Sendable {
    case title, artist, album, duration

    var id: String { rawValue }

    var label: String {
        switch self {
        case .title: "Title"
        case .artist: "Artist"
        case .album: "Album"
        case .duration: "Duration"
        }
    }

    var icon: String {
        switch self {
        case .title: "textformat"
        case .artist: "person"
        case .album: "square.stack"
        case .duration: "clock"
        }
    }

    var coreValue: SortOption {
        switch self {
        case .title: .title
        case .artist: .artist
        case .album: .album
        case .duration: .duration
        }
    }
}

@Observable
@MainActor
final class LibraryStore {
    private(set) var tracks: [Track] = []
    private(set) var displayTracks: [Track] = []
    private(set) var sections: [LibrarySection] = []
    private(set) var playlists: [Playlist] = []
    private(set) var syncConflictGroups: [ConflictRow] = []
    private(set) var scanIssues: [StatusRow] = []
    private(set) var contentState: LibraryContentState?
    private(set) var searchHits: [SearchHit] = []
    private(set) var settingsInfoRows: [TextRow] = []
    private(set) var folderURL: URL?
    private(set) var lastSynced: Date?
    private(set) var isScanning = false
    private(set) var isFiltering = false
    private(set) var scanProgress: ScanProgress?
    private(set) var progressRevealed = false
    var errorMessage: String?

    var searchText = "" {
        didSet { scheduleApply() }
    }

    var sortOption: LibrarySortOption = .title {
        didSet {
            UserDefaults.standard.set(sortOption.rawValue, forKey: Self.sortDefaultsKey)
            scheduleApply(immediate: true)
        }
    }

    @ObservationIgnored private var tracksByURL: [URL: Track] = [:]
    @ObservationIgnored private var tracksByCoreID: [String: Track] = [:]
    @ObservationIgnored private var applyTask: Task<Void, Never>?
    @ObservationIgnored private var scanTask: Task<Void, Never>?
    @ObservationIgnored private var scanAccessURL: URL?
    @ObservationIgnored private var applyRevision = 0
    @ObservationIgnored private var scanRevision = 0
    @ObservationIgnored private let core: MusicCoreClient
    @ObservationIgnored private let persistence: PersistenceManager
    @ObservationIgnored private let snapshotCache: LibrarySnapshotCache

    let deviceID: String

    private static let sortDefaultsKey = "librarySort"
    private static let deviceDefaultsKey = "LocalMusic_DeviceId"
    private static let metadataConcurrency = 8

    init(
        core: MusicCoreClient = MusicCoreClient(),
        persistence: PersistenceManager = .shared,
        snapshotCache: LibrarySnapshotCache = .shared,
        defaults: UserDefaults = .standard
    ) {
        self.core = core
        self.persistence = persistence
        self.snapshotCache = snapshotCache
        if let raw = defaults.string(forKey: Self.sortDefaultsKey),
           let option = LibrarySortOption(rawValue: raw) {
            sortOption = option
        }
        if let stored = defaults.string(forKey: Self.deviceDefaultsKey), !stored.isEmpty {
            deviceID = stored
        } else {
            let created = "ios-\(UUID().uuidString.lowercased())"
            defaults.set(created, forKey: Self.deviceDefaultsKey)
            deviceID = created
        }
        lastSynced = persistence.loadLastSynced()
        folderURL = persistence.loadFolderBookmark()
    }

    // MARK: - Lookup

    func track(forURL url: URL) -> Track? {
        tracksByURL[url.standardized]
    }

    func track(coreID: String) -> Track? {
        tracksByCoreID[coreID]
    }

    func playlist(id: String) -> Playlist? {
        playlists.first { $0.id == id }
    }

    func resolved(from urls: [URL]) -> [Track] {
        urls.compactMap { tracksByURL[$0.standardized] }
    }

    /// Search remains core policy. The caller should restore the main library
    /// view when a secondary search surface (such as Add Tracks) closes.
    func searchTracks(query: String) async -> [Track] {
        do {
            let ids = try await core.searchTrackIDs(query: query, sort: sortOption.coreValue)
            return ids.compactMap { tracksByCoreID[$0] }
        } catch {
            report(error)
            return []
        }
    }

    func restoreLibraryView() {
        scheduleApply(immediate: true)
    }

    // MARK: - Bootstrap and rescan

    func bootstrap() async {
        let migrated = await persistence.migrateLegacyLibraryIfNeeded()
        if migrated > 0 {
            Log.persistence.info(
                "Migrated \(migrated) legacy library entries; MusicSession will rescan"
            )
        }
        guard let folderURL else { return }
        startScanAccess(folderURL)
        await rescan()
    }

    func rescanIfNeeded() async {
        await rescan()
    }

    func checkForExternalChanges() async {
        await rescan()
    }

    func rescan() async {
        guard let folderURL else {
            Log.library.warning("Rescan requested but no folder selected")
            return
        }
        await cancelInFlightScan()
        startScanAccess(folderURL)
        isScanning = true
        scanProgress = nil
        progressRevealed = false
        scanRevision += 1
        let revision = scanRevision
        Task { await self.revealProgressIfNeeded(revision) }
        let capturedURL = folderURL
        let task = Task<Void, Never> { [weak self] in
            guard let self else { return }
            await self.performScan(capturedURL: capturedURL, revision: revision)
        }
        scanTask = task
        await task.value
        if revision == scanRevision {
            isScanning = false
            scanProgress = nil
            progressRevealed = false
            scanTask = nil
        }
    }

    private func revealProgressIfNeeded(_ revision: Int) async {
        try? await Task.sleep(for: .milliseconds(500))
        if isScanning, scanRevision == revision {
            progressRevealed = true
        }
    }

    var chromeProgress: ShellProgressData? {
        guard isScanning, progressRevealed else { return nil }
        if let progress = scanProgress, progress.total > 0 {
            return ShellProgressData(
                label: "Scanning",
                detail: "\(progress.completed) / \(progress.total)",
                fraction: Double(progress.completed) / Double(max(progress.total, 1)),
                cancel: false
            )
        }
        return ShellProgressData(label: "Scanning", cancel: false)
    }

    var settingsProgress: ShellProgressData? {
        guard isScanning else { return nil }
        if let progress = scanProgress, progress.total > 0 {
            return ShellProgressData(
                label: "Scanning",
                detail: "\(progress.completed) / \(progress.total)",
                fraction: Double(progress.completed) / Double(max(progress.total, 1)),
                cancel: false
            )
        }
        return ShellProgressData(label: "Scanning", cancel: false)
    }

    func adoptSavedFolder() async {
        guard let resolved = persistence.loadFolderBookmark() else {
            Log.library.warning("adoptSavedFolder: no bookmark to resolve")
            return
        }
        await cancelInFlightScan()
        folderURL = resolved
        startScanAccess(resolved)
        await rescan()
    }

    private func cancelInFlightScan() async {
        scanTask?.cancel()
        await scanTask?.value
        scanTask = nil
    }

    private func performScan(capturedURL: URL, revision: Int) async {
        let cachePath = snapshotCache.fileURL(root: capturedURL.standardized.path).path
        do {
            if let paint = try await core.tryOpenCached(
                root: capturedURL.standardized.path,
                device: deviceID,
                cachePath: cachePath
            ) {
                guard !Task.isCancelled, folderURL == capturedURL, scanRevision == revision else {
                    return
                }
                try await applyPaintedLibrary(paint)
                await Task.yield()
                try await core.reload()
                guard !Task.isCancelled, folderURL == capturedURL, scanRevision == revision else {
                    return
                }
                try await applyPaintedLibrary(try await core.libraryPaintRows())
                let requests = try await core.pendingMetadataRequests()
                try await enrichAndFinish(
                    requests,
                    capturedURL: capturedURL,
                    revision: revision,
                    cachePath: cachePath
                )
                return
            }

            let requests = try await core.prepare(
                root: capturedURL.standardized.path,
                device: deviceID
            )
            guard !Task.isCancelled, folderURL == capturedURL, scanRevision == revision else {
                return
            }
            try await enrichAndFinish(
                requests,
                capturedURL: capturedURL,
                revision: revision,
                cachePath: cachePath
            )
        } catch is CancellationError {
            Log.library.debug("Discarding cancelled MusicSession rescan")
        } catch {
            report(error)
        }
    }

    private func enrichAndFinish(
        _ requests: [MetadataRequest],
        capturedURL: URL,
        revision: Int,
        cachePath: String
    ) async throws {
        scanProgress = ScanProgress(completed: 0, total: requests.count)
        let enriched = try await loadMetadata(requests, revision: revision)
        try Task.checkCancellation()
        try await core.applyMetadata(enriched.map(\.result))
        let snapshot = try await core.librarySnapshot(
            query: searchText,
            sort: sortOption.coreValue
        )
        let rows = try await core.sessionRows()
        guard !Task.isCancelled, folderURL == capturedURL, scanRevision == revision else {
            return
        }

        var byID = tracksByCoreID
        for item in enriched {
            byID[item.track.coreID] = item.track
        }
        let liveIDs = Set(snapshot.allTrackIDs)
        byID = byID.filter { liveIDs.contains($0.key) }
        tracksByCoreID = byID
        rebuildURLIndex()
        apply(snapshot)
        apply(rows)
        try? await core.saveLibraryCache(cachePath: cachePath)
        let now = Date()
        persistence.saveLastSynced(now)
        lastSynced = now
        errorMessage = nil
        Log.library.info("MusicSession rescan complete: \(tracks.count) tracks")
    }

    private func applyPaintedLibrary(_ paint: [LibraryPaintRow]) async throws {
        var byID: [String: Track] = [:]
        for row in paint {
            let track = Track(paint: row)
            byID[track.coreID] = track
        }
        tracksByCoreID = byID
        rebuildURLIndex()
        let snapshot = try await core.librarySnapshot(
            query: searchText,
            sort: sortOption.coreValue
        )
        let rows = try await core.sessionRows()
        apply(snapshot)
        apply(rows)
        errorMessage = nil
    }

    private func loadMetadata(
        _ requests: [MetadataRequest],
        revision: Int
    ) async throws -> [HostTrackMetadata] {
        guard !requests.isEmpty else { return [] }
        var loaded: [HostTrackMetadata] = []
        loaded.reserveCapacity(requests.count)
        return try await withThrowingTaskGroup(of: HostTrackMetadata.self) { group in
            var iterator = requests.makeIterator()
            for _ in 0..<min(Self.metadataConcurrency, requests.count) {
                if let request = iterator.next() {
                    group.addTask { try await MetadataLoader.loadMetadata(for: request) }
                }
            }
            for try await item in group {
                try Task.checkCancellation()
                loaded.append(item)
                if loaded.count % 25 == 0 || loaded.count == requests.count {
                    if scanRevision == revision {
                        scanProgress = ScanProgress(
                            completed: loaded.count,
                            total: requests.count
                        )
                    }
                }
                if let request = iterator.next() {
                    group.addTask { try await MetadataLoader.loadMetadata(for: request) }
                }
            }
            return loaded
        }
    }

    private func startScanAccess(_ url: URL) {
        if let current = scanAccessURL {
            if current == url { return }
            current.stopAccessingSecurityScopedResource()
        }
        _ = url.startAccessingSecurityScopedResource()
        scanAccessURL = url
    }

    // MARK: - Core-owned view projection

    private func scheduleApply(immediate: Bool = false) {
        applyTask?.cancel()
        applyRevision += 1
        let revision = applyRevision
        let query = searchText
        let sort = sortOption.coreValue
        let knownTrackIDs = tracks.map(\.coreID)
        isFiltering = !immediate && !query.isEmpty
        applyTask = Task { [weak self] in
            guard let self else { return }
            if !immediate {
                try? await Task.sleep(for: .milliseconds(250))
            }
            guard !Task.isCancelled else { return }
            do {
                let snapshot = try await core.librarySnapshot(
                    query: query,
                    sort: sort,
                    knownAllTrackIDs: knownTrackIDs
                )
                guard revision == applyRevision, !Task.isCancelled else { return }
                apply(snapshot)
                isFiltering = false
            } catch is CancellationError {
                return
            } catch {
                guard revision == applyRevision else { return }
                isFiltering = false
                report(error)
            }
        }
    }

    private func apply(_ snapshot: CoreLibrarySnapshot) {
        tracks = snapshot.allTrackIDs.compactMap { tracksByCoreID[$0] }
        displayTracks = snapshot.visibleTrackIDs.compactMap { tracksByCoreID[$0] }
        sections = snapshot.sections.map { section in
            LibrarySection(
                id: section.id,
                title: section.title,
                tracks: section.trackIDs.compactMap { tracksByCoreID[$0] }
            )
        }
        contentState = snapshot.contentState
        searchHits = snapshot.searchHits
    }

    private func apply(_ rows: CoreSessionRows) {
        playlists = rows.playlists
        syncConflictGroups = rows.conflicts
        scanIssues = rows.scanIssues
        settingsInfoRows = rows.settingsInfo
    }

    private func rebuildURLIndex() {
        tracksByURL = Dictionary(
            uniqueKeysWithValues: tracksByCoreID.values.map { ($0.url.standardized, $0) }
        )
    }

    // MARK: - Typed playlist commands

    func refreshPlaylistsFromDisk() async {
        await rescan()
    }

    @discardableResult
    func createPlaylist(name: String) async -> Playlist? {
        do {
            let id = try await core.createPlaylist(name: name)
            try await refreshSessionRows()
            return playlist(id: id)
        } catch {
            report(error)
            return nil
        }
    }

    func addTrack(_ track: Track, to playlistID: String) async {
        guard let playlist = playlist(id: playlistID) else { return }
        do {
            _ = try await core.addTracks(
                playlistID: playlist.id,
                token: playlist.contentToken,
                trackIDs: [track.coreID]
            )
            try await refreshSessionRows()
        } catch {
            await recoverFromPlaylistError(error)
        }
    }

    func replacePlaylistSelection(playlistID: String, selectedTrackIDs: Set<String>) async {
        guard let playlist = playlist(id: playlistID) else { return }
        let existing = Set(playlist.entries.compactMap(\.trackID))
        let removeIDs: [String] = playlist.entries.compactMap { entry in
            guard let trackID = entry.trackID, !selectedTrackIDs.contains(trackID) else {
                return nil
            }
            return entry.id
        }
        let addIDs = tracks.map(\.coreID).filter {
            selectedTrackIDs.contains($0) && !existing.contains($0)
        }
        var token = playlist.contentToken
        do {
            if !removeIDs.isEmpty {
                token = try await core.removeEntries(
                    playlistID: playlistID,
                    token: token,
                    entryIDs: removeIDs
                )
            }
            if !addIDs.isEmpty {
                _ = try await core.addTracks(
                    playlistID: playlistID,
                    token: token,
                    trackIDs: addIDs
                )
            }
            try await refreshSessionRows()
        } catch {
            await recoverFromPlaylistError(error)
        }
    }

    func removePlaylistEntries(playlistID: String, offsets: IndexSet) async {
        guard let playlist = playlist(id: playlistID) else { return }
        let ids = offsets.compactMap { playlist.entries[safe: $0]?.id }
        guard !ids.isEmpty else { return }
        do {
            _ = try await core.removeEntries(
                playlistID: playlistID,
                token: playlist.contentToken,
                entryIDs: ids
            )
            try await refreshSessionRows()
        } catch {
            await recoverFromPlaylistError(error)
        }
    }

    func movePlaylistEntry(playlistID: String, from: IndexSet, to destination: Int) async {
        guard from.count == 1,
              let source = from.first,
              let playlist = playlist(id: playlistID),
              let moved = playlist.entries[safe: source]
        else { return }

        var remaining = playlist.entries
        remaining.remove(at: source)
        let insertion = min(destination > source ? destination - 1 : destination, remaining.count)
        let beforeID = remaining[safe: insertion]?.id
        do {
            _ = try await core.moveEntry(
                playlistID: playlistID,
                token: playlist.contentToken,
                entryID: moved.id,
                beforeEntryID: beforeID
            )
            try await refreshSessionRows()
        } catch {
            await recoverFromPlaylistError(error)
        }
    }

    func deletePlaylists(at offsets: IndexSet) async {
        let selected = offsets.compactMap { playlists[safe: $0] }
        do {
            for playlist in selected {
                try await core.deletePlaylist(
                    playlistID: playlist.id,
                    token: playlist.contentToken
                )
            }
            try await refreshSessionRows()
        } catch {
            await recoverFromPlaylistError(error)
        }
    }

    private func refreshSessionRows() async throws {
        apply(try await core.sessionRows())
        errorMessage = nil
    }

    private func recoverFromPlaylistError(_ error: Error) async {
        let message = displayMessage(for: error)
        report(error)
        await rescan()
        errorMessage = message
    }

    // MARK: - Syncthing conflicts

    func conflictChoices(groupID: String) async throws -> [TextRow] {
        try await core.conflictChoices(groupID: groupID)
    }

    func resolveConflict(groupID: String, selectedSource: String?) async throws {
        do {
            try await core.resolveConflict(groupID: groupID, selectedSource: selectedSource)
            try await refreshSessionRows()
            let snapshot = try await core.librarySnapshot(
                query: searchText,
                sort: sortOption.coreValue,
                knownAllTrackIDs: tracks.map(\.coreID)
            )
            apply(snapshot)
        } catch {
            report(error)
            throw error
        }
    }

    func clearError() {
        errorMessage = nil
    }

    private func report(_ error: Error) {
        let message = displayMessage(for: error)
        errorMessage = message
        Log.library.error("\(message)")
    }

    private func displayMessage(for error: Error) -> String {
        (error as? MusicError)?.displayMessage ?? error.localizedDescription
    }

    #if DEBUG
    func _testOpenFolder(_ url: URL) async {
        await cancelInFlightScan()
        folderURL = url
        startScanAccess(url)
        await rescan()
    }

    func _testWaitForApply() async {
        await applyTask?.value
    }
    #endif
}

struct LibrarySection: Identifiable, Equatable, Sendable {
    let id: String
    let title: String
    let tracks: [Track]
}
