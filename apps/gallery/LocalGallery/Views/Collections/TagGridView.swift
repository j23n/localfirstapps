import SwiftUI

// MARK: - Tag Grid View

struct TagGridView: View {
    let tag: TagSuggestion
    @Environment(GalleryStore.self) private var store

    private var isPersonTag: Bool {
        tag.namespace?.lowercased() == "people"
    }

    var body: some View {
        PhotoGridScreen(
            title: tag.displayName,
            subtitle: tag.fullPath.replacingOccurrences(of: "/", with: " › "),
            photos: store.sortedPhotos,
            usesWindowedLibrary: true,
            featureContextPerson: isPersonTag ? tag : nil,
            fixedTags: [tag]
        )
    }
}

