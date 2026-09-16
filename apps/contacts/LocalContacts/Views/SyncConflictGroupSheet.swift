import SwiftUI

/// Syncthing `.vcf` group (ADR 0005 R8–R11). Not the Apple CN sheet.
struct SyncConflictGroupSheet: View {
    @Environment(ContactsStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var errorMessage: String?
    @State private var preview: ConflictPreview?
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
                        Text(group.subtitle)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Text(group.trailing)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Button("Review Diff…") {
                            showPreview(group.id)
                        }
                    }
                }
            }
            .navigationTitle("Sync Conflicts")
            .navigationBarTitleDisplayMode(.inline)
            .accessibilityIdentifier(ContactsScreen.syncConflictGroup.rawValue)
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
                get: { preview != nil },
                set: { if !$0 { preview = nil } }
            )) {
                if let preview {
                    previewSheet(preview)
                }
            }
        }
    }

    private func showPreview(_ name: String) {
        do {
            let loaded = try store.conflictPreview(canonicalName: name)
            picks = [:]
            if loaded.kind == .choice {
                for field in loaded.fields {
                    if let first = field.sides.first {
                        picks[field.field] = "\(field.field)|\(first.source)"
                    }
                }
            }
            preview = loaded
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func previewSheet(_ preview: ConflictPreview) -> some View {
        NavigationStack {
            List {
                Section {
                    Text(discardedCopy(preview.discarded))
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                if preview.fields.isEmpty {
                    Section("Merged Contact") {
                        ForEach(Array(preview.mergedFields.enumerated()), id: \.offset) { _, field in
                            LabeledContent(field.label, value: field.value)
                        }
                    }
                } else {
                    ForEach(preview.fields, id: \.field) { field in
                        Section(field.field) {
                            ForEach(field.sides, id: \.source) { side in
                                Button {
                                    picks[field.field] = "\(field.field)|\(side.source)"
                                } label: {
                                    HStack {
                                        VStack(alignment: .leading) {
                                            Text(side.source)
                                            Text(side.value)
                                                .font(.caption)
                                                .foregroundStyle(.secondary)
                                        }
                                        Spacer()
                                        if picks[field.field] == "\(field.field)|\(side.source)" {
                                            Image(systemName: "checkmark")
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            .navigationTitle("Sync Conflict")
            .navigationBarTitleDisplayMode(.inline)
            .accessibilityIdentifier(ContactsScreen.syncConflictGroup.rawValue)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Keep This Version") {
                        Task {
                            let choices = preview.kind == .choice ? Array(picks.values) : []
                            if await resolve(preview.id, choices: choices) {
                                self.preview = nil
                            }
                        }
                    }
                    .disabled(preview.kind == .choice && picks.count != preview.fields.count)
                }
            }
        }
    }

    private func discardedCopy(_ discarded: [String]) -> String {
        if discarded.isEmpty {
            return "No copies will be deleted."
        }
        return "Confirming deletes: \(discarded.joined(separator: ", "))."
    }

    @discardableResult
    private func resolve(_ name: String, choices: [String]) async -> Bool {
        do {
            try await store.resolveSyncGroup(canonicalName: name, choiceIds: choices)
            if store.syncConflictGroups.isEmpty {
                dismiss()
            }
            return true
        } catch {
            errorMessage = error.localizedDescription
            return false
        }
    }
}
