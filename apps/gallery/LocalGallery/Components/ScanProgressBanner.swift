import SwiftUI

/// Compact, single-line scan-progress chip for the `.principal` toolbar slot
/// of the three main tabs (All Photos, Collections, Folders). Hidden until
/// the job has lasted 500 ms (ADR 0007 R5).
///
/// Same chip as Settings; the only tab-specific bit is the leading phase
/// word (Scanning / Tagging / Faces / Places).
struct ScanProgressBanner: View {
    @Environment(GalleryStore.self) private var store
    @State private var revealed = false

    var body: some View {
        Group {
            if revealed {
                if let progress = store.scanProgress {
                    ScanProgressChip(phase: progress.shortLabel, detail: progress.countText)
                        .foregroundStyle(Design.ink)
                } else if let progress = store.analysis.progress {
                    ScanProgressChip(phase: progress.shortLabel, detail: progress.countText)
                        .foregroundStyle(Design.ink)
                }
            }
        }
        .task(id: runningToken) {
            await armReveal()
        }
    }

    private var runningToken: String {
        if let progress = store.scanProgress {
            return "scan-\(progress.startedAt.timeIntervalSinceReferenceDate)"
        }
        if let progress = store.analysis.progress {
            return "analysis-\(progress.startedAt.timeIntervalSinceReferenceDate)"
        }
        return "idle"
    }

    private func armReveal() async {
        let started = store.scanProgress?.startedAt ?? store.analysis.progress?.startedAt
        guard let started else {
            revealed = false
            return
        }
        let remaining = 0.5 - Date().timeIntervalSince(started)
        if remaining > 0 {
            revealed = false
            try? await Task.sleep(for: .seconds(remaining))
        }
        revealed = store.scanProgress != nil || store.analysis.progress != nil
    }
}
