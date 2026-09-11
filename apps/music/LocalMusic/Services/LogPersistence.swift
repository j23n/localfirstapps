import Foundation

/// Debounced write of `LogStore.shared.asText` to disk. Capture is
/// opt-in (ADR 0007 R11); with crash reporting gone there is no toggle,
/// so instance methods are a no-op. The static helpers stay for tests.
@MainActor
final class LogPersistence {

    static let shared = LogPersistence()

    /// Maximum on-disk size of the log tail. After every flush, the file is
    /// truncated from the front to this byte count if it exceeds it.
    static let maxBytes = 500 * 1024

    /// Debounce window for coalescing burst writes (e.g. during a folder scan).
    static let debounceSeconds: UInt64 = 2

    private init() {}

    var isEnabled: Bool { false }

    func scheduleFlush() {}

    func flushNow() {}

    /// Atomically writes `text` to `url`, then truncates the file's leading
    /// bytes if it exceeds `maxBytes`. Exposed for tests.
    static func flush(text: String, to url: URL) {
        guard let data = text.data(using: .utf8) else { return }
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(),
                                                 withIntermediateDirectories: true)
        try? data.write(to: url, options: .atomic)
        truncateIfNeeded(at: url)
    }

    static func truncateIfNeeded(at url: URL) {
        guard let attrs = try? FileManager.default.attributesOfItem(atPath: url.path),
              let size = attrs[.size] as? Int,
              size > maxBytes,
              let data = try? Data(contentsOf: url) else { return }
        let tail = data.suffix(maxBytes)
        try? tail.write(to: url, options: .atomic)
    }
}
