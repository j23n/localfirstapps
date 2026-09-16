import Foundation
import Testing
@testable import LocalMusic

/// Pins the generated core binding to the shared conflict grammar fixtures.
@Suite("MusicCore conflict grammar")
struct SyncConflictTests {

    @Test("every valid grammar line is a conflict name")
    func validGrammarLines() throws {
        let names = try Self.grammarLines(named: "valid.txt")
        #expect(!names.isEmpty)
        for name in names {
            #expect(isConflictName(name: name), "expected conflict: \(name)")
        }
    }

    @Test("every invalid grammar line is rejected")
    func invalidGrammarLines() throws {
        let names = try Self.grammarLines(named: "invalid.txt")
        #expect(!names.isEmpty)
        for name in names {
            #expect(!isConflictName(name: name), "expected survivor: \(name)")
        }
    }

    private static func grammarLines(named file: String) throws -> [String] {
        let root = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let url = root
            .appendingPathComponent("core/localcore-conflict/fixtures/grammar")
            .appendingPathComponent(file)
        let text = try String(contentsOf: url, encoding: .utf8)
        return text
            .split(whereSeparator: \.isNewline)
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty && !$0.hasPrefix("#") }
            .map { String($0) }
    }
}
