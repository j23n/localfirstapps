import SwiftUI
import ShellKitSwift

struct LibraryView: View {
    // Intentionally does NOT observe `AudioPlayerManager`: that would force
    // a body re-render on every 0.5 s playback tick. Per-row playback state
    // is read inside `TrackRowButton`, where the cost is bounded.
    @Environment(LibraryStore.self) private var library
    @State private var showSettings = false
    @State private var showFolderPicker = false
    @State private var searchDraft = ""

    var body: some View {
        NavigationStack {
            Group {
                if library.folderURL == nil && library.tracks.isEmpty {
                    onboardingView
                } else if library.isScanning && library.tracks.isEmpty {
                    scanningView
                } else if library.tracks.isEmpty {
                    emptyStateView
                } else {
                    trackListView
                }
            }
            .navigationTitle("Library")
            .toolbar {
                ToolbarItem(placement: .principal) {
                    if let progress = library.chromeProgress {
                        ShellProgressChip(progress)
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    sortMenu
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        showSettings = true
                    } label: {
                        Image(systemName: "gear")
                    }
                }
            }
            .sheet(isPresented: $showSettings) {
                SettingsView()
            }
            .shellSearch(
                text: $searchDraft,
                data: .init(prompt: "Search by title, artist, or album")
            )
            .onChange(of: searchDraft) { _, newValue in
                library.searchText = newValue
            }
            .refreshable {
                await library.rescan()
            }
            .accessibilityIdentifier(MusicScreen.library.rawValue)
            .alert("Library Error", isPresented: Binding(
                get: { library.errorMessage != nil },
                set: { if !$0 { library.clearError() } }
            )) {
                Button("OK") { library.clearError() }
            } message: {
                Text(library.errorMessage ?? "")
            }
        }
    }

    // MARK: - Subviews

    private var sortMenu: some View {
        Menu {
            Picker("Sort By", selection: Binding(
                get: { library.sortOption },
                set: { library.sortOption = $0 }
            )) {
                ForEach(LibrarySortOption.allCases) { option in
                    Label(option.label, systemImage: option.icon).tag(option)
                }
            }
        } label: {
            Image(systemName: "arrow.up.arrow.down")
        }
    }

    private var onboardingView: some View {
        VStack(spacing: 24) {
            Spacer()

            Image(systemName: "music.note.house")
                .font(.system(size: 64))
                .foregroundStyle(.secondary)

            Text("Welcome to LocalMusic")
                .font(.title2.bold())

            Text("Select a folder containing your audio files to get started.")
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
                .padding(.horizontal, 32)

            Button {
                showFolderPicker = true
            } label: {
                Label("Choose Folder", systemImage: "folder")
                    .font(.headline)
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .padding(.horizontal, 48)

            Spacer()
        }
        .sheet(isPresented: $showFolderPicker) {
            DocumentPicker { pickerURL in
                _ = pickerURL.startAccessingSecurityScopedResource()
                PersistenceManager.shared.saveFolderBookmark(pickerURL)
                pickerURL.stopAccessingSecurityScopedResource()
                Task {
                    await library.adoptSavedFolder()
                }
            }
        }
        .accessibilityIdentifier(MusicScreen.folderPicker.rawValue)
    }

    private var emptyStateView: some View {
        VStack(spacing: 16) {
            ZStack {
                RoundedRectangle(cornerRadius: 20)
                    .fill(Color.accentColor.opacity(0.12))
                    .frame(width: 100, height: 100)
                Image(systemName: "music.note")
                    .font(.system(size: 40))
                    .foregroundColor(Color.accentColor)
            }
            Text("No Audio Files Found")
                .font(.title3)
                .fontWeight(.medium)
            Text("The selected folder contains no supported audio files.\nTry choosing a different folder.")
                .font(.body)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 40)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var scanningView: some View {
        VStack(spacing: 12) {
            ProgressView()
            Group {
                if let progress = library.scanProgress, progress.total > 0 {
                    Text("Scanning \(progress.completed) of \(progress.total)…")
                        .monospacedDigit()
                } else {
                    Text("Scanning folder…")
                }
            }
            .font(.callout)
            .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var isSearching: Bool {
        !library.searchText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private var playlistHits: [Playlist] {
        guard isSearching else { return [] }
        let needle = library.searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        return library.playlists.filter {
            $0.name.range(of: needle, options: .caseInsensitive) != nil
        }
    }

    private var artistHits: [String] {
        uniqueNames(matching: \.artist)
    }

    private var albumHits: [String] {
        uniqueNames(matching: \.album)
    }

    private func uniqueNames(matching keyPath: KeyPath<Track, String>) -> [String] {
        guard isSearching else { return [] }
        let needle = library.searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        var seen = Set<String>()
        var names: [String] = []
        for track in library.tracks {
            let value = track[keyPath: keyPath]
            guard value.range(of: needle, options: .caseInsensitive) != nil else { continue }
            if seen.insert(value.lowercased()).inserted {
                names.append(value)
            }
        }
        return names
    }

    @ViewBuilder
    private var trackListView: some View {
        let sections = library.sections
        let total = library.displayTracks.count
        List {
            if !library.scanIssues.isEmpty {
                Section("Scan Issues") {
                    ForEach(library.scanIssues, id: \.id) { issue in
                        Label(
                            issue.message,
                            systemImage: issue.severity == .error
                                ? "xmark.octagon"
                                : "exclamationmark.triangle"
                        )
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    }
                }
            }

            if total == 0 && playlistHits.isEmpty && artistHits.isEmpty && albumHits.isEmpty {
                Section {
                    Text(library.searchText.isEmpty
                         ? "No tracks."
                         : "No matches for “\(library.searchText)”")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .center)
                        .padding(.vertical, 30)
                }
                .listRowSeparator(.hidden)
            } else {
                if !artistHits.isEmpty {
                    Section {
                        ForEach(artistHits, id: \.self) { artist in
                            SearchLocationButton(
                                title: artist,
                                subtitle: "Artist",
                                symbol: MusicSearchKind.artist.symbol,
                                tracks: library.tracks.filter { $0.artist == artist }
                            )
                            .listRowSeparator(.hidden)
                            .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 0, trailing: 16))
                        }
                    } header: {
                        Text("Artists")
                            .font(.caption)
                            .fontWeight(.semibold)
                            .foregroundStyle(.primary)
                    }
                }

                if !albumHits.isEmpty {
                    Section {
                        ForEach(albumHits, id: \.self) { album in
                            SearchLocationButton(
                                title: album,
                                subtitle: "Album",
                                symbol: MusicSearchKind.album.symbol,
                                tracks: library.tracks.filter { $0.album == album }
                            )
                            .listRowSeparator(.hidden)
                            .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 0, trailing: 16))
                        }
                    } header: {
                        Text("Albums")
                            .font(.caption)
                            .fontWeight(.semibold)
                            .foregroundStyle(.primary)
                    }
                }

                if !playlistHits.isEmpty {
                    Section {
                        ForEach(playlistHits) { playlist in
                            NavigationLink {
                                PlaylistDetailView(playlistID: playlist.id)
                            } label: {
                                MusicSearchHitRow(
                                    title: playlist.name,
                                    subtitle: playlist.countLabel,
                                    symbol: MusicSearchKind.playlist.symbol
                                )
                            }
                            .listRowSeparator(.hidden)
                            .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 0, trailing: 16))
                        }
                    } header: {
                        Text("Playlists")
                            .font(.caption)
                            .fontWeight(.semibold)
                            .foregroundStyle(.primary)
                    }
                }

                if total > 0 {
                    Section {
                        Text("\(total) track\(total == 1 ? "" : "s")")
                            .font(.caption)
                            .fontWeight(.medium)
                            .textCase(.uppercase)
                            .tracking(0.5)
                            .foregroundStyle(.secondary)
                    }
                    .listRowSeparator(.hidden)
                    .listRowInsets(EdgeInsets(top: 2, leading: 16, bottom: 2, trailing: 16))

                    ForEach(sections) { section in
                        Section {
                            ForEach(section.tracks) { track in
                                TrackRowButton(
                                    track: track,
                                    matchKind: isSearching
                                        ? MusicSearchKind.firstMatch(track: track, query: library.searchText)
                                        : nil
                                )
                                .listRowSeparator(.hidden)
                                .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 0, trailing: 16))
                            }
                        } header: {
                            Text(section.title)
                                .font(.caption)
                                .fontWeight(.semibold)
                                .foregroundStyle(.primary)
                        }
                    }
                }
            }
        }
        .listStyle(.plain)
        .listSectionSpacing(.compact)
        .environment(\.defaultMinListRowHeight, 0)
        .contentMargins(.bottom, 80, for: .scrollContent)
    }
}

// MARK: - Row helpers

/// Wraps a `TrackRow` with the play action and Add-to-Playlist context menu.
/// Pulled out so SwiftUI doesn't re-evaluate the entire `LibraryView` body
/// each time `player.currentTrack` ticks.
private struct TrackRowButton: View {
    let track: Track
    var matchKind: MusicSearchKind? = nil
    @Environment(LibraryStore.self) private var library
    @Environment(AudioPlayerManager.self) private var player

    var body: some View {
        Button {
            let queue = library.displayTracks
            if let idx = queue.firstIndex(where: { $0.id == track.id }) {
                player.play(track: track, queue: queue, startIndex: idx)
            }
        } label: {
            TrackRow(track: track,
                     isCurrent: player.currentTrack?.id == track.id,
                     isActivelyPlaying: player.isPlaying && player.currentTrack?.id == track.id,
                     matchKind: matchKind)
        }
        .contextMenu {
            if !library.playlists.isEmpty {
                Menu("Add to Playlist") {
                    ForEach(library.playlists) { playlist in
                        Button(playlist.name) {
                            addTrack(track, to: playlist)
                        }
                    }
                }
            }
        }
    }

    private func addTrack(_ track: Track, to playlist: Playlist) {
        Task {
            await library.addTrack(track, to: playlist.id)
        }
    }
}

// MARK: - Track Row

struct TrackRow: View {
    let track: Track
    var isCurrent: Bool = false
    var isActivelyPlaying: Bool = false
    var matchKind: MusicSearchKind? = nil

    init(
        track: Track,
        isCurrent: Bool = false,
        isActivelyPlaying: Bool = false,
        matchKind: MusicSearchKind? = nil
    ) {
        self.track = track
        self.isCurrent = isCurrent
        self.isActivelyPlaying = isActivelyPlaying
        self.matchKind = matchKind
    }

    /// Compatibility for call sites not yet updated.
    init(track: Track, isPlaying: Bool) {
        self.track = track
        self.isCurrent = isPlaying
        self.isActivelyPlaying = isPlaying
        self.matchKind = nil
    }

    var body: some View {
        HStack(spacing: 14) {
            ZStack {
                if let matchKind {
                    Image(systemName: matchKind.systemImage)
                        .font(.body)
                        .foregroundStyle(.secondary)
                        .frame(width: 52, height: 52)
                } else {
                    ArtworkView(trackURL: track.url,
                                hasArtwork: track.hasArtwork,
                                pointSize: 52)
                        .frame(width: 52, height: 52)
                        .clipShape(RoundedRectangle(cornerRadius: 10))
                }

                if isCurrent && matchKind == nil {
                    RoundedRectangle(cornerRadius: 10)
                        .fill(.black.opacity(0.4))
                        .frame(width: 52, height: 52)
                    if isActivelyPlaying {
                        NowPlayingBars()
                            .frame(width: 16, height: 14)
                    }
                }
            }

            VStack(alignment: .leading, spacing: 3) {
                Text(track.title)
                    .font(.callout)
                    .fontWeight(.medium)
                    .lineLimit(1)
                    .foregroundColor(isCurrent ? Color.accentColor : .primary)
                Text(track.artist)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer()

            Text(formatDuration(track.duration))
                .font(.caption)
                .foregroundStyle(.tertiary)
                .monospacedDigit()
        }
        .padding(.vertical, 2)
    }

    private func formatDuration(_ seconds: Double) -> String {
        guard seconds > 0 else { return "0:00" }
        let mins = Int(seconds) / 60
        let secs = Int(seconds) % 60
        return String(format: "%d:%02d", mins, secs)
    }
}

// MARK: - Now Playing Bars Animation

struct NowPlayingBars: View {
    @State private var animating = false

    var body: some View {
        HStack(spacing: 2) {
            bar(delay: 0.0)
            bar(delay: 0.2)
            bar(delay: 0.4)
        }
        .onAppear { animating = true }
    }

    private func bar(delay: Double) -> some View {
        RoundedRectangle(cornerRadius: 1)
            .fill(.white)
            .frame(width: 3)
            .scaleEffect(y: animating ? 1.0 : 0.3, anchor: .bottom)
            .animation(
                .easeInOut(duration: 0.5)
                .repeatForever(autoreverses: true)
                .delay(delay),
                value: animating
            )
    }
}

// MARK: - Search match category

/// Same kinds as `music-core::SearchKind`. Icons use the core symbolic
/// names so GTK and Swift stay on one table.
enum MusicSearchKind: String {
    case track, album, artist, playlist

    /// Symbolic name from `SearchKind::symbol`.
    var symbol: String {
        switch self {
        case .track: "audio-x-generic-symbolic"
        case .album: "media-optical-symbolic"
        case .artist: "system-users-symbolic"
        case .playlist: "view-list-symbolic"
        }
    }

    var systemImage: String {
        switch symbol {
        case "audio-x-generic-symbolic": "music.note"
        case "media-optical-symbolic": "opticaldisc"
        case "system-users-symbolic": "person.2"
        case "view-list-symbolic": "list.bullet"
        default: "music.note"
        }
    }

    var label: String {
        switch self {
        case .track: "Track"
        case .album: "Album"
        case .artist: "Artist"
        case .playlist: "Playlist"
        }
    }

    /// First field that contains `query`, in title → artist → album order.
    static func firstMatch(track: Track, query: String) -> MusicSearchKind {
        let needle = query.trimmingCharacters(in: .whitespacesAndNewlines)
        if track.title.range(of: needle, options: .caseInsensitive) != nil {
            return .track
        }
        if track.artist.range(of: needle, options: .caseInsensitive) != nil {
            return .artist
        }
        if track.album.range(of: needle, options: .caseInsensitive) != nil {
            return .album
        }
        return .track
    }
}

private struct SearchLocationButton: View {
    let title: String
    let subtitle: String
    let symbol: String
    let tracks: [Track]
    @Environment(AudioPlayerManager.self) private var player

    var body: some View {
        Button {
            if let first = tracks.first {
                player.play(track: first, queue: tracks, startIndex: 0)
            }
        } label: {
            MusicSearchHitRow(title: title, subtitle: subtitle, symbol: symbol)
        }
        .buttonStyle(.plain)
    }
}

struct MusicSearchHitRow: View {
    let title: String
    var subtitle: String? = nil
    let symbol: String

    var body: some View {
        HStack(spacing: 14) {
            Image(systemName: Self.systemImage(for: symbol))
                .font(.body)
                .foregroundStyle(.secondary)
                .frame(width: 52, height: 52)
            VStack(alignment: .leading, spacing: 3) {
                Text(title)
                    .font(.callout)
                    .fontWeight(.medium)
                    .lineLimit(1)
                if let subtitle, !subtitle.isEmpty {
                    Text(subtitle)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
            Spacer()
        }
        .padding(.vertical, 2)
    }

    static func systemImage(for symbol: String) -> String {
        switch symbol {
        case "audio-x-generic-symbolic": "music.note"
        case "media-optical-symbolic": "opticaldisc"
        case "system-users-symbolic": "person.2"
        case "view-list-symbolic": "list.bullet"
        default: "music.note"
        }
    }
}
