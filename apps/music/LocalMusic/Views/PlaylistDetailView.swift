import SwiftUI

struct PlaylistDetailView: View {
    let playlistID: String
    @Environment(LibraryStore.self) private var library
    @State private var showAddTracks = false

    private var playlist: Playlist? {
        library.playlist(id: playlistID)
    }

    private var resolvedTracks: [Track] {
        playlist?.entries.compactMap { entry in
            entry.trackID.flatMap { library.track(coreID: $0) }
        } ?? []
    }

    var body: some View {
        Group {
            if let playlist {
                if playlist.entries.isEmpty {
                    emptyState
                } else {
                    listBody(playlist)
                }
            } else {
                ContentUnavailableView(
                    "Playlist Unavailable",
                    systemImage: "music.note.list",
                    description: Text("The playlist changed on disk.")
                )
            }
        }
        .navigationTitle(playlist?.name ?? "Playlist")
        .accessibilityIdentifier(MusicScreen.playlistDetail.rawValue)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    showAddTracks = true
                } label: {
                    Label("Add Tracks", systemImage: "plus")
                }
                .disabled(!actionEnabled("add-tracks"))
            }
            if !(playlist?.entries.isEmpty ?? true) {
                ToolbarItem(placement: .topBarTrailing) {
                    EditButton()
                }
            }
        }
        .sheet(isPresented: $showAddTracks) {
            AddTracksSheet(playlistID: playlistID)
        }
    }

    private var emptyState: some View {
        VStack(spacing: 16) {
            ZStack {
                RoundedRectangle(cornerRadius: 20)
                    .fill(Color.accentColor.opacity(0.12))
                    .frame(width: 100, height: 100)
                Image(systemName: "music.note")
                    .font(.system(size: 40))
                    .foregroundColor(Color.accentColor)
            }
            Text("Empty Playlist")
                .font(.title3)
                .fontWeight(.medium)
            Text("Tap + to add tracks from your library.")
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func listBody(_ playlist: Playlist) -> some View {
        let resolved = resolvedTracks
        let missingCount = playlist.entries.count - resolved.count
        return List {
            if !resolved.isEmpty {
                Section {
                    PlaylistPlayAllButton(resolved: resolved)
                        .disabled(!actionEnabled("play-all"))
                }
            }

            Section {
                ForEach(Array(playlist.entries.enumerated()), id: \.element.id) { index, entry in
                    if let trackID = entry.trackID,
                       let track = library.track(coreID: trackID) {
                        let queueIndex = playlist.entries[..<index]
                            .compactMap(\.trackID)
                            .filter { library.track(coreID: $0) != nil }
                            .count
                        PlaylistTrackRowButton(
                            track: track,
                            resolvedQueue: resolved,
                            startIndex: queueIndex
                        )
                        .listRowSeparator(.hidden)
                    } else {
                        MissingTrackRow(entry: entry)
                            .listRowSeparator(.hidden)
                    }
                }
                .onMove { from, to in
                    Task {
                        await library.movePlaylistEntry(
                            playlistID: playlistID,
                            from: from,
                            to: to
                        )
                    }
                }
                .onDelete { offsets in
                    Task {
                        await library.removePlaylistEntries(
                            playlistID: playlistID,
                            offsets: offsets
                        )
                    }
                }
            } header: {
                if missingCount > 0 {
                    Text("\(resolved.count) available, \(missingCount) missing or unsupported")
                } else {
                    Text(playlist.countLabel ?? "\(resolved.count) tracks")
                }
            }
        }
        .listStyle(.plain)
        .listSectionSpacing(.compact)
        .environment(\.defaultMinListRowHeight, 0)
        .contentMargins(.bottom, 80, for: .scrollContent)
    }

    private func actionEnabled(_ id: String) -> Bool {
        playlist?.actions.first(where: { $0.id == id })?.enabled ?? false
    }
}

private struct PlaylistPlayAllButton: View {
    let resolved: [Track]
    @Environment(AudioPlayerManager.self) private var player

    var body: some View {
        Button {
            if let first = resolved.first {
                player.play(track: first, queue: resolved, startIndex: 0)
            }
        } label: {
            HStack {
                Spacer()
                Label("Play All", systemImage: "play.fill")
                    .font(.callout.weight(.semibold))
                    .foregroundColor(.white)
                Spacer()
            }
            .padding(.vertical, 10)
            .background(Color.accentColor, in: Capsule())
        }
        .buttonStyle(.plain)
    }
}

private struct PlaylistTrackRowButton: View {
    let track: Track
    let resolvedQueue: [Track]
    let startIndex: Int
    @Environment(AudioPlayerManager.self) private var player

    var body: some View {
        Button {
            player.play(track: track, queue: resolvedQueue, startIndex: startIndex)
        } label: {
            TrackRow(
                track: track,
                isCurrent: player.currentTrack?.id == track.id,
                isActivelyPlaying: player.isPlaying && player.currentTrack?.id == track.id
            )
        }
    }
}

struct MissingTrackRow: View {
    let entry: PlaylistEntry

    var body: some View {
        HStack(spacing: 14) {
            ZStack {
                RoundedRectangle(cornerRadius: 10)
                    .fill(Color.orange.opacity(0.15))
                    .frame(width: 52, height: 52)
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundColor(.orange)
            }
            VStack(alignment: .leading, spacing: 3) {
                Text(entry.title)
                    .font(.callout.weight(.medium))
                    .lineLimit(1)
                    .truncationMode(.middle)
                Text(entry.trailing ?? entry.subtitle ?? "Unavailable")
                    .font(.caption)
                    .foregroundStyle(.orange)
            }
            Spacer()
        }
        .padding(.vertical, 4)
    }
}

struct AddTracksSheet: View {
    let playlistID: String
    @Environment(LibraryStore.self) private var library
    @Environment(\.dismiss) private var dismiss

    @State private var searchDraft = ""
    @State private var results: [Track] = []
    @State private var selectedTrackIDs: Set<String> = []
    @State private var searchTask: Task<Void, Never>?
    @State private var isSaving = false

    var body: some View {
        NavigationStack {
            List(results) { track in
                let selected = selectedTrackIDs.contains(track.coreID)
                Button {
                    if selected {
                        selectedTrackIDs.remove(track.coreID)
                    } else {
                        selectedTrackIDs.insert(track.coreID)
                    }
                } label: {
                    HStack(spacing: 12) {
                        Image(systemName: selected ? "checkmark.circle.fill" : "circle")
                            .font(.title3)
                            .foregroundColor(selected ? Color.accentColor : .secondary)
                        TrackRow(track: track)
                    }
                }
                .listRowSeparator(.hidden)
            }
            .listStyle(.plain)
            .searchable(text: $searchDraft, prompt: "Search library")
            .onChange(of: searchDraft) { _, query in
                runSearch(query: query)
            }
            .navigationTitle("Add Tracks")
            .navigationBarTitleDisplayMode(.inline)
            .accessibilityIdentifier(MusicScreen.addTracks.rawValue)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                        .disabled(isSaving)
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") {
                        isSaving = true
                        Task {
                            await library.replacePlaylistSelection(
                                playlistID: playlistID,
                                selectedTrackIDs: selectedTrackIDs
                            )
                            library.restoreLibraryView()
                            dismiss()
                        }
                    }
                    .disabled(isSaving)
                }
            }
            .onAppear {
                selectedTrackIDs = Set(
                    library.playlist(id: playlistID)?.entries.compactMap(\.trackID) ?? []
                )
                runSearch(query: "", immediate: true)
            }
            .onDisappear {
                searchTask?.cancel()
                library.restoreLibraryView()
            }
        }
    }

    private func runSearch(query: String, immediate: Bool = false) {
        searchTask?.cancel()
        searchTask = Task {
            if !immediate {
                try? await Task.sleep(for: .milliseconds(200))
            }
            guard !Task.isCancelled else { return }
            let found = await library.searchTracks(query: query)
            guard !Task.isCancelled else { return }
            results = found
        }
    }
}
