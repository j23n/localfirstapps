import SwiftUI

/// Destination picker for an on-disk move. Lists children through folder
/// windows (the same listing the Folders tab shows) and can create a
/// subdirectory. Confirming "Move Here" is what commits the move.
struct FolderMovePicker: View {
    let photos: [PhotoFile]
    var onMove: (PhotoFolder) -> Void

    @Environment(GalleryStore.self) private var store
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            if store.rootFolder != nil {
                FolderMoveList(parentID: nil, photos: photos) { dest in
                    onMove(dest)
                    dismiss()
                }
            } else {
                ContentUnavailableView(
                    "No Folder Selected",
                    systemImage: "folder",
                    description: Text("Set a library folder in Settings first.")
                )
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Cancel") { dismiss() }
                    }
                }
            }
        }
    }
}

private struct FolderMoveList: View {
    let parentID: String?
    let photos: [PhotoFile]
    var onMove: (PhotoFolder) -> Void

    @Environment(GalleryStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var showNewFolder = false
    @State private var newFolderName = ""
    @State private var pendingDest: PhotoFolder?

    /// Filesystem destination for create / Move Here. Listing does not use this tree.
    private var destination: PhotoFolder? {
        if let parentID, let uuid = UUID(uuidString: parentID) {
            return store.rootFolder?.folder(withID: uuid)
        }
        return store.rootFolder
    }

    private var rows: [GalleryTextRow] {
        _ = store.index.listingEpoch
        return store.index.sortedFolderRows(
            store.index.folderListing(parentID: parentID),
            order: store.folderSortOrder
        )
    }

    private var alreadyHere: Bool {
        guard let dest = destination else { return true }
        return photos.allSatisfy { photo in
            !GalleryStore.pathKeys(for: photo.url.deletingLastPathComponent())
                .isDisjoint(with: GalleryStore.pathKeys(for: dest.url))
        }
    }

    var body: some View {
        List {
            let subs = rows
            if !subs.isEmpty {
                Section("Subfolders") {
                    ForEach(subs, id: \.id) { row in
                        NavigationLink {
                            FolderMoveList(parentID: row.id, photos: photos, onMove: onMove)
                        } label: {
                            folderRow(row)
                        }
                    }
                }
            } else {
                Section {
                    Text("No subfolders")
                        .foregroundStyle(Design.ink3)
                }
            }
        }
        .background(Design.bg)
        .navigationTitle(destination?.name ?? store.index.folderHost(parentID ?? "")?.name ?? "Folders")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button("Cancel") { dismiss() }
            }
            ToolbarItem(placement: .primaryAction) {
                Button("Move Here") {
                    pendingDest = destination
                }
                .fontWeight(.semibold)
                .disabled(alreadyHere || photos.isEmpty || destination == nil)
            }
            ToolbarItem(placement: .bottomBar) {
                Button {
                    newFolderName = ""
                    showNewFolder = true
                } label: {
                    Label("New Folder", systemImage: "folder.badge.plus")
                }
                .disabled(destination == nil)
            }
        }
        .alert(PhotoMovePrompt.title(for: photos), isPresented: pendingDestBinding) {
            Button("Cancel", role: .cancel) { pendingDest = nil }
            Button("Move") {
                if let dest = pendingDest {
                    pendingDest = nil
                    onMove(dest)
                }
            }
        } message: {
            if let dest = pendingDest {
                Text(PhotoMovePrompt.message(for: photos, destination: dest))
            }
        }
        .alert("New Folder", isPresented: $showNewFolder) {
            TextField("Name", text: $newFolderName)
            Button("Cancel", role: .cancel) { newFolderName = "" }
            Button("Create") {
                if let dest = destination,
                   store.createFolder(named: newFolderName, in: dest) != nil,
                   let root = store.rootFolder {
                    store.index.attachFolders(from: root)
                }
                newFolderName = ""
            }
            .disabled(newFolderName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        } message: {
            Text("Created inside “\(destination?.name ?? "")”.")
        }
    }

    private var pendingDestBinding: Binding<Bool> {
        Binding(
            get: { pendingDest != nil },
            set: { if !$0 { pendingDest = nil } }
        )
    }

    private func folderRow(_ row: GalleryTextRow) -> some View {
        HStack(spacing: 14) {
            if let coverURL = store.index.folderCoverURL(row.id) {
                ThumbnailView(url: coverURL, size: 56, cornerRadius: 8)
            } else {
                RoundedRectangle(cornerRadius: 8)
                    .fill(Color(.systemGray5))
                    .frame(width: 56, height: 56)
                    .overlay {
                        Image(systemName: "folder.fill")
                            .font(.title3)
                            .foregroundStyle(.tertiary)
                    }
            }
            VStack(alignment: .leading, spacing: 4) {
                Text(row.title)
                    .font(.body)
                    .fontWeight(.medium)
                    .lineLimit(1)
                Text(row.trailing ?? photoCountLabel(0))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 2)
    }
}
