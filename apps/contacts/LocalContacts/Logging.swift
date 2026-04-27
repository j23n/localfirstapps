import os

enum Log {
    private static let subsystem = "localcontacts"

    static let store    = Logger(subsystem: subsystem, category: "store")
    static let parse    = Logger(subsystem: subsystem, category: "parse")
    static let bookmark = Logger(subsystem: subsystem, category: "bookmark")
    static let sync     = Logger(subsystem: subsystem, category: "sync")
    static let ui       = Logger(subsystem: subsystem, category: "ui")
}
