import SwiftUI

// MARK: - Memory grid mode (reachable via "See all" from slideshow)

struct MemoryGridView: View {
    let memory: Memory

    var body: some View {
        PhotoGridScreen(
            title: memory.title,
            subtitle: memory.subtitle,
            playableMemory: memory,
            fixedPhotoIDs: memory.photoIDs
        )
    }
}

