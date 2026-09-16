import SwiftUI

// MARK: - Tag Grid View

struct TagGridView: View {
    let tag: TagSuggestion

    private var isPersonTag: Bool {
        tag.namespace?.lowercased() == "people"
    }

    var body: some View {
        PhotoGridScreen(
            title: tag.displayName,
            subtitle: tag.fullPath.replacingOccurrences(of: "/", with: " › "),
            featureContextPerson: isPersonTag ? tag : nil,
            fixedTags: [tag]
        )
    }
}

