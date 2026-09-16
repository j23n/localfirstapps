import Foundation
import os
import os.lock

/// The app's side of the Rust scanner: owns the core's `ScannerSession` and
/// the bridge that turns a `ScanOutcomeRecord` into the app's own
/// `PhotoFile` / `PhotoFolder` values.
///
/// Replaces `FolderScanner`. Scan *policy* — light/full/auto resolution, the
/// 48-hour promotion, request dedupe, the two-phase ordering, and the
/// sidecar-sync / memories / widget steps that follow — is untouched and still
/// lives in `GalleryStore+Scanning.swift`. This type walks nothing and decides
/// nothing; it hands the core a root and hands the Store back a result in the
/// shape it already consumed.
///
/// One instance per Store. It holds no scan state between calls: the cache goes
/// in with each request and the outcome comes straight back out, exactly as
/// `FolderScanner.scan(cachedPhotos:…)` did.
final class CoreScanner: Sendable {

    /// What one pass produced. Field-for-field the old `FolderScanner.Result`,
    /// so `runScanPass` reads the same.
    struct Result: Sendable {
        let rootFolder: PhotoFolder?
        let flatPhotos: [PhotoFile]
        let needsEnrichment: Bool
        let sidecarManifest: [SidecarCandidate]
        /// URLs present in the scan but not in `cachedPhotos`.
        let addedURLs: [URL]
        /// URLs present in `cachedPhotos` but absent from the scan.
        let removedURLs: [URL]
        /// URLs whose `fileSize` or `fileModificationDate` changed since the
        /// cache. Disjoint from `addedURLs`.
        let modifiedURLs: [URL]
        /// Standardized paths of directories whose listing threw a *transient*
        /// error (`PermissionDenied`, other I/O). Photos under them are absent
        /// from `flatPhotos`/`removedURLs` — the Store carries the cached
        /// entries forward so a listing failure does not wipe a subtree's
        /// tags and enrichment. `NotFound` is not listed here: those photos appear
        /// in `removedURLs`, and a missing root arrives as `rootFolder == nil`.
        let failedDirectoryPaths: [String]
        /// The pass produced no outcome at all: the core threw instead of
        /// answering — a cancelled walk, or an unreadable snapshot.
        ///
        /// **This is not "the scan found nothing".** The two have the same
        /// shape — empty photos, no tree — and opposite meanings, and the Store
        /// publishes one of them. An unreadable *directory* is data and arrives
        /// through `failedDirectoryPaths` with the rest of the library intact;
        /// this flag says the value carries no information about the library,
        /// so `runScanPass` returns before it can reach `apply(_:)`.
        let didNotComplete: Bool

        /// A pass that never produced an answer. Every field is empty *and*
        /// `didNotComplete` is set, so a caller that ignores the flag still
        /// cannot mistake it for a scan of an empty folder — it will publish
        /// an empty library, which is exactly what the flag exists to stop.
        static let incomplete = Result(
            rootFolder: nil, flatPhotos: [], needsEnrichment: false,
            sidecarManifest: [], addedURLs: [], removedURLs: [],
            modifiedURLs: [], failedDirectoryPaths: [], didNotComplete: true
        )
    }

    private let session: ScannerSession

    init() {
        session = ScannerSession()
    }

    /// Walk `rootURL` and produce the tree, the flat list, and the diff.
    ///
    /// - Parameters:
    ///   - cachedPhotos: URL → previous-scan `PhotoFile`. Carries EXIF / tags /
    ///     GPS forward.
    ///   - cachedSidecarManifest: photoID → previous-scan `SidecarCandidate`.
    ///     A hit here is what lets a light scan skip rebuilding an `.xmp` row
    ///     when the listing size and mtime still match.
    ///   - reuseCached: light scan when true; see the blind spot documented on
    ///     `gallery_scan::scan`.
    ///   - onProgress: count fired from the core's scan thread. Mid-walk the
    ///     number is content files (photos, videos, `.xmp`); the last call is
    ///     the photo total. Safe to hop to the main actor inside.
    func scan(
        at rootURL: URL,
        cachedPhotos: [URL: PhotoFile] = [:],
        cachedSidecarManifest: [UUID: SidecarCandidate] = [:],
        reuseCached: Bool = false,
        onProgress: (@Sendable (Int) -> Void)? = nil
    ) async -> Result {
        let session = self.session

        // A scan whose Task was cancelled before the FFI call started must not
        // start: `cancel()` names the run in flight, and with none in flight it
        // is deliberately a no-op rather than an ambush on the next run.
        if Task.isCancelled { return .incomplete }

        // The detached task below does not inherit cancellation — that is the
        // point of detaching, and it is why the flag has to be reachable from
        // another thread. `onCancel` hands it to the core, which stops the walk
        // at its next directory boundary and answers `.cancelled`.
        let result = await withTaskCancellationHandler {
            await Self.run(
                session: session, rootURL: rootURL, cachedPhotos: cachedPhotos,
                cachedSidecarManifest: cachedSidecarManifest, reuseCached: reuseCached,
                onProgress: onProgress
            )
        } onCancel: {
            session.cancel()
        }

        // A cancel that lands between the core's last directory boundary and
        // its return produces a *complete* outcome for a scan the caller no
        // longer wants. Discard it: the caller cancelled because the state it
        // scanned against is gone, and publishing a result it did not ask for
        // is the failure mode this whole flag exists to prevent. Losing a
        // finished scan is the cheap direction.
        return Task.isCancelled ? .incomplete : result
    }

    private static func run(
        session: ScannerSession,
        rootURL: URL,
        cachedPhotos: [URL: PhotoFile],
        cachedSidecarManifest: [UUID: SidecarCandidate],
        reuseCached: Bool,
        onProgress: (@Sendable (Int) -> Void)?
    ) async -> Result {
        return await Task.detached(priority: .userInitiated) {
            let startedAt = CFAbsoluteTimeGetCurrent()
            let request = ScanRequest(
                reuseCached: reuseCached,
                cachedPhotos: cachedPhotos.values.map(Self.record(of:)),
                cachedSidecarManifest: cachedSidecarManifest.values.map(Self.row(of:))
            )
            let marshalledAt = CFAbsoluteTimeGetCurrent()

            let outcome: ScanOutcomeRecord
            do {
                outcome = try session.scan(
                    root: rootURL.path,
                    request: request,
                    progress: onProgress.map(ProgressBridge.init)
                )
            } catch {
                // A missing root is data, not an error: empty `folders`, no
                // failed-directory entry, `rootFolder == nil`. An unlistable
                // root (permission or other I/O) still arrives as
                // `failedDirectoryPaths` with an empty tree. Reaching here
                // means the core produced no answer at all — a cancelled walk
                // today — and an empty answer is indistinguishable from "the
                // library is now empty". `.incomplete` is how the Store tells
                // them apart.
                Log.scan.error("core scan failed: \(Log.r.error(error))")
                return .incomplete
            }
            let scannedAt = CFAbsoluteTimeGetCurrent()
            let result = Self.bridge(outcome)
            let builtAt = CFAbsoluteTimeGetCurrent()

            // The core does not own logging, so the one place an unreadable
            // directory becomes visible to a human is here. It matters: every
            // photo under such a directory is being carried forward on trust,
            // and a subtree that stays unreadable across scans is a real
            // problem wearing a "no changes" costume.
            if !result.failedDirectoryPaths.isEmpty {
                let listed = result.failedDirectoryPaths.prefix(5)
                    .map { Log.r.path($0) }
                    .joined(separator: ", ")
                Log.scan.warning("""
                    \(result.failedDirectoryPaths.count) unreadable \
                    director\(result.failedDirectoryPaths.count == 1 ? "y" : "ies"): \(listed)\
                    \(result.failedDirectoryPaths.count > 5 ? ", …" : "")
                    """)
            }
            // Same shape as the line docs/adr/0002
            // measures the acceptance gates from, so the harness keeps working
            // across the port. `core` is time inside Rust; `in`/`out` are the
            // two record marshals, which is the number that decides whether the
            // FFI payload strategy needs revisiting.
            let ms = { (a: CFAbsoluteTime, b: CFAbsoluteTime) in String(format: "%.0f", (b - a) * 1000) }
            let t = outcome.timings
            Log.scan.info("""
                Scan totals: \(outcome.flatPhotos.count) files in \(t.folders) folders, \
                total=\(ms(startedAt, builtAt))ms core=\(t.totalMillis)ms list=\(t.listMillis)ms \
                hits=\(t.cacheHits) slow=\(t.slowPath) \
                in=\(ms(startedAt, marshalledAt))ms out=\(ms(scannedAt, builtAt))ms \
                ffi=\(ms(marshalledAt, scannedAt))ms reuseCached=\(reuseCached)
                """)
            return result
        }.value
    }

    /// Ask an in-flight walk to stop.
    ///
    /// `scan(at:…)` already wires this to its own `Task`'s cancellation, so
    /// cancelling the Store's scan task is enough; this stays for a caller that
    /// holds the scanner but not the task.
    func cancel() {
        session.cancel()
    }

    // MARK: - Progress

    /// Bridges the core's foreign trait to a plain `@Sendable` closure. Holds
    /// nothing but the closure, so it cannot capture the Store.
    private final class ProgressBridge: ScanProgressListener {
        private let handler: @Sendable (Int) -> Void
        init(_ handler: @escaping @Sendable (Int) -> Void) { self.handler = handler }
        func onProgress(discovered: UInt32) { handler(Int(discovered)) }
    }

    // MARK: - Bridging out

    private static func bridge(_ outcome: ScanOutcomeRecord) -> Result {
        let photos = outcome.flatPhotos.map(photo(from:))
        return Result(
            rootFolder: folderTree(outcome.folders, photos: photos),
            flatPhotos: photos,
            needsEnrichment: outcome.needsEnrichment,
            sidecarManifest: outcome.sidecarManifest.map(candidate(from:)),
            addedURLs: outcome.addedPaths.map(fileURL(_:)),
            removedURLs: outcome.removedPaths.map(fileURL(_:)),
            modifiedURLs: outcome.modifiedPaths.map(fileURL(_:)),
            failedDirectoryPaths: outcome.failedDirectoryPaths,
            didNotComplete: false
        )
    }

    /// A file URL whose `path` is byte-for-byte the string it was built from.
    ///
    /// **Not** `URL(fileURLWithPath:)`, which DECOMPOSES its input
    /// (`PathNormalizationTests`). An externally-created NFC filename would
    /// come back NFD, `PhotoFile.stableID` hashes `standardized.path`, and the
    /// photo would land under a different id than the core just derived for it
    /// — a silent identity split on exactly the files that arrive from other
    /// systems. Percent-encoding round-trips the scalars intact and measures
    /// the same (~1 µs/path over a 20k library).
    static func fileURL(_ path: String) -> URL {
        guard let encoded = path.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed),
              let url = URL(string: "file://" + encoded) else {
            return URL(fileURLWithPath: path)
        }
        return url
    }

    private static func photo(from record: ScanPhoto) -> PhotoFile {
        PhotoFile(
            id: UUID(uuidString: record.id) ?? PhotoFile.stableID(for: fileURL(record.path)),
            url: fileURL(record.path),
            filename: record.filename,
            fileSize: record.fileSize,
            dateTaken: record.dateTaken.map(Date.init(timeIntervalSinceReferenceDate:)),
            dateFromMetadata: record.dateFromMetadata,
            isVideo: record.isVideo,
            livePhotoVideoURL: record.livePhotoVideoPath.map(fileURL(_:)),
            hierarchicalTags: record.hierarchicalTags.map {
                HierarchicalTag(fullPath: $0.fullPath, namespace: $0.namespace, displayName: $0.displayName)
            },
            countryCode: record.countryCode,
            enrichedFileDate: record.enrichedFileDate.map(Date.init(timeIntervalSinceReferenceDate:)),
            fileModificationDate: record.fileModificationDate.map(Date.init(timeIntervalSinceReferenceDate:)),
            gpsLatitude: record.gpsLatitude,
            gpsLongitude: record.gpsLongitude,
            faceRegions: record.faceRegions.map {
                FaceRegion(name: $0.name, centerX: $0.centerX, centerY: $0.centerY,
                           width: $0.width, height: $0.height)
            }
        )
    }

    /// Rebuild the recursive tree from the core's flat node list.
    ///
    /// The core sends folders flat, each carrying a parent index and a
    /// `(photoStart, photoCount)` slice of `flatPhotos`, because `PhotoFolder`
    /// owns its photos by value and shipping both shapes would put every photo
    /// on the wire twice. Nodes always precede their children, so one reverse
    /// pass assembles the tree with no repeated work.
    private static func folderTree(_ nodes: [ScanFolderNode], photos: [PhotoFile]) -> PhotoFolder? {
        guard !nodes.isEmpty else { return nil }
        var childrenByParent: [Int: [Int]] = [:]
        for (index, node) in nodes.enumerated() {
            guard let parent = node.parentIndex else { continue }
            childrenByParent[Int(parent), default: []].append(index)
        }
        var built: [Int: PhotoFolder] = [:]
        for index in nodes.indices.reversed() {
            let node = nodes[index]
            let start = Int(node.photoStart)
            let end = min(start + Int(node.photoCount), photos.count)
            built[index] = PhotoFolder(
                id: UUID(uuidString: node.id) ?? PhotoFolder.stableID(for: fileURL(node.path)),
                url: fileURL(node.path),
                name: node.name,
                subfolders: (childrenByParent[index] ?? []).compactMap { built[$0] },
                photos: start < end ? Array(photos[start..<end]) : [],
                coverPhotoURL: node.coverPhotoPath.map(fileURL(_:)),
                totalPhotoCount: Int(node.totalPhotoCount),
                dateModified: node.dateModified.map(Date.init(timeIntervalSinceReferenceDate:)),
                dateCreated: node.dateCreated.map(Date.init(timeIntervalSinceReferenceDate:))
            )
        }
        return built[0]
    }

    private static func candidate(from row: ScanSidecarRow) -> SidecarCandidate {
        SidecarCandidate(
            photoID: UUID(uuidString: row.photoId) ?? PhotoFile.stableID(
                for: fileURL(String(row.sidecarPath.dropLast(".xmp".count)))
            ),
            sidecarURL: fileURL(row.sidecarPath),
            currentVersion: ContentVersion(
                modificationDate: row.currentVersion.modificationDate
                    .map(Date.init(timeIntervalSinceReferenceDate:)),
                size: row.currentVersion.size
            )
        )
    }

    // MARK: - Bridging in

    static func record(of photo: PhotoFile) -> ScanPhoto {
        ScanPhoto(
            id: photo.id.uuidString,
            path: photo.url.path,
            filename: photo.filename,
            fileSize: photo.fileSize,
            dateTaken: photo.dateTaken?.timeIntervalSinceReferenceDate,
            dateFromMetadata: photo.dateFromMetadata,
            isVideo: photo.isVideo,
            livePhotoVideoPath: photo.livePhotoVideoURL?.path,
            hierarchicalTags: photo.hierarchicalTags.map {
                ScanTag(fullPath: $0.fullPath, namespace: $0.namespace, displayName: $0.displayName)
            },
            countryCode: photo.countryCode,
            enrichedFileDate: photo.enrichedFileDate?.timeIntervalSinceReferenceDate,
            fileModificationDate: photo.fileModificationDate?.timeIntervalSinceReferenceDate,
            gpsLatitude: photo.gpsLatitude,
            gpsLongitude: photo.gpsLongitude,
            faceRegions: photo.faceRegions.map {
                ScanRegion(name: $0.name, centerX: $0.centerX, centerY: $0.centerY,
                           width: $0.width, height: $0.height)
            }
        )
    }

    static func row(of candidate: SidecarCandidate) -> ScanSidecarRow {
        ScanSidecarRow(
            photoId: candidate.photoID.uuidString,
            sidecarPath: candidate.sidecarURL.path,
            currentVersion: ScanContentVersion(
                modificationDate: candidate.currentVersion.modificationDate?
                    .timeIntervalSinceReferenceDate,
                size: candidate.currentVersion.size
            )
        )
    }
}
