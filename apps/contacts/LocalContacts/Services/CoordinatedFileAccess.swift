import Foundation

/// Thin Sendable wrapper around `NSFileCoordinator` so folder I/O plays
/// nicely with Syncthing / iCloud / other coordinated writers. Errors
/// propagate; nothing is swallowed.
struct CoordinatedFileAccess: Sendable {

    func read(from url: URL) throws -> Data {
        try coordinateRead(at: url) { coordinatedURL in
            try Data(contentsOf: coordinatedURL)
        }
    }

    func write(_ data: Data, to url: URL) throws {
        try coordinateWrite(at: url, options: .forReplacing) { coordinatedURL in
            try data.write(to: coordinatedURL, options: .atomic)
        }
    }

    func delete(_ url: URL) throws {
        try coordinateWrite(at: url, options: .forDeleting) { coordinatedURL in
            guard FileManager.default.fileExists(atPath: coordinatedURL.path) else { return }
            try FileManager.default.removeItem(at: coordinatedURL)
        }
    }

    func contentsOfDirectory(at url: URL) throws -> [URL] {
        try coordinateRead(at: url) { coordinatedURL in
            try FileManager.default.contentsOfDirectory(
                at: coordinatedURL,
                includingPropertiesForKeys: [.isRegularFileKey],
                options: [.skipsHiddenFiles]
            )
        }
    }

    private func coordinateRead<T>(at url: URL, body: (URL) throws -> T) throws -> T {
        var coordinatorError: NSError?
        var result: Result<T, Error>?
        let coordinator = NSFileCoordinator()
        coordinator.coordinate(readingItemAt: url, options: [], error: &coordinatorError) { coordinatedURL in
            result = Result { try body(coordinatedURL) }
        }
        if let coordinatorError { throw coordinatorError }
        guard let result else { throw CoordinatedFileAccessError.coordinationFailed }
        return try result.get()
    }

    private func coordinateWrite(
        at url: URL,
        options: NSFileCoordinator.WritingOptions,
        body: (URL) throws -> Void
    ) throws {
        var coordinatorError: NSError?
        var result: Result<Void, Error>?
        let coordinator = NSFileCoordinator()
        coordinator.coordinate(writingItemAt: url, options: options, error: &coordinatorError) { coordinatedURL in
            result = Result { try body(coordinatedURL) }
        }
        if let coordinatorError { throw coordinatorError }
        guard let result else { throw CoordinatedFileAccessError.coordinationFailed }
        try result.get()
    }
}

enum CoordinatedFileAccessError: LocalizedError {
    case coordinationFailed

    var errorDescription: String? {
        switch self {
        case .coordinationFailed: "File coordination failed."
        }
    }
}
