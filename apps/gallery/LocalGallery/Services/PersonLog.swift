import Foundation

/// Swift face of the gallery person-state log.
///
/// On-disk layout (ADR 0005 R4/R5):
/// `{library}/.gallery/log/<dev>/YYYY-MM.ndjson`
///
/// Writes go through gallery-ffi (`person_log_append` /
/// `person_log_project` / `person_log_migrate_from_snapshot`), which
/// calls `localcore-log`. Lines are field order `id,ts,dev,type,body`,
/// 9-digit UTC `ts`, no HTML escape.
///
/// The device id is the ADR 0005 R5 UserDefaults exception and is not
/// written into the synced folder.
enum PersonLog {
    static let deviceIdKey = "galleryDeviceId"

    /// Per-device id, created once. Must match `localcore_log::valid_device`.
    static func deviceId(in defaults: UserDefaults) -> String {
        if let existing = defaults.string(forKey: deviceIdKey), isValidDevice(existing) {
            return existing
        }
        let id = UUID().uuidString.lowercased()
        defaults.set(id, forKey: deviceIdKey)
        return id
    }

    static func isValidDevice(_ name: String) -> Bool {
        guard let first = name.unicodeScalars.first,
              CharacterSet.alphanumerics.contains(first)
        else { return false }
        return name.unicodeScalars.allSatisfy {
            CharacterSet.alphanumerics.contains($0) || $0 == "." || $0 == "_" || $0 == "-"
        }
    }

    /// Pre-M2 dump of the five UserDefaults keys (ADR 0005 R19 fixture shape).
    struct Snapshot: Equatable {
        var hiddenPeople: [String]
        var pinnedPeople: [String]
        var featuredPhotoByPerson: [String: String]
        var mePersonPath: String
        var personContactLinks: [String: PersonLink]

        func jsonString() -> String {
            let encodedLinks: [String: Any] = Dictionary(
                uniqueKeysWithValues: personContactLinks.map { path, link in
                    switch link {
                    case .manual(let id):
                        return (path, ["manual": ["contactID": id]])
                    case .disabled:
                        return (path, ["disabled": [String: String]()])
                    }
                }
            )
            let obj: [String: Any] = [
                "hiddenPeople": hiddenPeople,
                "pinnedPeople": pinnedPeople,
                "featuredPhotoByPerson": featuredPhotoByPerson,
                "mePersonPath": mePersonPath,
                "personContactLinks": encodedLinks,
            ]
            let data = try? JSONSerialization.data(withJSONObject: obj, options: [.sortedKeys])
            return String(data: data ?? Data("{}".utf8), encoding: .utf8) ?? "{}"
        }
    }

    struct State: Equatable {
        var hidden: Set<String> = []
        var featured: [String] = []
        var me: String = ""
        var featuredPhoto: [String: String] = [:]
        /// Empty value is `PersonLink.disabled`.
        var links: [String: String] = [:]

        init(
            hidden: Set<String> = [],
            featured: [String] = [],
            me: String = "",
            featuredPhoto: [String: String] = [:],
            links: [String: String] = [:]
        ) {
            self.hidden = hidden
            self.featured = featured
            self.me = me
            self.featuredPhoto = featuredPhoto
            self.links = links
        }

        init(_ record: PersonStateRecord) {
            hidden = Set(record.hidden)
            featured = record.featured
            me = record.me
            featuredPhoto = Dictionary(uniqueKeysWithValues: record.featuredPhoto.map { ($0.path, $0.value) })
            links = Dictionary(uniqueKeysWithValues: record.links.map { ($0.path, $0.value) })
        }

        func personLinks() -> [String: PersonLink] {
            Dictionary(uniqueKeysWithValues: links.map { path, value in
                (path, value.isEmpty ? PersonLink.disabled : PersonLink.manual(contactID: value))
            })
        }
    }

    enum LogJSON {
        case string(String)
        case bool(Bool)

        var encoded: String {
            switch self {
            case .string(let s): return jsonString(s)
            case .bool(let b): return b ? "true" : "false"
            }
        }
    }

    static func append(
        libraryRoot: URL,
        device: String,
        type: String,
        body: [(String, LogJSON)]
    ) {
        let bodyJSON = "{" + body.map { "\(jsonString($0.0)):\($0.1.encoded)" }.joined(separator: ",") + "}"
        try? personLogAppend(
            root: path(libraryRoot),
            device: device,
            eventType: type,
            bodyJson: bodyJSON
        )
    }

    /// One-shot import. Returns events written (0 if this device already logged).
    @discardableResult
    static func migrate(
        libraryRoot: URL,
        device: String,
        snapshot: Snapshot
    ) -> Int {
        let n = try? personLogMigrateFromSnapshot(
            root: path(libraryRoot),
            device: device,
            snapshotJson: snapshot.jsonString()
        )
        return Int(n ?? 0)
    }

    static func project(libraryRoot: URL) -> State? {
        guard let record = try? personLogProject(root: path(libraryRoot)) else {
            return nil
        }
        return State(record)
    }

    /// Compact JSON string. `<` `>` `&` stay literal (Go `SetEscapeHTML(false)`).
    static func jsonString(_ s: String) -> String {
        var out = "\""
        for scalar in s.unicodeScalars {
            switch scalar.value {
            case 0x22: out += "\\\""
            case 0x5C: out += "\\\\"
            case 0x08: out += "\\b"
            case 0x0C: out += "\\f"
            case 0x0A: out += "\\n"
            case 0x0D: out += "\\r"
            case 0x09: out += "\\t"
            case 0..<0x20:
                out += String(format: "\\u%04x", scalar.value)
            default:
                out.append(Character(scalar))
            }
        }
        out += "\""
        return out
    }

    private static func path(_ url: URL) -> String {
        url.standardized.path
    }
}
