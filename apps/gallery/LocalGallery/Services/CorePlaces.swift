import Foundation
import Observation

/// Main-actor state around the Rust `gallery_session::run_places` entry point.
///
/// Eligibility, sidecar authority, cache radius, lookup, retry, and write
/// policy all live in Rust. This type only crosses the FFI off the main actor
/// and publishes progress in the shape the app already displays.
@Observable
@MainActor
final class CorePlaces {
    struct Progress: Equatable, Sendable {
        var done: Int
        var total: Int
        var startedAt: Date

        var countText: String {
            ProgressETA.countText(processed: done, total: total, startedAt: startedAt)
        }
    }

    struct Summary: Equatable, Sendable {
        var processed = 0
        var written = 0
        var skipped = 0
        var failed = 0
        var cancelled = false
    }

    private(set) var isRunning = false
    private(set) var progress: Progress?
    private(set) var lastSummary: Summary?
    private(set) var lastError: String?

    @ObservationIgnored private let cacheURL: URL
    @ObservationIgnored private let session = PlacesSession()
    @ObservationIgnored var onPlaceRecorded: (@MainActor (URL, String?, ScanActivityEntry.Outcome) -> Void)?

    init(cacheURL: URL) {
        self.cacheURL = cacheURL
    }

    nonisolated static func isEligible(_ photo: PhotoFile) -> Bool {
        placesCandidate(photo: CoreScanner.record(of: photo))
    }

    func geocode(_ photos: [PhotoFile], force: Bool = false) async -> Summary {
        guard !isRunning else { return lastSummary ?? Summary() }
        isRunning = true
        lastError = nil
        let startedAt = Date()
        progress = Progress(done: 0, total: 0, startedAt: startedAt)

        let session = self.session
        session.prepare()
        let cachePath = cacheURL.path
        let bridge = CorePlacesProgressBridge { [weak self] done, total in
            Task { @MainActor [weak self] in
                guard let self, self.isRunning else { return }
                self.progress = Progress(done: done, total: total, startedAt: startedAt)
            }
        }
        let result = await withTaskCancellationHandler {
            await Task.detached(priority: .utility) {
                session.run(
                    photos: photos.map(CoreScanner.record(of:)),
                    cachePath: cachePath,
                    force: force,
                    progress: bridge
                )
            }.value
        } onCancel: {
            session.cancel()
        }

        for record in result.records {
            let outcome: ScanActivityEntry.Outcome
            switch record.outcome {
            case .written:
                outcome = .written
            case .skipped:
                outcome = .skipped
            case .failed(let detail):
                outcome = .failed(detail)
            }
            onPlaceRecorded?(
                CoreScanner.fileURL(record.imagePath),
                record.placePath,
                outcome
            )
        }

        let summary = Summary(
            processed: Int(result.processed),
            written: Int(result.written),
            skipped: Int(result.skipped),
            failed: Int(result.failed),
            cancelled: result.cancelled
        )
        lastSummary = summary
        lastError = result.error
        progress = nil
        isRunning = false
        return summary
    }

    func cancel() {
        session.cancel()
    }
}

private final class CorePlacesProgressBridge: PlacesProgressListener, Sendable {
    private let handler: @Sendable (Int, Int) -> Void

    init(_ handler: @escaping @Sendable (Int, Int) -> Void) {
        self.handler = handler
    }

    func onProgress(done: UInt32, total: UInt32) {
        handler(Int(done), Int(total))
    }
}
