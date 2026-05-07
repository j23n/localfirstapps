import Foundation

/// Debounced flush of `LogStore.shared.asText` to disk so crash reports can
/// ship a recent log tail. Gated behind `CrashDiagnosticsService.isEnabled` —
/// when the user hasn't opted in, every call is a no-op.
///
/// File is bounded to a 500 KB tail so a long-lived session doesn't grow it
/// unbounded; truncation runs at most once per flush, only on overflow.
@MainActor
final class LogPersistence {
    static let shared = LogPersistence()

    private static let maxFileSize = 500 * 1024
    private static let debounceNanoseconds: UInt64 = 2_000_000_000

    private var pendingFlush: Task<Void, Never>?

    var isEnabled: Bool { CrashDiagnosticsService.shared.isEnabled }

    private init() {}

    func scheduleFlush() {
        guard isEnabled else { return }
        pendingFlush?.cancel()
        pendingFlush = Task { [weak self] in
            try? await Task.sleep(nanoseconds: Self.debounceNanoseconds)
            if Task.isCancelled { return }
            self?.flush()
        }
    }

    private func flush() {
        let url = CrashDiagnosticsService.shared.logTailURL
        let text = LogStore.shared.asText
        guard let data = text.data(using: .utf8) else { return }
        do {
            try data.write(to: url, options: .atomic)
            if data.count > Self.maxFileSize {
                let tail = data.suffix(Self.maxFileSize)
                try Data(tail).write(to: url, options: .atomic)
            }
        } catch {
            // Persistence failure isn't actionable from code; leaving a stale
            // file is preferable to logging from inside the log path.
        }
    }
}
