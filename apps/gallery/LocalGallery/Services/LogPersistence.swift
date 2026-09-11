import Foundation

/// Debounced flush of `LogStore.shared.asText` to disk. Capture is opt-in
/// (ADR 0007 R11); with crash reporting gone there is no toggle, so this
/// is a no-op until a later local-only log setting exists.
@MainActor
final class LogPersistence {
    static let shared = LogPersistence()

    var isEnabled: Bool { false }

    private init() {}

    func scheduleFlush() {}
}
