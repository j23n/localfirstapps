import SwiftUI

struct FolderBrowserView: View {
    @Environment(GalleryStore.self) private var store
    @Environment(AppRouter.self) private var router
    var folder: PhotoFolder? = nil
    /// Only the root tab should show the gear. Child browsers leave it off.
    var isRoot: Bool = true

    @State private var showSettings = false

    /// Destination identity only — listing comes from folder windows.
    private var folderID: String? { folder?.id.uuidString }

    private var listingEpoch: UInt64 { store.index.listingEpoch }

    private var childRows: [GalleryTextRow] {
        _ = listingEpoch
        return store.index.sortedFolderRows(
            store.index.folderListing(parentID: folderID),
            order: store.folderSortOrder
        )
    }

    /// Own photos for this screen: the scan root at the tab root, else this node.
    private var photoFolderID: UUID? {
        _ = listingEpoch
        if let folder { return folder.id }
        return store.index.scanRootFolderID()
    }

    private var ownPhotoIDs: [UUID] {
        guard let photoFolderID else { return [] }
        return store.index.folderPhotoIDs(photoFolderID)
    }

    private var folderStillExists: Bool {
        guard let folderID else { return store.rootFolder != nil }
        return store.index.folderExists(folderID)
    }

    var body: some View {
        @Bindable var store = store
        Group {
            if folder != nil {
                childBody
            } else {
                rootBody
            }
        }
        .navigationTitle(folder?.name ?? store.rootFolder?.name ?? (isRoot ? "Folders" : ""))
        .navigationBarTitleDisplayMode(isRoot ? .large : .inline)
        .toolbar {
            // Always include the banner — it returns EmptyView when no
            // scan is running. The `if store.scanProgress != nil` used to
            // live here, but reading scanProgress in the parent body made
            // every progress tick re-evaluate FolderBrowserView and the
            // List of folder rows below it. Scoping the read to
            // ScanProgressBanner.body keeps the parent untouched.
            ToolbarItem(placement: .principal) {
                ScanProgressBanner()
            }
            ToolbarItemGroup(placement: .topBarTrailing) {
                if !childRows.isEmpty {
                    Menu {
                        Picker("Sort Folders", selection: $store.folderSortOrder) {
                            ForEach(FolderSortOrder.allCases, id: \.self) { order in
                                Text(order.label).tag(order)
                            }
                        }
                    } label: {
                        Image(systemName: "arrow.up.arrow.down")
                    }
                }
                if isRoot {
                    SettingsToolbarButton(isPresented: $showSettings)
                }
            }
        }
        .sheet(isPresented: $showSettings) { SettingsView() }
        .onChange(of: store.hasSortedPhotos) { _, ready in
            if ready { router.consumePendingIfReady(store: store) }
        }
    }

    /// Root tab only. Unavailable takes the whole screen — the cached tree
    /// may still be in memory after an unlistable root, and showing it would
    /// look like the library was still there. The cold-launch spinner is
    /// `isScanning && displayFolder == nil` so a Reload Library pass does
    /// not hide a tree we already have.
    @ViewBuilder
    private var rootBody: some View {
        if store.libraryAvailability == .unavailable {
            unavailableState
        } else if store.isScanning && store.rootFolder == nil {
            ProgressView("Scanning folder…")
        } else if !store.hasSortedPhotos && store.rootFolder != nil {
            ProgressView("Scanning folder…")
        } else if store.rootFolder != nil || store.hasSortedPhotos {
            folderContent
        } else if store.libraryAvailability == .empty {
            emptyLibraryState
        } else {
            emptyState
        }
    }

    @ViewBuilder
    private var childBody: some View {
        // Unlistable root keeps `rootFolder` in memory so lookup would still
        // hit and show a ghost tree. The root tab already hid that; a pushed
        // child has to as well.
        if store.libraryAvailability == .unavailable {
            unavailableState
        } else if !store.hasSortedPhotos {
            ProgressView("Scanning folder…")
        } else if folderStillExists {
            folderContent
        } else {
            ContentUnavailableView(
                "This folder is no longer available",
                systemImage: "folder"
            )
        }
    }

    private var emptyState: some View {
        LibraryEmptyState(icon: "folder", title: "No Folder Selected",
                          message: "Set a folder in Settings to get started.")
    }

    private var emptyLibraryState: some View {
        LibraryEmptyState.selectedFolderEmpty(icon: "folder", title: "No Photos")
    }

    private var unavailableState: some View {
        LibraryEmptyState.unavailable(icon: "folder") {
            Task { await store.rescan(kind: .light, silent: false) }
        }
    }

    @ViewBuilder
    private var folderContent: some View {
        let rows = childRows
        let photos = ownPhotoIDs
        List {
            if !photos.isEmpty, let photoFolderID {
                Section("Photos") {
                    NavigationLink {
                        FolderGridView(
                            title: folder?.name ?? store.rootFolder?.name ?? "Photos",
                            folderID: photoFolderID
                        )
                    } label: {
                        Label("\(photos.count) photos in this folder", systemImage: "photo.on.rectangle")
                    }
                }
            }

            if !rows.isEmpty {
                Section("Subfolders") {
                    ForEach(rows, id: \.id) { row in
                        folderLink(row)
                    }
                }
            }

            if photos.isEmpty && rows.isEmpty {
                ContentUnavailableView(
                    "No Photos Found",
                    systemImage: "photo",
                    description: Text("This folder doesn't contain any images.")
                )
            }
        }
        .softTopScrollEdge()
        .refreshable {
            await store.rescan(kind: .light)
        }
    }

    @ViewBuilder
    private func folderLink(_ row: GalleryTextRow) -> some View {
        let hasChildren = store.index.folderHasChildren(row.id)
        let photoIDs = store.index.folderPhotoIDs(folderID: row.id)
        return NavigationLink {
            if !hasChildren, !photoIDs.isEmpty, let uuid = UUID(uuidString: row.id) {
                FolderGridView(title: row.title, folderID: uuid)
            } else if let dest = store.index.folderDestination(id: row.id) {
                FolderBrowserView(folder: dest, isRoot: false)
            }
        } label: {
            folderRow(row)
        }
    }

    private func folderRow(_ row: GalleryTextRow) -> some View {
        HStack(spacing: 14) {
            if let coverURL = store.index.folderCoverURL(row.id) {
                ThumbnailView(url: coverURL, size: 72, cornerRadius: 8)
            } else {
                RoundedRectangle(cornerRadius: 8)
                    .fill(Color(.systemGray5))
                    .frame(width: 72, height: 72)
                    .overlay {
                        Image(systemName: "folder.fill")
                            .font(.title2)
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
            Spacer()
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }
}
