import Foundation

/// Thin SwiftUI projection of core-owned playlist detail.
struct Playlist: Identifiable, Sendable {
    let id: String
    let name: String
    let location: String?
    let countLabel: String?
    var contentToken: String
    var entries: [PlaylistEntry]
    var actions: [ActionRow]

    var resolvedTrackIDs: [String] {
        entries.compactMap(\.trackID)
    }
}

/// One display row plus the optional host playback source returned by core.
struct PlaylistEntry: Identifiable, Sendable {
    let id: String
    let title: String
    let subtitle: String?
    let trailing: String?
    let trackID: String?
    let sourceURL: URL?
}
