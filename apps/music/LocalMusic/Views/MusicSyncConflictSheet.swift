import SwiftUI

/// Minimal native review surface for Syncthing `.m3u` / `.m3u8` groups.
struct MusicSyncConflictSheet: View {
    @Environment(LibraryStore.self) private var library
    @Environment(\.dismiss) private var dismiss
    @State private var confirmingGroup: ConflictRow?
    @State private var choiceGroupID: String?
    @State private var choiceRows: [TextRow] = []
    @State private var selectedSource: String?
    @State private var errorMessage: String?
    @State private var isResolving = false

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Text("Playlist files changed on more than one device. LocalMusic preserves every copy until you confirm a core-planned resolution.")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }

                ForEach(library.syncConflictGroups, id: \.id) { group in
                    Section(group.title) {
                        LabeledContent(group.subtitle, value: group.trailing)
                        switch group.disposition {
                        case .choice:
                            Button("Choose Playlist Order") {
                                Task { await loadChoices(for: group) }
                            }
                        case .auto, .deletedVersusModified:
                            Button("Review and Resolve") {
                                confirmingGroup = group
                            }
                        case .manualOnly:
                            Text("Resolve this audio or PLS conflict in Files. LocalMusic will not rewrite it.")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }
            .navigationTitle("Sync Conflicts")
            .navigationBarTitleDisplayMode(.inline)
            .accessibilityIdentifier(MusicScreen.syncConflictGroup.rawValue)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .alert("Resolve Playlist Conflict?", isPresented: Binding(
                get: { confirmingGroup != nil },
                set: { if !$0 { confirmingGroup = nil } }
            ), presenting: confirmingGroup) { group in
                Button("Cancel", role: .cancel) { confirmingGroup = nil }
                Button("Resolve") {
                    Task {
                        await resolve(groupID: group.id, selectedSource: nil)
                        confirmingGroup = nil
                    }
                }
            } message: { group in
                Text("\(group.trailing.capitalized). Conflict copies are removed only after the canonical playlist is written.")
            }
            .alert("Conflict Error", isPresented: Binding(
                get: { errorMessage != nil },
                set: { if !$0 { errorMessage = nil } }
            )) {
                Button("OK") { errorMessage = nil }
            } message: {
                Text(errorMessage ?? "")
            }
            .sheet(isPresented: Binding(
                get: { choiceGroupID != nil },
                set: { if !$0 { choiceGroupID = nil } }
            )) {
                choiceSheet
            }
        }
    }

    private var choiceSheet: some View {
        NavigationStack {
            List(choiceRows, id: \.id) { row in
                Button {
                    selectedSource = row.id
                } label: {
                    HStack {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(row.title)
                            if let subtitle = row.subtitle {
                                Text(subtitle)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        }
                        Spacer()
                        if selectedSource == row.id {
                            Image(systemName: "checkmark")
                        }
                    }
                }
            }
            .navigationTitle("Choose Order")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { choiceGroupID = nil }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Resolve") {
                        guard let groupID = choiceGroupID else { return }
                        Task {
                            await resolve(groupID: groupID, selectedSource: selectedSource)
                            choiceGroupID = nil
                        }
                    }
                    .disabled(selectedSource == nil || isResolving)
                }
            }
        }
    }

    private func loadChoices(for group: ConflictRow) async {
        do {
            choiceRows = try await library.conflictChoices(groupID: group.id)
            selectedSource = nil
            choiceGroupID = group.id
        } catch {
            errorMessage = (error as? MusicError)?.displayMessage
                ?? error.localizedDescription
        }
    }

    private func resolve(groupID: String, selectedSource: String?) async {
        isResolving = true
        defer { isResolving = false }
        do {
            try await library.resolveConflict(
                groupID: groupID,
                selectedSource: selectedSource
            )
            if library.syncConflictGroups.isEmpty {
                dismiss()
            }
        } catch {
            errorMessage = (error as? MusicError)?.displayMessage
                ?? error.localizedDescription
        }
    }
}
