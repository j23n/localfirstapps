import Foundation

/// Identity used to detect when a file changed without reading it.
/// Size and mtime are the live pair (ADR 0002 R6). Older snapshots may carry
/// additional keys; `Codable` ignores those keys and current writes omit them.
struct ContentVersion: Hashable, Codable, Sendable {
    var modificationDate: Date?
    var size: Int64?

    var isEmpty: Bool {
        modificationDate == nil && size == nil
    }

    init(modificationDate: Date? = nil, size: Int64? = nil) {
        self.modificationDate = modificationDate
        self.size = size
    }

    enum CodingKeys: String, CodingKey {
        case modificationDate
        case size
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        modificationDate = try c.decodeIfPresent(Date.self, forKey: .modificationDate)
        size = try c.decodeIfPresent(Int64.self, forKey: .size)
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encodeIfPresent(modificationDate, forKey: .modificationDate)
        try c.encodeIfPresent(size, forKey: .size)
    }

    /// Size and modification date are the complete identity.
    static func sameContent(_ lhs: ContentVersion, _ rhs: ContentVersion) -> Bool {
        lhs.modificationDate == rhs.modificationDate && lhs.size == rhs.size
    }

    /// Size/mtime stamp from local resource values.
    static func ofFile(at url: URL) -> ContentVersion {
        let values = try? url.resourceValues(forKeys: [.contentModificationDateKey, .fileSizeKey])
        return ContentVersion(
            modificationDate: values?.contentModificationDate,
            size: values?.fileSize.map(Int64.init)
        )
    }
}
