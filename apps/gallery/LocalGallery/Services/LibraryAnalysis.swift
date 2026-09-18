import Foundation
import Observation
import os

/// Host adapter for one Scan Photos run.
///
/// The three engines run inside UniFFI `AnalysisSession` →
/// `gallery_session::run_analysis`. This type keeps ImageIO HEIC, the
/// Scan Activity journal, Settings phase chrome, and sidecar-refresh hooks.
@Observable
@MainActor
final class LibraryAnalysis {
    enum Phase: Equatable, Sendable {
        case tagging
        case faces
        case places
    }

    /// Combined progress across the three phases. The chip uses `phase` plus
    /// this phase's work queue (`done` / `total`), not the whole library.
    struct Progress: Equatable, Sendable {
        var phase: Phase
        var done: Int
        var total: Int
        var overallDone: Int
        var overallTotal: Int
        var startedAt: Date

        var label: String {
            switch phase {
            case .tagging: return "Tagging…"
            case .faces: return "Finding faces…"
            case .places: return "Looking up places…"
            }
        }

        /// One word for the shared progress chip.
        var shortLabel: String {
            switch phase {
            case .tagging: return "Tagging"
            case .faces: return "Faces"
            case .places: return "Places"
            }
        }

        /// "X / Y · ~M:SS" for the current phase's work queue — photos that
        /// still need this pass, not the whole library. `nil` while the
        /// engine is still counting that queue.
        var countText: String? {
            guard total > 0 else { return nil }
            return ProgressETA.countText(
                processed: done,
                total: total,
                startedAt: startedAt
            )
        }

        /// Whole-library fraction, not the current phase. Tagging 16 of
        /// 18,220 while faces and places are still queued is a few percent
        /// of the run, not 16 / 18,220 of "this scan".
        var percentText: String {
            guard overallTotal > 0 else { return "0%" }
            let pct = Int((Double(overallDone) / Double(overallTotal) * 100).rounded(.towardZero))
            return "\(min(pct, 100))%"
        }
    }

    struct Summary: Equatable, Sendable {
        var tagging: TaggingService.Summary?
        var faces: FaceService.Summary?
        var places: CorePlaces.Summary?
        var cancelled = false
    }

    private(set) var isRunning = false
    /// Phases this `start` asked for. The Settings rows use this so a full
    /// Scan Photos does not light up the single-phase force buttons.
    private(set) var activePhases: Set<Phase> = []
    private(set) var progress: Progress?
    private(set) var lastSummary: Summary?
    private(set) var lastError: String?
    /// Live journal of this run (and the last finished one). Settings can
    /// open it while tagging is still writing.
    let activity = ScanActivityLog()

    @ObservationIgnored private let tagging: TaggingService
    @ObservationIgnored private let faces: FaceService
    @ObservationIgnored private let mlCacheURL: URL
    @ObservationIgnored private let geoCacheURL: URL
    @ObservationIgnored private var session: AnalysisSession?
    @ObservationIgnored var photos: (@MainActor () -> [PhotoFile])?
    /// Light rescan after the run, so Places tags (and any tagging/face
    /// sidecars whose coalesced refresh hasn't fired yet) land in the index.
    @ObservationIgnored var onSidecarsWritten: (@MainActor () async -> Void)?
    /// One Places sidecar just landed. `GalleryStore` wires this to the
    /// shared 30 s coalescer — same cooldown tagging and faces already use.
    @ObservationIgnored var onPlaceWritten: (@MainActor () -> Void)?
    /// Sidecar bytes just landed (or were re-read). The Store unions them
    /// onto the live `PhotoFile` — scans will not, because the image file
    /// itself did not change.
    @ObservationIgnored var onSidecarPaths: (@MainActor ([String]) -> Void)?
    /// Store arms the 500 ms chrome reveal when a run begins or ends.
    @ObservationIgnored var onChromeProgress: (@MainActor () -> Void)?

    init(
        tagging: TaggingService,
        faces: FaceService,
        places _: CorePlaces,
        mlCacheURL: URL,
        geoCacheURL: URL
    ) {
        self.tagging = tagging
        self.faces = faces
        self.mlCacheURL = mlCacheURL
        self.geoCacheURL = geoCacheURL
    }

    func start() async {
        await start(phases: [.tagging, .faces, .places], force: false)
    }

    /// Force one photo through `phases`. Does not reset the library queues.
    func startOne(_ photo: PhotoFile, phases: Set<Phase>) async {
        guard !isRunning else { return }
        await tagging.refreshAvailability()
        let tag = phases.contains(.tagging) && tagging.isAvailable && TaggingService.isEligible(photo)
        let face = phases.contains(.faces) && faces.isAvailable && FaceService.isEligible(photo)
        let place = phases.contains(.places) && CorePlaces.isEligible(photo)
        guard tag || face || place else { return }

        isRunning = true
        activePhases = phases
        lastError = nil
        lastSummary = nil
        activity.beginRun()
        let startedAt = Date()
        publish(
            firstPhase(tag: tag, face: face, place: place),
            done: 0,
            total: 1,
            overallDone: 0,
            overallTotal: 1,
            startedAt: startedAt
        )
        await faces.unloadSession()
        let run = await runSession(
            photos: [photo],
            phases: AnalysisPhases(tagging: tag, faces: face, places: place),
            force: true,
            one: true,
            startedAt: startedAt
        )
        await end(hostSummary(run))
    }

    /// Run `phases` in the usual order. `force` resets that phase's skip
    /// (queue + sidecar version for tagging, `face_scans` for faces, an
    /// existing `Places/…` city for geocoding) and walks the whole library.
    func start(phases: Set<Phase>, force: Bool) async {
        guard !isRunning else { return }
        await tagging.refreshAvailability()
        let all = photos?() ?? []
        let wantTag = phases.contains(.tagging)
        let wantFace = phases.contains(.faces)
        let wantPlace = phases.contains(.places)

        isRunning = true
        activePhases = phases
        lastError = nil
        lastSummary = nil
        let startedAt = Date()
        publish(
            firstPhase(tag: wantTag && tagging.isAvailable, face: wantFace && faces.isAvailable, place: wantPlace),
            done: 0,
            total: 0,
            overallDone: 0,
            overallTotal: 0,
            startedAt: startedAt
        )
        Log.ml.info(
            "Scan start library=\(all.count) tag=\(wantTag) face=\(wantFace) place=\(wantPlace) force=\(force)"
        )

        if all.isEmpty {
            lastSummary = Summary()
            progress = nil
            defer {
                isRunning = false
                activePhases = []
                session = nil
            }
            await faces.refreshClusters()
            return
        }

        activity.beginRun()
        await faces.unloadSession()
        let run = await runSession(
            photos: all,
            phases: AnalysisPhases(tagging: wantTag, faces: wantFace, places: wantPlace),
            force: force,
            one: false,
            startedAt: startedAt
        )
        await end(hostSummary(run))
    }

    func cancel() {
        guard isRunning else { return }
        session?.cancel()
    }

    private func runSession(
        photos: [PhotoFile],
        phases: AnalysisPhases,
        force: Bool,
        one: Bool,
        startedAt: Date
    ) async -> AnalysisRunSummary {
        let session = AnalysisSession()
        self.session = session
        let packDir = tagging.pack?.directory.path
        let mlCache = mlCacheURL.path
        let geoCache = geoCacheURL.path
        return await withCheckedContinuation { (cont: CheckedContinuation<AnalysisRunSummary, Never>) in
            let bridge = AnalysisProgressBridge { phase, done, total in
                Task { @MainActor [weak self] in
                    guard let self, self.isRunning else { return }
                    self.publish(
                        Self.hostPhase(phase),
                        done: Int(done),
                        total: Int(total),
                        overallDone: Int(done),
                        overallTotal: Int(total),
                        startedAt: startedAt
                    )
                }
            } onFinished: { summary in
                cont.resume(returning: summary)
            }
            do {
                if one, let photo = photos.first {
                    try session.startOne(
                        photo: CoreScanner.record(of: photo),
                        packDir: packDir,
                        mlCachePath: mlCache,
                        geoCachePath: geoCache,
                        phases: phases,
                        decoder: ImageIOHeicDecoder.shared,
                        progress: bridge
                    )
                } else {
                    try session.start(
                        photos: photos.map(CoreScanner.record(of:)),
                        packDir: packDir,
                        mlCachePath: mlCache,
                        geoCachePath: geoCache,
                        phases: phases,
                        force: force,
                        decoder: ImageIOHeicDecoder.shared,
                        progress: bridge
                    )
                }
            } catch {
                cont.resume(returning: AnalysisRunSummary(
                    photos: UInt32(photos.count),
                    tagged: nil,
                    faces: nil,
                    places: nil,
                    writtenPaths: [],
                    cancelled: false,
                    error: String(describing: error),
                    skippedMl: nil
                ))
            }
        }
    }

    private func end(_ summary: Summary) async {
        // Stay `isRunning` through the cluster refresh and the final light
        // rescan this run owes, so `LibraryRootMonitor` keeps ignoring the
        // sidecar writes that just landed. Flipping the flag first is what
        // let a queued vnode hop start another walk the moment we finished.
        // `defer` so a cancellation mid-refresh cannot leave the flag stuck.
        finish(summary)
        defer {
            isRunning = false
            activePhases = []
            session = nil
        }
        // A tagging-only or places-only run never hits FaceService.finish(),
        // which is otherwise the publisher for `allClusters`. The face phase
        // already refreshed and dropped its session, so this reopens briefly.
        await faces.refreshClusters()
        await faces.unloadSession()
        await onSidecarsWritten?()
    }

    private func finish(_ summary: Summary) {
        progress = nil
        lastSummary = summary
        onChromeProgress?()
        Log.ml.info(
            "Library analysis finished cancelled=\(summary.cancelled) tagging=\(summary.tagging?.processed ?? 0) faces=\(summary.faces?.processed ?? 0) places=\(summary.places?.processed ?? 0)"
        )
    }

    private func hostSummary(_ run: AnalysisRunSummary) -> Summary {
        lastError = run.error
        journal(run)
        if !run.writtenPaths.isEmpty {
            onSidecarPaths?(run.writtenPaths)
        }
        if let places = run.places, places.written > 0 {
            onPlaceWritten?()
        }
        var summary = Summary(cancelled: run.cancelled)
        if let tagged = run.tagged {
            summary.tagging = TaggingService.Summary(
                processed: Int(tagged),
                tagged: Int(tagged),
                sidecarsWritten: Int(tagged),
                cancelled: run.cancelled
            )
        }
        if let faces = run.faces {
            summary.faces = FaceService.Summary(
                processed: Int(faces),
                facesFound: Int(faces),
                sidecarsWritten: Int(faces),
                cancelled: run.cancelled
            )
        }
        if let places = run.places {
            summary.places = CorePlaces.Summary(
                processed: Int(places.processed),
                written: Int(places.written),
                skipped: Int(places.skipped),
                failed: Int(places.failed),
                cancelled: places.cancelled
            )
        }
        return summary
    }

    private func journal(_ run: AnalysisRunSummary) {
        if let places = run.places {
            for record in places.records {
                let outcome: ScanActivityEntry.Outcome
                switch record.outcome {
                case .written:
                    outcome = .written
                case .skipped:
                    outcome = .skipped
                case .failed(let detail):
                    outcome = .failed(detail)
                }
                activity.record(.place(
                    url: CoreScanner.fileURL(record.imagePath),
                    path: record.placePath,
                    outcome: outcome
                ))
            }
        }
        var leftover = run.writtenPaths
        if let places = run.places {
            let placed = Set(places.writtenPaths)
            leftover.removeAll { placed.contains($0) }
        }
        if !leftover.isEmpty {
            let phase: ScanActivityEntry.Phase = run.tagged != nil ? .tagging : .faces
            activity.scheduleIngest(paths: leftover, phase: phase)
        }
    }

    private func firstPhase(tag: Bool, face: Bool, place: Bool) -> Phase {
        if tag { return .tagging }
        if face { return .faces }
        return .places
    }

    private static func hostPhase(_ phase: AnalysisPhase) -> Phase {
        switch phase {
        case .tagging: return .tagging
        case .faces: return .faces
        case .places: return .places
        }
    }

    private func publish(
        _ phase: Phase,
        done: Int,
        total: Int,
        overallDone: Int,
        overallTotal: Int,
        startedAt: Date
    ) {
        progress = Progress(
            phase: phase,
            done: done,
            total: total,
            overallDone: overallDone,
            overallTotal: overallTotal,
            startedAt: startedAt
        )
        onChromeProgress?()
    }
}

private final class AnalysisProgressBridge: AnalysisProgressListener, @unchecked Sendable {
    private let onProgressHop: @Sendable (AnalysisPhase, UInt32, UInt32) -> Void
    private let onFinishedHop: @Sendable (AnalysisRunSummary) -> Void

    init(
        onProgress: @escaping @Sendable (AnalysisPhase, UInt32, UInt32) -> Void,
        onFinished: @escaping @Sendable (AnalysisRunSummary) -> Void
    ) {
        self.onProgressHop = onProgress
        self.onFinishedHop = onFinished
    }

    func onProgress(phase: AnalysisPhase, done: UInt32, total: UInt32) {
        onProgressHop(phase, done, total)
    }

    func onFinished(summary: AnalysisRunSummary) {
        onFinishedHop(summary)
    }
}
