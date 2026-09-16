import Foundation
import Testing
@testable import LocalMusic

final class MetadataLoaderTests {
    private let tempDir: URL

    init() throws {
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("MetadataLoaderTests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
    }

    deinit {
        try? FileManager.default.removeItem(at: tempDir)
    }

    @Test func hostMetadataPinsCoreSwiftIdentityParity() async throws {
        let url = tempDir.appendingPathComponent("Café.mp3")
        FileManager.default.createFile(atPath: url.path, contents: Data())
        let id = Track.stableID(for: url).uuidString.lowercased()
        let loaded = try await MetadataLoader.loadMetadata(
            for: MetadataRequest(id: id, path: url.path)
        )
        #expect(loaded.track.coreID == id)
        #expect(loaded.result.id == id)
        #expect(loaded.track.url.standardized.path == url.standardized.path)
    }

    @Test func hostMetadataRejectsIdentityMismatch() async {
        let url = tempDir.appendingPathComponent("song.mp3")
        do {
            _ = try await MetadataLoader.loadMetadata(
                for: MetadataRequest(
                    id: "00000000-0000-5000-8000-000000000000",
                    path: url.path
                )
            )
            Issue.record("expected identity mismatch")
        } catch let error as MetadataLoaderError {
            guard case .identityMismatch = error else {
                Issue.record("unexpected metadata error")
                return
            }
        } catch {
            Issue.record("unexpected error: \(error)")
        }
    }

    @Test func parseSYLT_utf8BasicLines() throws {
        var data = Self.sylTHeaderUTF8()
        Self.appendUTF8Line(into: &data, "Hello", timestampMs: 1_000)
        Self.appendUTF8Line(into: &data, "World", timestampMs: 2_500)
        let lines = try #require(MetadataLoader.parseSYLT(data: data))
        #expect(lines.map(\.text) == ["Hello", "World"])
        #expect(abs(lines[0].timestamp - 1.0) < 0.0001)
        #expect(abs(lines[1].timestamp - 2.5) < 0.0001)
    }

    @Test func parseSYLT_sortsAndFiltersBlankLines() throws {
        var data = Self.sylTHeaderUTF8()
        Self.appendUTF8Line(into: &data, "Second", timestampMs: 5_000)
        Self.appendUTF8Line(into: &data, "  ", timestampMs: 2_000)
        Self.appendUTF8Line(into: &data, "First", timestampMs: 1_000)
        let lines = try #require(MetadataLoader.parseSYLT(data: data))
        #expect(lines.map(\.text) == ["First", "Second"])
    }

    @Test func parseSYLT_utf16BigEndian() throws {
        var data = Data([0x02, 0x65, 0x6E, 0x67, 0x02, 0x00, 0x00, 0x00])
        Self.appendUTF16BELine(into: &data, "Yo", timestampMs: 250)
        let lines = try #require(MetadataLoader.parseSYLT(data: data))
        #expect(lines.first?.text == "Yo")
    }

    @Test func parseSYLT_shortOrTruncatedBufferIsSafe() throws {
        #expect(MetadataLoader.parseSYLT(data: Data()) == nil)
        var data = Self.sylTHeaderUTF8()
        Self.appendUTF8Line(into: &data, "Complete", timestampMs: 1_000)
        data.append(contentsOf: Array("Cut".utf8))
        data.append(contentsOf: [0x00, 0x00, 0x01])
        let lines = try #require(MetadataLoader.parseSYLT(data: data))
        #expect(lines.map(\.text) == ["Complete"])
    }

    private static func sylTHeaderUTF8() -> Data {
        Data([0x03, 0x65, 0x6E, 0x67, 0x02, 0x00, 0x00])
    }

    private static func appendUTF8Line(
        into data: inout Data,
        _ text: String,
        timestampMs: UInt32
    ) {
        data.append(contentsOf: text.utf8)
        data.append(0)
        appendBigEndianUInt32(&data, timestampMs)
    }

    private static func appendUTF16BELine(
        into data: inout Data,
        _ text: String,
        timestampMs: UInt32
    ) {
        if let encoded = text.data(using: .utf16BigEndian) {
            data.append(encoded)
        }
        data.append(contentsOf: [0, 0])
        appendBigEndianUInt32(&data, timestampMs)
    }

    private static func appendBigEndianUInt32(_ data: inout Data, _ value: UInt32) {
        data.append(UInt8((value >> 24) & 0xFF))
        data.append(UInt8((value >> 16) & 0xFF))
        data.append(UInt8((value >> 8) & 0xFF))
        data.append(UInt8(value & 0xFF))
    }
}
