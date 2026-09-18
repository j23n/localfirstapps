import SwiftUI

struct FolderGridView: View {
    /// Fallback title when the live node is gone (deleted while this grid
    /// was on the stack). The live folder's name wins when the lookup hits.
    let title: String
    let folderID: UUID

    @Environment(GalleryStore.self) private var store

    private var listingEpoch: UInt64 { store.index.listingEpoch }

    private var photoIDs: [UUID] {
        _ = listingEpoch
        return store.index.folderPhotoIDs(folderID)
    }

    private var liveName: String? {
        _ = listingEpoch
        return store.index.folderHost(folderID.uuidString)?.name
    }

    private var folderKnown: Bool {
        _ = listingEpoch
        return store.index.folderExists(folderID.uuidString) || !photoIDs.isEmpty
    }

    var body: some View {
        if store.libraryAvailability == .unavailable {
            LibraryEmptyState.unavailable(icon: "folder") {
                Task { await store.rescan(kind: .light, silent: false) }
            }
            .navigationTitle(title)
        } else if !store.hasSortedPhotos {
            ProgressView()
                .navigationTitle(title)
        } else if folderKnown {
            PhotoGridScreen(
                title: liveName ?? title,
                subtitle: photoCountLabel(photoIDs.count),
                showSearch: true,
                fixedPhotoIDs: photoIDs
            )
        } else {
            ContentUnavailableView(
                "This folder is no longer available",
                systemImage: "folder"
            )
            .navigationTitle(title)
        }
    }
}
