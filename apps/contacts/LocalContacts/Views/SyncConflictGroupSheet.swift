import SwiftUI

/// Syncthing `.vcf` group (ADR 0005 R8–R11). Not the Apple CN sheet.
struct SyncConflictGroupSheet: View {
    @Environment(ContactsStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var errorMessage: String?
    @State private var picking: String?
    @State private var choiceRows: [TextRow] = []
    @State private var picks: [String: String] = [:]

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Text("Two devices edited the same file. Disjoint fields merge automatically. The same field is a choice. Losing copies are deleted only after you confirm.")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                ForEach(store.syncConflictGroups, id: \.id) { group in
                    Section(group.title) {
                        Text(group.trailing ?? "")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        if group.trailing == "needs choice" {
                            Button("Choose fields") {
                                showChoices(group.id)
                            }
                        } else {
                            Button("Resolve") {
                                Task { await resolve(group.id, choices: []) }
                            }
                        }
                    }
                }
            }
            .navigationTitle("Sync Conflicts")
            .navigationBarTitleDisplayMode(.inline)
            .accessibilityIdentifier("sync-conflict-group")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .alert("Error", isPresented: .init(
                get: { errorMessage != nil },
                set: { if !$0 { errorMessage = nil } }
            )) {
                Button("OK") { errorMessage = nil }
            } message: {
                Text(errorMessage ?? "")
            }
            .sheet(isPresented: Binding(
                get: { picking != nil },
                set: { if !$0 { picking = nil } }
            )) {
                if let name = picking {
                    choiceSheet(name)
                }
            }
        }
    }

    private func showChoices(_ name: String) {
        do {
            choiceRows = try store.syncConflictChoiceRows(canonicalName: name)
            picks = [:]
            picking = name
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func choiceSheet(_ name: String) -> some View {
        let fields = Dictionary(grouping: choiceRows, by: \.title)
        return NavigationStack {
            List {
                ForEach(fields.keys.sorted(), id: \.self) { field in
                    Section(field) {
                        ForEach(fields[field] ?? [], id: \.id) { row in
                            Button {
                                picks[field] = row.id
                            } label: {
                                HStack {
                                    VStack(alignment: .leading) {
                                        Text(row.subtitle ?? "")
                                        Text(row.trailing ?? "")
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                    }
                                    Spacer()
                                    if picks[field] == row.id {
                                        Image(systemName: "checkmark")
                                    }
                                }
                            }
                        }
                    }
                }
            }
            .navigationTitle("Choose")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Resolve") {
                        Task {
                            await resolve(name, choices: Array(picks.values))
                            picking = nil
                        }
                    }
                    .disabled(picks.count != fields.count)
                }
            }
        }
    }

    private func resolve(_ name: String, choices: [String]) async {
        do {
            try await store.resolveSyncGroup(canonicalName: name, choiceIds: choices)
            if store.syncConflictGroups.isEmpty {
                dismiss()
            }
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}
