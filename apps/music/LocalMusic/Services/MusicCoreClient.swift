import Foundation

extension MusicError {
    var displayMessage: String {
        switch self {
        case .Io(let message, _),
             .InvalidPlaylist(let message, _),
             .NotFound(let message, _),
             .InvalidCommand(let message, _),
             .StalePlaylist(let message, _),
             .StaleGeneration(let message, _),
             .NeedsChoice(let message, _),
             .UnsupportedConflict(let message, _):
            message
        }
    }
}

enum MusicCoreClientError: LocalizedError, Sendable {
    case noSession

    var errorDescription: String? {
        "Choose a music folder before using the library."
    }
}

struct CoreLibrarySection: Sendable {
    let id: String
    let title: String
    let trackIDs: [String]
}

struct CoreLibrarySnapshot: Sendable {
    let allTrackIDs: [String]
    let contentState: LibraryContentState
    let sections: [CoreLibrarySection]

    var visibleTrackIDs: [String] {
        sections.flatMap(\.trackIDs)
    }
}

struct CoreSessionRows: Sendable {
    let playlists: [Playlist]
    let conflicts: [ConflictRow]
    let scanIssues: [StatusRow]
}

/// Serializes access to the generated, internally locked `MusicSession` and
/// maps its list/window/detail APIs to small SwiftUI projections.
actor MusicCoreClient {
    private static let windowSize: UInt64 = 500

    private var session: MusicSession?
    private var root: String?

    func prepare(root: String, device: String) throws -> [MetadataRequest] {
        if self.root == root, let session {
            try session.reload()
        } else {
            self.session = try MusicSession.open(root: root, device: device)
            self.root = root
        }
        return try metadataRequests()
    }

    func applyMetadata(_ results: [MetadataResult]) throws {
        let session = try requireSession()
        for start in stride(from: 0, to: results.count, by: Int(Self.windowSize)) {
            let end = min(start + Int(Self.windowSize), results.count)
            _ = try session.applyMetadataBatch(results: Array(results[start..<end]))
        }
    }

    func librarySnapshot(
        query: String,
        sort: SortOption,
        knownAllTrackIDs: [String]? = nil
    ) throws -> CoreLibrarySnapshot {
        let session = try requireSession()
        let allTrackIDs: [String]
        if let knownAllTrackIDs {
            allTrackIDs = knownAllTrackIDs
        } else {
            allTrackIDs = try projectedTrackIDs(
                session: session,
                query: "",
                sort: sort
            ).ids
        }
        let visible = try projectedTrackIDs(session: session, query: query, sort: sort)
        return CoreLibrarySnapshot(
            allTrackIDs: allTrackIDs,
            contentState: try session.libraryContentState(),
            sections: visible.sections
        )
    }

    func searchTrackIDs(query: String, sort: SortOption) throws -> [String] {
        try projectedTrackIDs(session: requireSession(), query: query, sort: sort).ids
    }

    func sessionRows() throws -> CoreSessionRows {
        let session = try requireSession()
        let rows = try session.playlistRows()
        let playlists = try rows.map { row in
            let entryRows = try session.playlistEntryRows(playlistId: row.id)
            let sources = try session.playlistEntryMediaSources(playlistId: row.id)
            let sourceByEntry = Dictionary(
                uniqueKeysWithValues: sources.map { ($0.entryId, $0) }
            )
            return Playlist(
                id: row.id,
                name: row.title,
                location: row.subtitle,
                countLabel: row.trailing,
                contentToken: try session.playlistContentToken(playlistId: row.id),
                entries: entryRows.map { entry in
                    let source = sourceByEntry[entry.id]
                    return PlaylistEntry(
                        id: entry.id,
                        title: entry.title,
                        subtitle: entry.subtitle,
                        trailing: entry.trailing,
                        trackID: source?.trackId,
                        sourceURL: source.map { URL(fileURLWithPath: $0.path) }
                    )
                },
                actions: try session.playlistActionRows(playlistId: row.id)
            )
        }
        return CoreSessionRows(
            playlists: playlists,
            conflicts: try session.conflictRows(),
            scanIssues: try session.scanIssueRows()
        )
    }

    @discardableResult
    func createPlaylist(name: String) throws -> String {
        try requireSession().createPlaylist(command: CreatePlaylistCommand(name: name))
    }

    @discardableResult
    func addTracks(playlistID: String, token: String, trackIDs: [String]) throws -> String {
        try requireSession().addTracks(command: AddTracksCommand(
            playlistId: playlistID,
            contentToken: token,
            trackIds: trackIDs
        ))
    }

    @discardableResult
    func removeEntries(playlistID: String, token: String, entryIDs: [String]) throws -> String {
        try requireSession().removePlaylistEntries(command: RemovePlaylistEntriesCommand(
            playlistId: playlistID,
            contentToken: token,
            entryIds: entryIDs
        ))
    }

    @discardableResult
    func moveEntry(
        playlistID: String,
        token: String,
        entryID: String,
        beforeEntryID: String?
    ) throws -> String {
        try requireSession().movePlaylistEntry(command: MovePlaylistEntryCommand(
            playlistId: playlistID,
            contentToken: token,
            entryId: entryID,
            beforeEntryId: beforeEntryID
        ))
    }

    func deletePlaylist(playlistID: String, token: String) throws {
        try requireSession().deletePlaylist(command: DeletePlaylistCommand(
            playlistId: playlistID,
            contentToken: token
        ))
    }

    func conflictChoices(groupID: String) throws -> [TextRow] {
        try requireSession().conflictChoiceRows(groupId: groupID)
    }

    func resolveConflict(groupID: String, selectedSource: String?) throws {
        try requireSession().resolveConflict(command: ResolveConflictCommand(
            groupId: groupID,
            selectedSource: selectedSource
        ))
    }

    private func metadataRequests() throws -> [MetadataRequest] {
        let session = try requireSession()
        var requests: [MetadataRequest] = []
        var offset: UInt64 = 0
        while true {
            let window = try session.metadataRequests(offset: offset, limit: Self.windowSize)
            requests.append(contentsOf: window)
            if window.count < Int(Self.windowSize) { return requests }
            offset += UInt64(window.count)
        }
    }

    private func projectedTrackIDs(
        session: MusicSession,
        query: String,
        sort: SortOption
    ) throws -> (ids: [String], sections: [CoreLibrarySection]) {
        let generation = try session.setLibraryView(
            command: SetLibraryViewCommand(query: query, sort: sort)
        )
        var allIDs: [String] = []
        var sections: [CoreLibrarySection] = []
        for section in try session.librarySectionRows() {
            var trackIDs: [String] = []
            var offset: UInt64 = 0
            while true {
                let window = try session.libraryTrackRows(
                    sectionId: section.id,
                    offset: offset,
                    limit: Self.windowSize,
                    generation: generation
                )
                trackIDs.append(contentsOf: window.map(\.id))
                if window.count < Int(Self.windowSize) { break }
                offset += UInt64(window.count)
            }
            allIDs.append(contentsOf: trackIDs)
            sections.append(CoreLibrarySection(
                id: section.id,
                title: section.title,
                trackIDs: trackIDs
            ))
        }
        return (allIDs, sections)
    }

    private func requireSession() throws -> MusicSession {
        guard let session else { throw MusicCoreClientError.noSession }
        return session
    }
}
