import SwiftUI
import ShellKitSwift

struct PlaylistsView: View {
    // Doesn't observe `AudioPlayerManager` for the same reason as `LibraryView`:
    // playback ticks shouldn't re-render the playlist list.
    @Environment(LibraryStore.self) private var library
    @State private var showNewPlaylistAlert = false
    @State private var newPlaylistName = ""
    @State private var showSyncConflicts = false
    @State private var pendingDeleteOffsets: IndexSet?

    var body: some View {
        NavigationStack {
            Group {
                if library.playlists.isEmpty {
                    emptyState
                } else {
                    List {
                        ForEach(library.playlists) { playlist in
                            NavigationLink {
                                PlaylistDetailView(playlistID: playlist.id)
                            } label: {
                                playlistRow(playlist)
                            }
                            .listRowSeparator(.hidden)
                            .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 0, trailing: 16))
                        }
                        .onDelete { offsets in
                            pendingDeleteOffsets = offsets
                        }
                    }
                    .listStyle(.plain)
                    .listSectionSpacing(.compact)
                    .environment(\.defaultMinListRowHeight, 0)
                }
            }
            .navigationTitle("Playlists")
            .refreshable {
                await library.refreshPlaylistsFromDisk()
            }
            .accessibilityIdentifier(MusicScreen.playlistList.rawValue)
            .toolbar {
                if !library.syncConflictGroups.isEmpty {
                    ToolbarItem(placement: .topBarLeading) {
                        Button {
                            showSyncConflicts = true
                        } label: {
                            Label(
                                "Sync Conflicts",
                                systemImage: "exclamationmark.arrow.triangle.2.circlepath"
                            )
                        }
                        .accessibilityIdentifier(MusicScreen.syncConflictGroup.rawValue)
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        newPlaylistName = ""
                        showNewPlaylistAlert = true
                    } label: {
                        Label("New Playlist", systemImage: "plus")
                    }
                }
            }
            .alert("New Playlist", isPresented: $showNewPlaylistAlert) {
                TextField("Playlist name", text: $newPlaylistName)
                Button("Cancel", role: .cancel) { }
                Button("Create") {
                    Task { _ = await library.createPlaylist(name: newPlaylistName) }
                }
            } message: {
                Text("Enter a name for the new playlist.")
            }
            .sheet(isPresented: $showSyncConflicts) {
                MusicSyncConflictSheet()
            }
            .shellConfirmation(
                data: .init(
                    actionID: "delete-playlist",
                    question: "Delete Playlist?",
                    destructiveLabel: "Delete",
                    message: "This removes the playlist file from the selected folder."
                ),
                isPresented: Binding(
                    get: { pendingDeleteOffsets != nil },
                    set: { if !$0 { pendingDeleteOffsets = nil } }
                )
            ) { _ in
                guard let offsets = pendingDeleteOffsets else { return }
                pendingDeleteOffsets = nil
                Task { await library.deletePlaylists(at: offsets) }
            }
            .alert("Playlist Error", isPresented: Binding(
                get: { library.errorMessage != nil },
                set: { if !$0 { library.clearError() } }
            )) {
                Button("OK") { library.clearError() }
            } message: {
                Text(library.errorMessage ?? "")
            }
        }
    }

    private var emptyState: some View {
        VStack(spacing: 16) {
            ZStack {
                RoundedRectangle(cornerRadius: 20)
                    .fill(Color.accentColor.opacity(0.12))
                    .frame(width: 100, height: 100)
                Image(systemName: "rectangle.stack")
                    .font(.system(size: 40))
                    .foregroundColor(Color.accentColor)
            }
            Text("No Playlists Found")
                .font(.title3)
                .fontWeight(.medium)
            Text("Add .m3u or .pls files to your music folder, or tap + to create one.")
                .font(.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 40)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    @ViewBuilder
    private func playlistRow(_ playlist: Playlist) -> some View {
        HStack(spacing: 14) {
            PlaylistMosaicView(
                trackURLs: playlist.entries.compactMap(\.sourceURL)
            )
                .frame(width: 52, height: 52)
                .clipShape(RoundedRectangle(cornerRadius: 10))

            VStack(alignment: .leading, spacing: 3) {
                Text(playlist.name)
                    .font(.callout)
                    .fontWeight(.medium)
                Text(playlist.countLabel ?? "0 tracks")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 2)
    }
}

// MARK: - Playlist Mosaic Thumbnail

struct PlaylistMosaicView: View {
    let trackURLs: [URL]
    @Environment(LibraryStore.self) private var library

    /// Resolves up to four URLs that have artwork, using the store's O(1)
    /// lookup instead of a per-row linear scan over the library.
    private var artworkURLs: [URL] {
        var result: [URL] = []
        for url in trackURLs {
            if result.count >= 4 { break }
            if let track = library.track(forURL: url), track.hasArtwork {
                result.append(track.url)
            }
        }
        return result
    }

    var body: some View {
        GeometryReader { geo in
            let half = geo.size.width / 2
            let urls = artworkURLs
            if urls.isEmpty {
                ZStack {
                    Color(white: 0.85).opacity(0.5)
                    Image(systemName: "music.note.list")
                        .font(.body)
                        .foregroundStyle(Color(white: 0.55))
                }
            } else if urls.count == 1 {
                ArtworkView(trackURL: urls[0], hasArtwork: true, pointSize: geo.size.width)
            } else {
                let grid = padded(urls, to: 4)
                VStack(spacing: 0) {
                    HStack(spacing: 0) {
                        cell(grid[0], side: half)
                        cell(grid[1], side: half)
                    }
                    HStack(spacing: 0) {
                        cell(grid[2], side: half)
                        cell(grid[3], side: half)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func cell(_ url: URL?, side: CGFloat) -> some View {
        if let url {
            ArtworkView(trackURL: url, hasArtwork: true, pointSize: side)
                .frame(width: side, height: side)
                .clipped()
        } else {
            Color(white: 0.85).opacity(0.5)
                .frame(width: side, height: side)
        }
    }

    private func padded(_ urls: [URL], to count: Int) -> [URL?] {
        var result: [URL?] = urls.map { $0 }
        while result.count < count { result.append(nil) }
        return result
    }
}
