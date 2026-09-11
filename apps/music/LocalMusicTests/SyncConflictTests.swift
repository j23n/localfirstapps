import Foundation
import Testing
@testable import LocalMusic

/// Grammar pins from `core/localcore-conflict/fixtures/grammar/{valid,invalid}.txt`.
@Suite("SyncConflict")
struct SyncConflictTests {

    @Test("every valid grammar line is a conflict name")
    func validGrammarLines() {
        #expect(!Self.validNames.isEmpty)
        for name in Self.validNames {
            #expect(SyncConflict.isConflictName(name), "expected conflict: \(name)")
        }
    }

    @Test("every invalid grammar line is rejected")
    func invalidGrammarLines() {
        #expect(!Self.invalidNames.isEmpty)
        for name in Self.invalidNames {
            #expect(!SyncConflict.isConflictName(name), "expected survivor: \(name)")
        }
    }

    // MARK: - Fixture names

    /// `core/localcore-conflict/fixtures/grammar/valid.txt` (comments stripped).
    private static let validNames = [
        "IMG_1234.sync-conflict-20200901-120000-DEVICEABC.heic",
        "alice.sync-conflict-20200901-120000-DEVICEABC.vcf",
        "photo.jpg.sync-conflict-20200901-120000-DEVICEABC.xmp",
        "playlist.sync-conflict-20200901-120000-DEVICEABC.m3u",
        "photo.sync-conflict-20200901-120000-PHONE01.heic",
        "photo.sync-conflict-20200902-130000-LAPTOP02.heic",
        "photo.heic.sync-conflict-20200901-120000-PHONE01.xmp",
        "photo.heic.sync-conflict-20200903-090000-LAPTOP02.xmp",
        "alice.sync-conflict-20200901-120000-PHONE01.vcf",
        "alice.sync-conflict-20200901-140000-LAPTOP02.vcf",
        "playlist.sync-conflict-20200901-120000-PHONE01.m3u",
        "playlist.sync-conflict-20200904-160000-LAPTOP02.m3u",
        "note.sync-conflict-20200901-120000-A.vcf",
        "note.sync-conflict-20200901-120000-A.b_c-1.vcf",
        "dotted.name.v2.sync-conflict-20210102-030405-Dev_1.jpg",
        "IMG_1234.sync-conflict-20200901-120000-DEVICEABC.HEIC",
    ]

    /// `core/localcore-conflict/fixtures/grammar/invalid.txt` (comments stripped).
    private static let invalidNames = [
        "photo.sync-conflict.heic",
        "photo.conflict-20200901-120000-DEV.heic",
        "photo.jpg",
        ".sync-conflict-20200901-120000-DEV.heic",
        "photo.heic",
        "photo.heic.xmp",
        "alice.vcf",
        "playlist.m3u",
        "photo.sync-conflict-20200901.heic",
        "photo.sync-conflict-20200901-120000.heic",
        "photo.sync-conflict-20200901-120000-.heic",
        "sync-conflict-20200901-120000-DEV.heic",
        "photo.sync-conflict-2020901-120000-DEV.heic",
        "photo.sync-conflict-20200901-12000-DEV.heic",
        "photo.sync-conflict-20200901-120000--DEV.heic",
        "photo.sync-conflict-20200901-120000-.DEV.heic",
        "photo.sync-conflict-20200901-120000-_DEV.heic",
        "photo.SYNC-CONFLICT-20200901-120000-DEV.heic",
        "2026-09.ndjson",
    ]
}
