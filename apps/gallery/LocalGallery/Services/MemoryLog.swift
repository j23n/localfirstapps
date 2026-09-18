import Foundation

/// Swift face of the gallery memory-chrome log.
///
/// On-disk layout (ADR 0005 R4/R5), shared with `PersonLog`:
/// `{library}/.gallery/log/<dev>/YYYY-MM.ndjson`
///
/// Writes go through gallery-ffi (`memory_log_append` /
/// `memory_log_project` / `memory_log_migrate_from_snapshot`), which
/// calls `localcore-log`. UserDefaults is read only as the pre-M2 import
/// source; after attach the log projection is authoritative. Migrate is
/// one-shot via a `memory_migrated` marker written last.
///
/// The device id is reused from `PersonLog.deviceId` (ADR 0005 R5) and is
/// not written into the synced folder.
enum MemoryLog {
    /// Install-local one-shot cursor. Once true, the five obsolete domain
    /// keys are not even read again.
    static let migrationCursorKey = "galleryMemoryLogMigrationComplete"

    /// Same per-device id `PersonLog` created. Must match
    /// `localcore_log::valid_device`.
    static func deviceId(in defaults: UserDefaults) -> String {
        PersonLog.deviceId(in: defaults)
    }

    /// Pre-M2 dump of the five UserDefaults keys.
    struct Snapshot: Equatable, Sendable {
        var hiddenMemories: [String]
        var seenMemoryIDs: [String: Date]
        var surfacedClusters: [String: Date]
        var birthdayMemoriesEnabled: Bool?
        var memoriesGeneratedDay: Date?

        static let empty = Snapshot(
            hiddenMemories: [],
            seenMemoryIDs: [:],
            surfacedClusters: [:],
            birthdayMemoriesEnabled: nil,
            memoriesGeneratedDay: nil
        )

        /// Read old install-local fields only for the one-shot M2 import.
        static func legacy(in defaults: UserDefaults) -> Snapshot {
            guard !defaults.bool(forKey: MemoryLog.migrationCursorKey) else {
                return .empty
            }
            let seen: [String: Date]
            if let data = defaults.data(forKey: "seenMemoryIDs"),
               let decoded = try? JSONDecoder().decode([String: Date].self, from: data) {
                seen = decoded
            } else {
                seen = [:]
            }
            let surfaced: [String: Date]
            if let data = defaults.data(forKey: "surfacedClusters"),
               let decoded = try? JSONDecoder().decode([String: Date].self, from: data) {
                surfaced = decoded
            } else {
                surfaced = [:]
            }
            let birthdays: Bool?
            if defaults.object(forKey: "birthdayMemoriesEnabled") != nil {
                birthdays = defaults.bool(forKey: "birthdayMemoriesEnabled")
            } else {
                birthdays = nil
            }
            return Snapshot(
                hiddenMemories: defaults.array(forKey: "hiddenMemories") as? [String] ?? [],
                seenMemoryIDs: seen,
                surfacedClusters: surfaced,
                birthdayMemoriesEnabled: birthdays,
                memoriesGeneratedDay: defaults.object(forKey: "memoriesGeneratedDay") as? Date
            )
        }

        /// Called only once `memory_log_migrate_from_snapshot` confirms this
        /// device's marker exists. Obsolete domain snapshots must not linger
        /// as a tempting fallback.
        static func clearLegacy(from defaults: UserDefaults) {
            for key in [
                "hiddenMemories",
                "seenMemoryIDs",
                "surfacedClusters",
                "birthdayMemoriesEnabled",
                "memoriesGeneratedDay",
            ] {
                defaults.removeObject(forKey: key)
            }
            defaults.set(true, forKey: MemoryLog.migrationCursorKey)
        }

        func jsonString() -> String {
            var obj: [String: Any] = [
                "hiddenMemories": hiddenMemories,
                "seenMemoryIDs": seenMemoryIDs.mapValues(MemoryLog.formatDate),
                "surfacedClusters": surfacedClusters.mapValues(MemoryLog.formatDate),
            ]
            if let birthdayMemoriesEnabled {
                obj["birthdayMemoriesEnabled"] = birthdayMemoriesEnabled
            }
            if let memoriesGeneratedDay {
                obj["memoriesGeneratedDay"] = MemoryLog.formatDate(memoriesGeneratedDay)
            }
            let data = try? JSONSerialization.data(withJSONObject: obj, options: [.sortedKeys])
            return String(data: data ?? Data("{}".utf8), encoding: .utf8) ?? "{}"
        }

        var isEmpty: Bool {
            hiddenMemories.isEmpty
                && seenMemoryIDs.isEmpty
                && surfacedClusters.isEmpty
                && birthdayMemoriesEnabled == nil
                && memoriesGeneratedDay == nil
        }
    }

    struct State: Equatable, Sendable {
        var hidden: Set<String> = []
        var seen: [String: Date] = [:]
        var surfaced: [String: Date] = [:]
        var birthdaysEnabled: Bool = true
        var generatedDay: Date?

        init(
            hidden: Set<String> = [],
            seen: [String: Date] = [:],
            surfaced: [String: Date] = [:],
            birthdaysEnabled: Bool = true,
            generatedDay: Date? = nil
        ) {
            self.hidden = hidden
            self.seen = seen
            self.surfaced = surfaced
            self.birthdaysEnabled = birthdaysEnabled
            self.generatedDay = generatedDay
        }

        var isEmpty: Bool {
            hidden.isEmpty
                && seen.isEmpty
                && surfaced.isEmpty
                && birthdaysEnabled
                && generatedDay == nil
        }

        init(_ record: MemoryStateStructure) {
            hidden = Set(record.hidden)
            seen = Dictionary(uniqueKeysWithValues: record.seen.compactMap { pair in
                MemoryLog.parseDate(pair.at).map { (pair.key, $0) }
            })
            surfaced = Dictionary(uniqueKeysWithValues: record.surfaced.compactMap { pair in
                MemoryLog.parseDate(pair.at).map { (pair.key, $0) }
            })
            birthdaysEnabled = record.birthdaysEnabled
            generatedDay = record.generatedDay.isEmpty ? nil : MemoryLog.parseDate(record.generatedDay)
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
        try memoryLogAppend(
            root: path(libraryRoot),
            device: device,
            eventType: type,
            bodyJson: bodyJSON
        )
    }

    /// One-shot import. Returns events written (0 if this device already has
    /// a `memory_migrated` marker). Throws `MemoryLogError` on FFI failure.
    @discardableResult
    static func migrate(
        libraryRoot: URL,
        device: String,
        snapshot: Snapshot
    ) throws -> Int {
        let n = try memoryLogMigrateFromSnapshot(
            root: path(libraryRoot),
            device: device,
            snapshotJson: snapshot.jsonString()
        )
        return Int(n)
    }

    static func project(libraryRoot: URL) throws -> State {
        State(try memoryLogProject(root: path(libraryRoot)))
    }

    static func projectReport(libraryRoot: URL) throws -> Projection {
        let report = try memoryLogProjectReport(root: path(libraryRoot))
        return Projection(
            state: State(report.state),
            tornTails: report.tornTails.map {
                TornTail(path: $0.path, offset: $0.offset, detail: $0.detail)
            }
        )
    }

    /// Compact JSON string. `<` `>` `&` stay literal (Go `SetEscapeHTML(false)`).
    static func jsonString(_ s: String) -> String {
        PersonLog.jsonString(s)
    }

    static func formatDate(_ date: Date) -> String {
        rfc3339.string(from: date)
    }

    static func parseDate(_ raw: String) -> Date? {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }
        if let date = rfc3339.date(from: trimmed) { return date }
        if let date = rfc3339Fractional.date(from: trimmed) { return date }
        if let date = isoFractional.date(from: trimmed) { return date }
        if let date = isoBasic.date(from: trimmed) { return date }
        if trimmed.count == 10, let date = dayFormatter.date(from: trimmed) {
            return Calendar.current.startOfDay(for: date)
        }
        if let secs = Double(trimmed), isNumericTimestamp(trimmed) {
            return Date(timeIntervalSince1970: secs)
        }
        return nil
    }

    private static let rfc3339: DateFormatter = {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        formatter.dateFormat = "yyyy-MM-dd'T'HH:mm:ssXXXXX"
        return formatter
    }()

    private static let rfc3339Fractional: DateFormatter = {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        formatter.dateFormat = "yyyy-MM-dd'T'HH:mm:ss.SSSSSSSSSXXXXX"
        return formatter
    }()

    /// Apple documents `ISO8601DateFormatter` as thread-safe; this matches
    /// `WidgetSnapshotExporter.dayKeyFormatter`.
    private nonisolated(unsafe) static let isoFractional: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        return formatter
    }()

    private nonisolated(unsafe) static let isoBasic: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime]
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        return formatter
    }()

    private static let dayFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = TimeZone.current
        formatter.dateFormat = "yyyy-MM-dd"
        return formatter
    }()

    private static func isNumericTimestamp(_ raw: String) -> Bool {
        var sawDigit = false
        var sawDot = false
        for (index, ch) in raw.enumerated() {
            if ch == "-" && index == 0 { continue }
            if ch == "." {
                if sawDot { return false }
                sawDot = true
                continue
            }
            guard ch.isASCII && ch.isNumber else { return false }
            sawDigit = true
        }
        return sawDigit
    }

    private static func path(_ url: URL) -> String {
        url.standardized.path
    }
}

/// Injectable log boundary. Production stays on the Rust FFI; unit tests can
/// force an append failure without relying on chmod behavior under CI.
@MainActor
struct MemoryLogBackend {
    var append: (
        _ libraryRoot: URL,
        _ device: String,
        _ type: String,
        _ body: [(String, MemoryLog.LogJSON)]
    ) throws -> Void
    var migrate: (
        _ libraryRoot: URL,
        _ device: String,
        _ snapshot: MemoryLog.Snapshot
    ) throws -> Int
    var project: (_ libraryRoot: URL) throws -> MemoryLog.Projection

    static let live = MemoryLogBackend(
        append: { root, device, type, body in
            try MemoryLog.append(libraryRoot: root, device: device, type: type, body: body)
        },
        migrate: { root, device, snapshot in
            try MemoryLog.migrate(libraryRoot: root, device: device, snapshot: snapshot)
        },
        project: { root in
            try MemoryLog.projectReport(libraryRoot: root)
        }
    )
}
