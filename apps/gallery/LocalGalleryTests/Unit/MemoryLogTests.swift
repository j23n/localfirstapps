import Foundation
import XCTest
@testable import LocalGallery

/// Memory-chrome cutover: UserDefaults → `.gallery/log/<dev>/`.
@MainActor
final class MemoryLogTests: XCTestCase {
    private var defaults: UserDefaults!
    private var library: TempDir!
    private let today = date(2024, 6, 11)

    override func setUp() async throws {
        try await super.setUp()
        defaults = TestUserDefaults.make()
        library = TempDir.make()
    }

    override func tearDown() async throws {
        if let defaults {
            TestUserDefaults.cleanup(defaults)
        }
        defaults = nil
        library?.teardown()
        library = nil
        try await super.tearDown()
    }

    private func makeCoordinator(
        attached: Bool = true,
        log: MemoryLogBackend = .live
    ) -> MemoryCoordinator {
        let photos = [PhotoFile.fixture(url: URL(fileURLWithPath: "/lib/a.jpg"))]
        let index = CoreLibraryIndex()
        index.build(allPhotos: photos)
        let people = PeopleStore(
            defaults: defaults,
            clock: FixedClock(date: today),
            index: index
        )
        let coordinator = MemoryCoordinator(
            defaults: defaults,
            clock: FixedClock(date: today),
            cache: JSONDiskCache(
                url: library.appending("memories_cache.json"),
                version: 1,
                label: "test memories"
            ),
            index: index,
            people: people,
            log: log
        )
        if attached {
            _ = coordinator.attachLibrary(library.url, snapshot: .empty)
        }
        return coordinator
    }

    func testLegacyKeysAreRemovedAndCursorPreventsFutureDomainReads() {
        defaults.set(["mem-old"], forKey: "hiddenMemories")
        defaults.set(
            try! JSONEncoder().encode(["mem-old": today]),
            forKey: "seenMemoryIDs"
        )
        defaults.set(
            try! JSONEncoder().encode(["onThisDay": today]),
            forKey: "surfacedClusters"
        )
        defaults.set(false, forKey: "birthdayMemoriesEnabled")
        defaults.set(today, forKey: "memoriesGeneratedDay")

        MemoryLog.Snapshot.clearLegacy(from: defaults)

        for key in [
            "hiddenMemories", "seenMemoryIDs", "surfacedClusters",
            "birthdayMemoriesEnabled", "memoriesGeneratedDay",
        ] {
            XCTAssertNil(defaults.object(forKey: key), "\(key) survived cutover")
        }
        XCTAssertTrue(defaults.bool(forKey: MemoryLog.migrationCursorKey))

        defaults.set(["mem-stale"], forKey: "hiddenMemories")
        XCTAssertTrue(MemoryLog.Snapshot.legacy(in: defaults).isEmpty)
    }

    func testAttachMigratesThenRelaunchProjectsWithoutUserDefaults() throws {
        let root = TempDir.make()
        defer { root.teardown() }

        let coordinator = makeCoordinator(attached: false)
        let seenAt = date(2024, 6, 1)
        let surfacedAt = date(2024, 6, 10)
        let generated = Calendar.current.startOfDay(for: today)
        let snapshot = MemoryLog.Snapshot(
            hiddenMemories: ["onThisDay-2024-06-11"],
            seenMemoryIDs: ["onThisDay-2023-01-01": seenAt],
            surfacedClusters: ["onThisDay": surfacedAt],
            birthdayMemoriesEnabled: false,
            memoriesGeneratedDay: generated
        )

        let attached = coordinator.attachLibrary(root.url, snapshot: snapshot)
        XCTAssertTrue(attached.legacyMigrationComplete)
        XCTAssertEqual(coordinator.hiddenMemories, ["onThisDay-2024-06-11"])
        XCTAssertEqual(coordinator.seenMemoryIDs["onThisDay-2023-01-01"], seenAt)
        XCTAssertEqual(coordinator.surfacedClusters["onThisDay"], surfacedAt)
        XCTAssertFalse(coordinator.birthdaysEnabled)
        XCTAssertEqual(coordinator.generatedDay, generated)

        let logDir = root.url
            .appendingPathComponent(".gallery/log/\(coordinator.deviceId)", isDirectory: true)
        XCTAssertTrue(
            FileManager.default.fileExists(atPath: logDir.path),
            "migrate writes {library}/.gallery/log/<dev>/"
        )

        var projected = try MemoryLog.project(libraryRoot: root.url)
        XCTAssertEqual(projected.hidden, ["onThisDay-2024-06-11"])
        XCTAssertEqual(projected.seen["onThisDay-2023-01-01"], seenAt)
        XCTAssertEqual(projected.surfaced["onThisDay"], surfacedAt)
        XCTAssertFalse(projected.birthdaysEnabled)
        XCTAssertEqual(projected.generatedDay, generated)

        coordinator.hide("yearsAgo-5-2024-06-11")
        XCTAssertNil(defaults.object(forKey: "hiddenMemories"))
        projected = try MemoryLog.project(libraryRoot: root.url)
        XCTAssertEqual(
            projected.hidden,
            ["onThisDay-2024-06-11", "yearsAgo-5-2024-06-11"]
        )

        coordinator.markSeen("yearsAgo-5-2024-06-11")
        XCTAssertNil(defaults.object(forKey: "seenMemoryIDs"))
        projected = try MemoryLog.project(libraryRoot: root.url)
        XCTAssertEqual(projected.seen["yearsAgo-5-2024-06-11"], today)

        coordinator.birthdaysEnabled = true
        XCTAssertNil(defaults.object(forKey: "birthdayMemoriesEnabled"))
        projected = try MemoryLog.project(libraryRoot: root.url)
        XCTAssertTrue(projected.birthdaysEnabled)

        let relaunched = makeCoordinator(attached: false)
        XCTAssertTrue(
            relaunched.hiddenMemories.isEmpty,
            "no folder means no memory projection"
        )
        let again = relaunched.attachLibrary(root.url, snapshot: .empty)
        XCTAssertEqual(
            again.state.hidden,
            ["onThisDay-2024-06-11", "yearsAgo-5-2024-06-11"]
        )
        XCTAssertEqual(relaunched.seenMemoryIDs["yearsAgo-5-2024-06-11"], today)
        XCTAssertTrue(relaunched.birthdaysEnabled)
        XCTAssertNil(defaults.object(forKey: "hiddenMemories"))
        XCTAssertNil(defaults.object(forKey: "seenMemoryIDs"))
        XCTAssertNil(defaults.object(forKey: "surfacedClusters"))
        XCTAssertNil(defaults.object(forKey: "birthdayMemoriesEnabled"))
        XCTAssertNil(defaults.object(forKey: "memoriesGeneratedDay"))
    }

    func testGalleryStoreAttachImportsAndDeletesEveryLegacyMemoryField() throws {
        let seenAt = date(2024, 6, 1)
        let surfacedAt = date(2024, 6, 10)
        let generated = Calendar.current.startOfDay(for: today)
        let harness = TestGalleryStore.make(clock: FixedClock(date: today)) { defaults in
            defaults.set(["onThisDay-2024-06-11"], forKey: "hiddenMemories")
            defaults.set(
                try! JSONEncoder().encode(["onThisDay-2023-01-01": seenAt]),
                forKey: "seenMemoryIDs"
            )
            defaults.set(
                try! JSONEncoder().encode(["onThisDay": surfacedAt]),
                forKey: "surfacedClusters"
            )
            defaults.set(false, forKey: "birthdayMemoriesEnabled")
            defaults.set(generated, forKey: "memoriesGeneratedDay")
        }
        addTeardownBlock { harness.teardown() }

        harness.store.attachPersonLog(to: harness.tempDir.url)

        XCTAssertEqual(harness.store.memories.hiddenMemories, ["onThisDay-2024-06-11"])
        XCTAssertEqual(harness.store.memories.seenMemoryIDs["onThisDay-2023-01-01"], seenAt)
        XCTAssertEqual(harness.store.memories.surfacedClusters["onThisDay"], surfacedAt)
        XCTAssertFalse(harness.store.memories.birthdaysEnabled)
        XCTAssertEqual(harness.store.memories.generatedDay, generated)
        for key in [
            "hiddenMemories", "seenMemoryIDs", "surfacedClusters",
            "birthdayMemoriesEnabled", "memoriesGeneratedDay",
        ] {
            XCTAssertNil(harness.defaults.object(forKey: key), "\(key) survived attach")
        }
        XCTAssertTrue(harness.defaults.bool(forKey: MemoryLog.migrationCursorKey))
    }

    func testPruneSeenAndSurfacedAfterProject() {
        let coordinator = makeCoordinator(attached: false)
        let staleSeen = date(2023, 5, 1)
        let staleSurfaced = date(2024, 5, 1)
        let freshSeen = date(2024, 6, 1)
        let freshSurfaced = date(2024, 6, 10)
        _ = coordinator.attachLibrary(
            library.url,
            snapshot: MemoryLog.Snapshot(
                hiddenMemories: [],
                seenMemoryIDs: [
                    "old": staleSeen,
                    "fresh": freshSeen,
                ],
                surfacedClusters: [
                    "old": staleSurfaced,
                    "fresh": freshSurfaced,
                ],
                birthdayMemoriesEnabled: nil,
                memoriesGeneratedDay: nil
            )
        )
        XCTAssertNil(coordinator.seenMemoryIDs["old"])
        XCTAssertEqual(coordinator.seenMemoryIDs["fresh"], freshSeen)
        XCTAssertNil(coordinator.surfacedClusters["old"])
        XCTAssertEqual(coordinator.surfacedClusters["fresh"], freshSurfaced)
    }

    func testExistingMarkerMakesEmptyProjectionAuthoritative() throws {
        let root = TempDir.make()
        defer { root.teardown() }

        defaults.set(["stale"], forKey: "hiddenMemories")
        let coordinator = makeCoordinator(attached: false)
        XCTAssertEqual(coordinator.hiddenMemories, ["stale"])
        try writeMemoryMigratedMarker(library: root.url, device: coordinator.deviceId)

        let attached = coordinator.attachLibrary(
            root.url,
            snapshot: MemoryLog.Snapshot(
                hiddenMemories: ["stale"],
                seenMemoryIDs: [:],
                surfacedClusters: [:],
                birthdayMemoriesEnabled: false,
                memoriesGeneratedDay: today
            )
        )
        XCTAssertTrue(attached.legacyMigrationComplete)
        XCTAssertTrue(attached.state.hidden.isEmpty)
        XCTAssertTrue(coordinator.hiddenMemories.isEmpty)
        XCTAssertTrue(coordinator.birthdaysEnabled)
        XCTAssertNil(coordinator.generatedDay)
    }

    private func writeMemoryMigratedMarker(library: URL, device: String) throws {
        let dir = library.appendingPathComponent(".gallery/log/\(device)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let line =
            "{\"id\":\"01900000-0000-7000-8000-00000000ffff\",\"ts\":\"2024-07-01T10:00:00.000000000Z\",\"dev\":\(MemoryLog.jsonString(device)),\"type\":\"memory_migrated\",\"body\":{}}\n"
        try Data(line.utf8).write(to: dir.appendingPathComponent("2024-07.ndjson"))
    }
}
