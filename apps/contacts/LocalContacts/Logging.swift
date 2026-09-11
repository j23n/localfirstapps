import os

/// Thin wrapper around `os.Logger` that mirrors every call into
/// `LogStore.shared` so the in-app LogsView can surface them. The signature
/// matches the `os.Logger` methods we actually call (`debug` / `info` /
/// `notice` / `warning` / `error`) so existing call sites compile unchanged.
struct TeeLogger: Sendable {
    let logger: Logger
    let category: String

    func debug(_ message: @autoclosure () -> String) {
        let s = message()
        logger.debug("\(s, privacy: .public)")
        LogStore.shared.append(level: .debug, category: category, message: s)
    }

    func info(_ message: @autoclosure () -> String) {
        let s = message()
        logger.info("\(s, privacy: .public)")
        LogStore.shared.append(level: .info, category: category, message: s)
    }

    func notice(_ message: @autoclosure () -> String) {
        let s = message()
        logger.notice("\(s, privacy: .public)")
        LogStore.shared.append(level: .info, category: category, message: s)
    }

    func warning(_ message: @autoclosure () -> String) {
        let s = message()
        logger.warning("\(s, privacy: .public)")
        LogStore.shared.append(level: .warning, category: category, message: s)
    }

    func error(_ message: @autoclosure () -> String) {
        let s = message()
        logger.error("\(s, privacy: .public)")
        LogStore.shared.append(level: .error, category: category, message: s)
    }
}

enum Log {
    private static let subsystem = "localcontacts"

    private static func tee(_ category: String) -> TeeLogger {
        TeeLogger(logger: Logger(subsystem: subsystem, category: category), category: category)
    }

    static let store    = tee("store")
    static let parse    = tee("parse")
    static let bookmark = tee("bookmark")
    static let sync     = tee("sync")
    static let ui       = tee("ui")
    static let bg       = tee("background")
}
