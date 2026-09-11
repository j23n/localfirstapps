import Foundation

/// Identity used to detect when a file changed without reading it.
/// Size and mtime are the live pair (ADR 0002 R6). `contentIdentifier` is
/// decode-only: old snapshots and sidecar-cache rows may still carry a
/// provider-vended token; new writes omit it (Phase 1 / M4).
struct ContentVersion: Hashable, Codable, Sendable {
    var contentIdentifier: String?
    var modificationDate: Date?
    var size: Int64?

    var isEmpty: Bool {
        contentIdentifier == nil && modificationDate == nil && size == nil
    }

    init(contentIdentifier: String? = nil, modificationDate: Date? = nil, size: Int64? = nil) {
        self.contentIdentifier = contentIdentifier
        self.modificationDate = modificationDate
        self.size = size
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        if let raw = try? c.decodeIfPresent(String.self, forKey: .contentIdentifier) {
            contentIdentifier = raw
        } else if let legacy = try? c.decodeIfPresent(Int64.self, forKey: .contentIdentifier) {
            contentIdentifier = String(legacy)
        } else {
            contentIdentifier = nil
        }
        modificationDate = try c.decodeIfPresent(Date.self, forKey: .modificationDate)
        size = try c.decodeIfPresent(Int64.self, forKey: .size)
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        // Stop writing the provider token. Old files still decode.
        try c.encodeIfPresent(modificationDate, forKey: .modificationDate)
        try c.encodeIfPresent(size, forKey: .size)
    }

    /// Identifiers win when both sides have one; `(mtime, size)` when neither
    /// does. Mixed presence is different.
    static func sameContent(_ lhs: ContentVersion, _ rhs: ContentVersion) -> Bool {
        switch (lhs.contentIdentifier, rhs.contentIdentifier) {
        case let (l?, r?): return l == r
        case (nil, nil):
            return lhs.modificationDate == rhs.modificationDate && lhs.size == rhs.size
        default:
            return false
        }
    }

    /// Stat-only stamp. No ubiquitous keys, no content identifier.
    static func ofFile(at url: URL) -> ContentVersion {
        let values = try? url.resourceValues(forKeys: [.contentModificationDateKey, .fileSizeKey])
        return ContentVersion(
            modificationDate: values?.contentModificationDate,
            size: values?.fileSize.map(Int64.init)
        )
    }
}

/// Wire spelling for `SidecarCandidate.downloadStatus`. Decode-tolerant;
/// new writes omit `.local` (M4 preferred).
enum DownloadStatus: String, Codable, Equatable, Sendable {
    case local
    case placeholder
    case downloading
    case stale
}
