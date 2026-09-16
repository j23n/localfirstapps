import Foundation
import XCTest
@testable import LocalGallery

/// ADR 0005 R19 survival coverage for M1. The committed fixture was written
/// before stable ids NFC-normalised path bytes and deliberately contains both
/// spellings of one visible photo.
@MainActor
final class PersistedStateMigrationTests: XCTestCase {
    private struct Envelope<T: Decodable>: Decodable {
        let version: Int
        let value: T
    }

    private var temp: TempDir!
    private var defaults: UserDefaults!

    override func setUp() async throws {
        try await super.setUp()
        temp = TempDir.make()
        defaults = TestUserDefaults.make()
    }

    override func tearDown() async throws {
        TestUserDefaults.cleanup(defaults)
        temp.teardown()
        defaults = nil
        temp = nil
        try await super.tearDown()
    }

    func testPreM1FixtureConvergesAndLeavesNoOrphanedDerivedState() throws {
        let paths = try stageFixture()
        let oldID = try XCTUnwrap(UUID(uuidString: "30D974E4-3610-5519-AC6E-C053AE920638"))
        let newID = try XCTUnwrap(UUID(uuidString: "49B2A4AE-2C81-5F0D-8050-1DDEC048CC9D"))
        let orphanID = try XCTUnwrap(UUID(uuidString: "AAAAAAAA-BBBB-5CCC-8DDD-EEEEEEEEEEEE"))

        try Data("jpeg".utf8).write(
            to: paths.thumbnailDir.appendingPathComponent(oldID.uuidString).appendingPathExtension("jpg")
        )
        try Data("stamp".utf8).write(
            to: paths.thumbnailDir.appendingPathComponent(oldID.uuidString).appendingPathExtension("stamp")
        )
        try Data("orphan".utf8).write(
            to: paths.thumbnailDir.appendingPathComponent(orphanID.uuidString).appendingPathExtension("jpg")
        )
        try Data("memory".utf8).write(to: paths.memoriesCacheURL)
        let widgetSentinel = try XCTUnwrap(paths.widgetDataDir).appendingPathComponent("index.json")
        try Data("widget".utf8).write(to: widgetSentinel)
        let authoritativeXMP = temp.appending("authoritative.xmp")
        try Data("authoritative".utf8).write(to: authoritativeXMP)

        defaults.set(
            ["People/Ada": oldID.uuidString],
            forKey: "featuredPhotoByPerson"
        )
        let oldFolder = "43BF3EF3-5CFF-562F-9261-CA8EC6DF4F7F"
        let newFolder = "AC701734-A924-5059-AE63-DD08D00904BC"
        defaults.set(["folder-\(oldFolder)"], forKey: "hiddenMemories")

        let outcome = try PersistedStateMigration.run(paths: paths, defaults: defaults)

        XCTAssertTrue(outcome.migrated)
        XCTAssertEqual(outcome.photoIDChanges, 1)
        XCTAssertEqual(outcome.duplicatePhotosRemoved, 1)
        XCTAssertEqual(
            defaults.integer(forKey: PersistedStateMigration.markerKey),
            PersistedStateMigration.currentVersion
        )

        let snapshot = try loadLibrary(at: paths.libraryCacheURL)
        XCTAssertEqual(snapshot.allPhotos.map(\.id), [newID])
        XCTAssertEqual(snapshot.rootFolder.id.uuidString, newFolder)
        XCTAssertEqual(snapshot.rootFolder.photos.map(\.id), [newID])
        XCTAssertEqual(snapshot.rootFolder.totalPhotoCount, 1)
        XCTAssertEqual(snapshot.sidecarManifest?.map(\.photoID), [newID])
        XCTAssertEqual(snapshot.allPhotos[0].hierarchicalTags.map(\.fullPath), ["People/Ada"])

        let sidecarKeys = try cacheKeys(at: paths.sidecarCacheURL)
        XCTAssertEqual(sidecarKeys, Set([newID.uuidString]))
        XCTAssertFalse(
            FileManager.default.fileExists(
                atPath: paths.thumbnailDir.appendingPathComponent(oldID.uuidString)
                    .appendingPathExtension("jpg").path
            )
        )
        XCTAssertTrue(
            FileManager.default.fileExists(
                atPath: paths.thumbnailDir.appendingPathComponent(newID.uuidString)
                    .appendingPathExtension("jpg").path
            )
        )
        XCTAssertFalse(
            FileManager.default.fileExists(
                atPath: paths.thumbnailDir.appendingPathComponent(orphanID.uuidString)
                    .appendingPathExtension("jpg").path
            )
        )
        XCTAssertFalse(FileManager.default.fileExists(atPath: paths.memoriesCacheURL.path))
        XCTAssertFalse(FileManager.default.fileExists(atPath: widgetSentinel.path))
        XCTAssertEqual(try Data(contentsOf: authoritativeXMP), Data("authoritative".utf8))
        XCTAssertEqual(
            (defaults.dictionary(forKey: "featuredPhotoByPerson") as? [String: String])?["People/Ada"],
            newID.uuidString
        )
        XCTAssertEqual(
            defaults.array(forKey: "hiddenMemories") as? [String],
            ["folder-\(newFolder)"]
        )
    }

    func testInterruptionAfterLibraryCommitReplaysIdempotentlyOnRelaunch() throws {
        let paths = try stageFixture()

        XCTAssertThrowsError(
            try PersistedStateMigration.run(
                paths: paths,
                defaults: defaults,
                interruptAfter: .libraryCache
            )
        ) { error in
            XCTAssertEqual(
                error as? PersistedStateMigration.MigrationError,
                .interrupted(after: .libraryCache)
            )
        }
        XCTAssertEqual(defaults.integer(forKey: PersistedStateMigration.markerKey), 0)
        XCTAssertEqual(try loadLibrary(at: paths.libraryCacheURL).allPhotos.count, 1)

        let relaunched = try PersistedStateMigration.run(paths: paths, defaults: defaults)
        XCTAssertTrue(relaunched.migrated)
        XCTAssertEqual(try loadLibrary(at: paths.libraryCacheURL).allPhotos.count, 1)
        XCTAssertEqual(
            try cacheKeys(at: paths.sidecarCacheURL),
            Set(["49B2A4AE-2C81-5F0D-8050-1DDEC048CC9D"])
        )

        let noOp = try PersistedStateMigration.run(paths: paths, defaults: defaults)
        XCTAssertFalse(noOp.migrated, "the durable marker makes later launches a no-op")
    }

    private func stageFixture() throws -> GalleryPaths {
        let bundle = Bundle(for: Self.self)
        let libraryFixture = try XCTUnwrap(
            bundle.url(forResource: "library_cache_pre_m1", withExtension: "json")
        )
        let sidecarFixture = try XCTUnwrap(
            bundle.url(forResource: "sidecar_cache_pre_m1", withExtension: "json")
        )
        let library = temp.appending("library_cache.json")
        let sidecars = temp.appending("sidecar_cache.json")
        let thumbnails = temp.appending("thumbnails", isDirectory: true)
        let widgets = temp.appending("WidgetData", isDirectory: true)
        try FileManager.default.copyItem(at: libraryFixture, to: library)
        try FileManager.default.copyItem(at: sidecarFixture, to: sidecars)
        try FileManager.default.createDirectory(at: thumbnails, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: widgets, withIntermediateDirectories: true)
        return GalleryPaths(
            libraryCacheURL: library,
            memoriesCacheURL: temp.appending("memories_cache.json"),
            sidecarCacheURL: sidecars,
            thumbnailDir: thumbnails,
            mlCacheDatabaseURL: temp.appending("gallery-cache.sqlite"),
            modelPacksDirectoryURL: temp.appending("ModelPacks", isDirectory: true),
            bundledModelPackURL: nil,
            geocodeCacheURL: temp.appending("geocode-cache.json"),
            widgetDataDir: widgets,
            bookmarkKey: "rootFolderBookmark"
        )
    }

    private func loadLibrary(at url: URL) throws -> LibrarySnapshot {
        try JSONDecoder().decode(
            Envelope<LibrarySnapshot>.self,
            from: Data(contentsOf: url)
        ).value
    }

    private func cacheKeys(at url: URL) throws -> Set<String> {
        let object = try XCTUnwrap(
            try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any]
        )
        let value = try XCTUnwrap(object["value"] as? [String: Any])
        return Set(value.keys)
    }
}
