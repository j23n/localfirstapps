import Foundation

/// Syncthing conflict-copy grammar, matching `localcore-conflict`
/// (`is_conflict_name` / ADR 0005 R7, ADR 0002 R7).
///
/// A file matching
/// `<base>.sync-conflict-<YYYYMMDD>-<HHMMSS>-<device>.<ext>` is never content.
enum SyncConflict {
    /// Syncthing inserts this marker immediately before the final extension.
    private static let marker = ".sync-conflict-"

    /// `true` when `name` is a Syncthing conflict copy.
    static func isConflictName(_ name: String) -> Bool {
        guard let markerRange = name.range(of: marker, options: .backwards) else {
            return false
        }
        let baseStem = name[..<markerRange.lowerBound]
        if baseStem.isEmpty {
            return false
        }
        return parseSuffix(String(name[markerRange.upperBound...]))
    }

    /// `YYYYMMDD-HHMMSS-<device>.<ext>`
    private static func parseSuffix(_ rest: String) -> Bool {
        // 8 date + '-' + 6 time + '-' + device + '.' + ext
        let bytes = Array(rest.utf8)
        if bytes.count < 8 + 1 + 6 + 1 + 1 + 1 + 1 {
            return false
        }
        if !isDigits(bytes[0..<8]) {
            return false
        }
        if bytes[8] != UInt8(ascii: "-") {
            return false
        }
        if !isDigits(bytes[9..<15]) {
            return false
        }
        if bytes[15] != UInt8(ascii: "-") {
            return false
        }
        let deviceAndExt = bytes[16...]
        guard let dot = deviceAndExt.lastIndex(of: UInt8(ascii: ".")) else {
            return false
        }
        let originDevice = deviceAndExt[..<dot]
        let ext = deviceAndExt[(dot + 1)...]
        if ext.isEmpty {
            return false
        }
        guard let device = String(bytes: originDevice, encoding: .utf8) else {
            return false
        }
        return isDeviceID(device)
    }

    private static func isDigits(_ bytes: ArraySlice<UInt8>) -> Bool {
        !bytes.isEmpty && bytes.allSatisfy { $0 >= UInt8(ascii: "0") && $0 <= UInt8(ascii: "9") }
    }

    /// Health event device ids: `[A-Za-z0-9][A-Za-z0-9._-]*`.
    private static func isDeviceID(_ s: String) -> Bool {
        var chars = s.makeIterator()
        guard let first = chars.next() else {
            return false
        }
        if !isDeviceIDStart(first) {
            return false
        }
        for c in chars {
            if !isDeviceIDContinue(c) {
                return false
            }
        }
        return true
    }

    private static func isDeviceIDStart(_ c: Character) -> Bool {
        c.isASCII && (c.isLetter || c.isNumber)
    }

    private static func isDeviceIDContinue(_ c: Character) -> Bool {
        isDeviceIDStart(c) || c == "." || c == "_" || c == "-"
    }
}
