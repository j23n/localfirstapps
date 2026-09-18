import SwiftUI

/// Syncthing `.xmp` / image group (ADR 0005 R8–R11). Not an Apple CN sheet.
struct SyncConflictGroupSheet: View {
    @Environment(GalleryStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var errorMessage: String?
    @State private var xmpPreview: ConflictPreview?
    @State private var imagePreview: ImageConflictPreview?
    @State private var keepName: String?

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Text("Two devices wrote the same photo. Sidecar keywords merge automatically. Image copies are keep-one — pick the file that stays. Losing copies are deleted only after you confirm.")
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
                        Button(group.kind == .image ? "Choose Copy…" : "Review Diff…") {
                            showPreview(group)
                        }
                    }
                }
            }
            .navigationTitle("Sync Conflicts")
            .navigationBarTitleDisplayMode(.inline)
            .accessibilityIdentifier(GalleryScreen.syncConflictGroup.rawValue)
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
                get: { xmpPreview != nil },
                set: { if !$0 { xmpPreview = nil } }
            )) {
                if let xmpPreview {
                    xmpPreviewSheet(xmpPreview)
                }
            }
            .sheet(isPresented: Binding(
                get: { imagePreview != nil },
                set: { if !$0 { imagePreview = nil; keepName = nil } }
            )) {
                if let imagePreview {
                    imagePreviewSheet(imagePreview)
                }
            }
            .onAppear { store.refreshSyncConflicts() }
        }
    }

    private func showPreview(_ group: ConflictRow) {
        do {
            if group.kind == .image {
                let loaded = try store.imagePreview(groupId: group.id)
                keepName = loaded.copies.first
                imagePreview = loaded
            } else {
                xmpPreview = try store.conflictPreview(groupId: group.id)
            }
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func xmpPreviewSheet(_ preview: ConflictPreview) -> some View {
        NavigationStack {
            List {
                Section {
                    Text(discardedCopy(preview.discarded))
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                Section("Merged Sidecar") {
                    ForEach(Array(preview.mergedFields.enumerated()), id: \.offset) { _, field in
                        LabeledContent(field.label, value: field.value)
                    }
                }
            }
            .navigationTitle("Sync Conflict")
            .navigationBarTitleDisplayMode(.inline)
            .accessibilityIdentifier(GalleryScreen.syncConflictGroup.rawValue)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Keep This Version") {
                        Task {
                            if await resolveXmp(preview.id) {
                                xmpPreview = nil
                            }
                        }
                    }
                }
            }
        }
    }

    private func imagePreviewSheet(_ preview: ImageConflictPreview) -> some View {
        NavigationStack {
            List {
                Section {
                    Text("Confirming deletes the copies you do not keep. Image bytes are not rewritten.")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                Section("Keep One Copy") {
                    ForEach(preview.copies, id: \.self) { name in
                        Button {
                            keepName = name
                        } label: {
                            HStack {
                                Text(name)
                                Spacer()
                                if keepName == name {
                                    Image(systemName: "checkmark")
                                }
                            }
                        }
                    }
                }
            }
            .navigationTitle("Sync Conflict")
            .navigationBarTitleDisplayMode(.inline)
            .accessibilityIdentifier(GalleryScreen.syncConflictGroup.rawValue)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Keep This Copy") {
                        Task {
                            guard let keepName else { return }
                            if await keepImage(preview.id, surviving: keepName) {
                                imagePreview = nil
                                self.keepName = nil
                            }
                        }
                    }
                    .disabled(keepName == nil)
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
    private func resolveXmp(_ groupId: String) async -> Bool {
        do {
            try await store.resolveSyncGroup(groupId: groupId)
            if store.syncConflictGroups.isEmpty {
                dismiss()
            }
            return true
        } catch {
            errorMessage = error.localizedDescription
            return false
        }
    }

    @discardableResult
    private func keepImage(_ groupId: String, surviving: String) async -> Bool {
        do {
            try await store.keepImageCopy(groupId: groupId, surviving: surviving)
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
