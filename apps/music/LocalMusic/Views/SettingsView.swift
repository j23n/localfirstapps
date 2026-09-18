import SwiftUI
import ShellKitSwift

struct SettingsView: View {
    @Environment(LibraryStore.self) private var library

    private static let githubURL = URL(string: "https://github.com/j23n/localmusic")!

    private var appVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "1.0.0"
    }

    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL
    @State private var showFolderPicker = false

    var body: some View {
        ShellSettings(
            title: "Settings",
            tokens: ShellTokens(cardRadius: MusicTokens.cardRadius),
            dismissLabel: "Done",
            onDismiss: { dismiss() }
        ) {
                Section("Music Folder") {
                    Button {
                        showFolderPicker = true
                    } label: {
                        ShellTextRow(
                            .init(
                                title: "Folder",
                                trailingValue: library.folderURL?.lastPathComponent
                                    ?? "Not selected",
                                leadingSymbol: "folder"
                            )
                        )
                    }
                    .tint(.primary)

                    ShellActionRow(
                        .init(
                            actionID: "reload-music",
                            label: "Reload Music",
                            isEnabled: library.folderURL != nil
                                && !library.isScanning,
                            leadingSymbol: "arrow.clockwise"
                        )
                    ) { _ in
                        Task { await library.rescan() }
                    }

                    if let lastSynced = library.lastSynced {
                        LabeledContent("Last Synced", value: lastSynced, format: .dateTime)
                    }

                    if let progress = library.settingsProgress {
                        ShellProgressRow(progress)
                    }
                }

                Section("Stats") {
                    ShellTextRow(
                        .init(
                            title: "Total Songs",
                            trailingValue: "\(library.tracks.count)"
                        )
                    )
                    ShellTextRow(
                        .init(
                            title: "Total Playlists",
                            trailingValue: "\(library.playlists.count)"
                        )
                    )
                }

                Section("Diagnostics") {
                    ShellNavRow(
                        .init(
                            destinationID: "logs",
                            label: "Logs",
                            leadingSymbol: "doc.text.magnifyingglass"
                        )
                    ) {
                        LogsView()
                    }
                    ShellTextRow(
                        .init(title: "Version", trailingValue: appVersion)
                    )
                }

                Section("About") {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("LocalMusic plays audio files from a folder of your choice — no streaming, no accounts.")
                            .font(.callout)

                        Text("Found a bug or have feedback? Open an issue or get in touch:")
                            .font(.callout)
                            .foregroundStyle(.secondary)
                    }
                    .padding(.vertical, 4)

                    Button {
                        openURL(Self.githubURL)
                    } label: {
                        LabeledContent {
                            Image(systemName: "arrow.up.right")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        } label: {
                            Label("GitHub", systemImage: "chevron.left.forwardslash.chevron.right")
                        }
                    }
                    .tint(.primary)
                }
        }
            .accessibilityIdentifier(MusicScreen.settings.rawValue)
            .sheet(isPresented: $showFolderPicker) {
                DocumentPicker { pickerURL in
                    // The picker's URL carries a transient security scope that
                    // must be claimed and turned into a bookmark synchronously
                    // here; the rescan can then run as a Task.
                    _ = pickerURL.startAccessingSecurityScopedResource()
                    PersistenceManager.shared.saveFolderBookmark(pickerURL)
                    pickerURL.stopAccessingSecurityScopedResource()
                    Task {
                        await library.adoptSavedFolder()
                    }
                }
            }
    }
}
