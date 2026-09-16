import Foundation

/// Swift face of the gallery person-state log.
///
/// On-disk layout (ADR 0005 R4/R5):
/// `{library}/.gallery/log/<dev>/YYYY-MM.ndjson`
///
/// Writes go through gallery-ffi (`person_log_append` /
/// `person_log_project` / `person_log_migrate_from_snapshot`), which
/// calls `localcore-log`. Lines are field order `id,ts,dev,type,body`,
/// 9-digit UTC `ts`, no HTML escape. UserDefaults is read only as the
/// pre-M2 import source; after attach the log projection is authoritative.
/// Migrate is one-shot via a `person_migrated` marker written last.
///
/// The device id is the ADR 0005 R5 UserDefaults exception and is not
/// written into the synced folder.
enum PersonLog {
    static let deviceIdKey = "galleryDeviceId"
    /// Install-local one-shot cursor. Once true, the five obsolete domain
    /// keys are not even read again.
    static let migrationCursorKey = "galleryPersonLogMigrationComplete"

    /// Per-device id, created once. Must match `localcore_log::valid_device`.
    static func deviceId(in defaults: UserDefaults) -> String {
        if let existing = defaults.string(forKey: deviceIdKey), isValidDevice(existing) {
            return existing
        }
        let id = UUID().uuidString.lowercased()
        defaults.set(id, forKey: deviceIdKey)
        return id
    }

    /// ASCII `[A-Za-z0-9][A-Za-z0-9._-]*`, matching `localcore_log::valid_device`.
    static func isValidDevice(_ name: String) -> Bool {
        var bytes = name.utf8.makeIterator()
        guard let first = bytes.next(), isAsciiAlphanumeric(first) else { return false }
        return bytes.allSatisfy { isAsciiAlphanumeric($0) || $0 == 0x2E || $0 == 0x5F || $0 == 0x2D }
    }

    private static func isAsciiAlphanumeric(_ b: UInt8) -> Bool {
        (0x30...0x39).contains(b) || (0x41...0x5A).contains(b) || (0x61...0x7A).contains(b)
    }

    /// Pre-M2 dump of the five UserDefaults keys (ADR 0005 R19 fixture shape).
    struct Snapshot: Equatable, Sendable {
        var hiddenPeople: [String]
        var pinnedPeople: [String]
        var featuredPhotoByPerson: [String: String]
        var mePersonPath: String
        var personContactLinks: [String: PersonLink]

        static let empty = Snapshot(
            hiddenPeople: [],
            pinnedPeople: [],
            featuredPhotoByPerson: [:],
            mePersonPath: "",
            personContactLinks: [:]
        )

        /// Read old install-local fields only for the one-shot M2 import.
        /// Per-entry tolerant contact-link decoding preserves every valid
        /// decision if one old value is malformed.
        static func legacy(in defaults: UserDefaults) -> Snapshot {
            guard !defaults.bool(forKey: PersonLog.migrationCursorKey) else {
                return .empty
            }
            let links: [String: PersonLink]
            if let data = defaults.data(forKey: "personContactLinks"),
               let decoded = try? JSONDecoder().decode(
                   [String: FailableDecodable<PersonLink>].self,
                   from: data
               ) {
                links = decoded.compactMapValues(\.value)
            } else {
                links = [:]
            }
            return Snapshot(
                hiddenPeople: defaults.array(forKey: "hiddenPeople") as? [String] ?? [],
                pinnedPeople: defaults.array(forKey: "pinnedPeople") as? [String] ?? [],
                featuredPhotoByPerson:
                    defaults.dictionary(forKey: "featuredPhotoByPerson") as? [String: String] ?? [:],
                mePersonPath: defaults.string(forKey: "mePersonPath") ?? "",
                personContactLinks: links
            )
        }

        /// Called only once `person_log_migrate_from_snapshot` confirms this
        /// device's marker exists. Obsolete domain snapshots must not linger
        /// as a tempting fallback.
        static func clearLegacy(from defaults: UserDefaults) {
            for key in [
                "hiddenPeople",
                "pinnedPeople",
                "featuredPhotoByPerson",
                "mePersonPath",
                "personContactLinks",
            ] {
                defaults.removeObject(forKey: key)
            }
            defaults.set(true, forKey: PersonLog.migrationCursorKey)
        }

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

        var isEmpty: Bool {
            hiddenPeople.isEmpty
                && pinnedPeople.isEmpty
                && featuredPhotoByPerson.isEmpty
                && mePersonPath.isEmpty
                && personContactLinks.isEmpty
        }

        func remappingPhotoIDs(_ ids: [UUID: UUID]) -> Snapshot {
            guard !ids.isEmpty else { return self }
            var copy = self
            copy.featuredPhotoByPerson = featuredPhotoByPerson.compactMapValues { raw in
                guard let old = UUID(uuidString: raw) else { return nil }
                return (ids[old] ?? old).uuidString
            }
            return copy
        }
    }

    struct State: Equatable, Sendable {
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

        var isEmpty: Bool {
            hidden.isEmpty && featured.isEmpty && me.isEmpty && featuredPhoto.isEmpty && links.isEmpty
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

    struct TornTail: Equatable, Sendable {
        var path: String
        var offset: UInt64
        var detail: String
    }

    struct Projection: Equatable, Sendable {
        var state: State
        var tornTails: [TornTail]
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
    ) throws {
        let bodyJSON = "{" + body.map { "\(jsonString($0.0)):\($0.1.encoded)" }.joined(separator: ",") + "}"
        try personLogAppend(
            root: path(libraryRoot),
            device: device,
            eventType: type,
            bodyJson: bodyJSON
        )
    }

    /// One-shot import. Returns events written (0 if this device already has
    /// a `person_migrated` marker). Throws `PersonLogError` on FFI failure.
    @discardableResult
    static func migrate(
        libraryRoot: URL,
        device: String,
        snapshot: Snapshot
    ) throws -> Int {
        let n = try personLogMigrateFromSnapshot(
            root: path(libraryRoot),
            device: device,
            snapshotJson: snapshot.jsonString()
        )
        return Int(n)
    }

    static func project(libraryRoot: URL) throws -> State {
        State(try personLogProject(root: path(libraryRoot)))
    }

    static func projectReport(libraryRoot: URL) throws -> Projection {
        let report = try personLogProjectReport(root: path(libraryRoot))
        return Projection(
            state: State(report.state),
            tornTails: report.tornTails.map {
                TornTail(path: $0.path, offset: $0.offset, detail: $0.detail)
            }
        )
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

/// Injectable log boundary. Production stays on the Rust FFI; unit tests can
/// force an append failure without relying on chmod behavior under CI.
@MainActor
struct PersonLogBackend {
    var append: (
        _ libraryRoot: URL,
        _ device: String,
        _ type: String,
        _ body: [(String, PersonLog.LogJSON)]
    ) throws -> Void
    var migrate: (
        _ libraryRoot: URL,
        _ device: String,
        _ snapshot: PersonLog.Snapshot
    ) throws -> Int
    var project: (_ libraryRoot: URL) throws -> PersonLog.Projection

    static let live = PersonLogBackend(
        append: { root, device, type, body in
            try PersonLog.append(libraryRoot: root, device: device, type: type, body: body)
        },
        migrate: { root, device, snapshot in
            try PersonLog.migrate(libraryRoot: root, device: device, snapshot: snapshot)
        },
        project: { root in
            try PersonLog.projectReport(libraryRoot: root)
        }
    )
}
