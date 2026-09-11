import Foundation

/// One row of the sidecar manifest a scan emits: which `.xmp` belongs to which
/// photo, and what version of it the scan saw. `SidecarSyncService` diffs these
/// against `SidecarCacheStore` to decide what to fetch, without re-reading
/// anything.
///
/// Lived on `FolderScanner` until the scanner moved into the Rust core; it is a
/// value type the Store, the sync service and the persisted `LibrarySnapshot`
/// all share, so it belongs with the other models rather than inside whichever
/// service happens to produce it.
///
/// **`Codable` is load-bearing.** docs/adr/0002: the manifest rides in
/// `LibrarySnapshot` as an optional field so the light-scan fast path
/// survives relaunch. Encoding must match
/// `gallery_model::snapshot::SidecarCandidate` — the Rust core decodes
/// the same file.
struct SidecarCandidate: Codable, Equatable, Sendable {
    let photoID: UUID
    let sidecarURL: URL
    let currentVersion: ContentVersion
    let downloadStatus: DownloadStatus

    init(
        photoID: UUID,
        sidecarURL: URL,
        currentVersion: ContentVersion,
        downloadStatus: DownloadStatus = .local
    ) {
        self.photoID = photoID
        self.sidecarURL = sidecarURL
        self.currentVersion = currentVersion
        self.downloadStatus = downloadStatus
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        photoID = try c.decode(UUID.self, forKey: .photoID)
        sidecarURL = try c.decode(URL.self, forKey: .sidecarURL)
        currentVersion = try c.decode(ContentVersion.self, forKey: .currentVersion)
        downloadStatus = try c.decodeIfPresent(DownloadStatus.self, forKey: .downloadStatus) ?? .local
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(photoID, forKey: .photoID)
        try c.encode(sidecarURL, forKey: .sidecarURL)
        try c.encode(currentVersion, forKey: .currentVersion)
        if downloadStatus != .local {
            try c.encode(downloadStatus, forKey: .downloadStatus)
        }
    }
}
