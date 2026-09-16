import Foundation
import XCTest
@testable import LocalGallery

/// The people domain's persisted state across a rename.
///
/// Every projected decision is keyed by a person's **tag path**, and the rescan
/// that follows a rename cannot tell a renamed person from a new one — so a
/// rename that does not migrate them loses the user's "me" person, their pins,
/// their hidden set and their cover photos, silently, with nothing to notice
/// until they go looking. That is what these cases pin.
@MainActor
final class PeopleStoreTests: XCTestCase {
    private var defaults: UserDefaults!
    private var library: TempDir!

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

    private func makeStore(
        attached: Bool = true,
        log: PersonLogBackend = .live
    ) -> PeopleStore {
        let store = PeopleStore(
            defaults: defaults,
            clock: SystemClock(),
            index: CoreLibraryIndex(),
            log: log
        )
        if attached {
            _ = store.attachLibrary(library.url, snapshot: .empty)
        }
        return store
    }

    private func person(_ name: String, count: Int = 1) -> TagSuggestion {
        TagSuggestion(
            id: "people/\(name.lowercased())",
            displayName: name,
            fullPath: "People/\(name)",
            namespace: "People",
            count: count
        )
    }

    // MARK: - Projected fields

    func testRenameCarriesEveryPersistedDecisionToTheNewPath() {
        let store = makeStore()
        let photo = UUID()
        store.hidePerson("People/Anna")
        store.unhidePerson("People/Anna")
        store.hidePerson("People/Anna")
        store.toggleFeaturePerson("People/Bob")
        store.toggleFeaturePerson("People/Anna")
        store.toggleFeaturePerson("People/Cy")
        store.setFeaturedPhoto(personPath: "People/Anna", photoID: photo)
        store.markAsMe("People/Anna")
        store.setContactLink(
            personPath: "People/Anna",
            link: .manual(contactID: "CN:anna")
        )

        store.renamePerson(from: "People/Anna", to: "People/Anna Schmidt")

        XCTAssertEqual(store.hiddenPeople, ["People/Anna Schmidt"])
        XCTAssertEqual(
            store.featuredPeople,
            ["People/Bob", "People/Anna Schmidt", "People/Cy"],
            "the feature order is the user's, and a rename is not a reordering"
        )
        XCTAssertEqual(store.featuredPhotoByPerson["People/Anna Schmidt"], photo)
        XCTAssertNil(store.featuredPhotoByPerson["People/Anna"])
        XCTAssertEqual(store.mePersonPath, "People/Anna Schmidt")
        XCTAssertEqual(
            store.personContactLinks["People/Anna Schmidt"],
            .manual(contactID: "CN:anna")
        )
        XCTAssertNil(store.personContactLinks["People/Anna"])
    }

    /// The rename event has to survive the process, not just the object.
    func testTheMigratedStateIsWhatTheNextLaunchProjects() {
        let store = makeStore()
        store.hidePerson("People/Anna")
        store.toggleFeaturePerson("People/Anna")
        store.setFeaturedPhoto(personPath: "People/Anna", photoID: UUID())
        store.markAsMe("People/Anna")

        store.renamePerson(from: "People/Anna", to: "People/Ada")

        let relaunched = makeStore()
        XCTAssertEqual(relaunched.hiddenPeople, ["People/Ada"])
        XCTAssertEqual(relaunched.featuredPeople, ["People/Ada"])
        XCTAssertNotNil(relaunched.featuredPhotoByPerson["People/Ada"])
        XCTAssertEqual(relaunched.mePersonPath, "People/Ada")
    }

    func testRenamingToTheSameNameChangesNothing() {
        let store = makeStore()
        store.toggleFeaturePerson("People/Anna")
        store.markAsMe("People/Anna")

        store.renamePerson(from: "People/Anna", to: "People/Anna")

        XCTAssertEqual(store.featuredPeople, ["People/Anna"])
        XCTAssertEqual(store.mePersonPath, "People/Anna")
    }

    func testAPersonWithNoPersistedStateRenamesWithoutInventingAny() {
        let store = makeStore()
        store.toggleFeaturePerson("People/Bob")

        store.renamePerson(from: "People/Anna", to: "People/Ada")

        XCTAssertTrue(store.hiddenPeople.isEmpty)
        XCTAssertEqual(store.featuredPeople, ["People/Bob"], "an unrelated person was touched")
        XCTAssertTrue(store.featuredPhotoByPerson.isEmpty)
        XCTAssertEqual(store.mePersonPath, "")
    }

    // MARK: - Collisions

    /// Renaming onto an existing person is a merge at the name level, so both
    /// sides can carry the same decision. Featuring twice would render as two
    /// identical rail entries.
    func testRenamingOntoAFeaturedPersonLeavesOnePin() {
        let store = makeStore()
        store.toggleFeaturePerson("People/Bob")
        store.toggleFeaturePerson("People/Anna")
        store.toggleFeaturePerson("People/Anna Schmidt")

        store.renamePerson(from: "People/Anna", to: "People/Anna Schmidt")

        XCTAssertEqual(
            store.featuredPeople,
            ["People/Bob", "People/Anna Schmidt"],
            "the survivor keeps the older position, and nobody is pinned twice"
        )
    }

    /// Both sides hidden collapses to one entry, which is what a set is for —
    /// and the result must still be hidden, not quietly back on the rail.
    func testRenamingOntoAHiddenPersonLeavesThemHidden() {
        let store = makeStore()
        store.hidePerson("People/Anna")
        store.hidePerson("People/Anna Schmidt")

        store.renamePerson(from: "People/Anna", to: "People/Anna Schmidt")

        XCTAssertEqual(store.hiddenPeople, ["People/Anna Schmidt"])
    }

    /// Both sides have a cover photo and only one can survive. The name the
    /// user just chose is the one whose choice is kept.
    func testACoverPhotoCollisionKeepsTheNewNamesChoice() {
        let store = makeStore()
        let oldCover = UUID()
        let newCover = UUID()
        store.setFeaturedPhoto(personPath: "People/Anna", photoID: oldCover)
        store.setFeaturedPhoto(personPath: "People/Anna Schmidt", photoID: newCover)

        store.renamePerson(from: "People/Anna", to: "People/Anna Schmidt")

        XCTAssertEqual(store.featuredPhotoByPerson["People/Anna Schmidt"], newCover)
        XCTAssertNil(store.featuredPhotoByPerson["People/Anna"], "the old entry was left behind")
        XCTAssertEqual(store.featuredPhotoByPerson.count, 1)
    }

    /// The rail is what the user actually sees the result in: one entry, under
    /// the new name, still ahead of the people who were never featured.
    func testTheRailShowsOnePersonAfterACollidingRename() {
        let store = makeStore()
        store.updateTopPeople([person("Anna Schmidt", count: 9), person("Bob", count: 4)])
        store.toggleFeaturePerson("People/Anna")
        store.toggleFeaturePerson("People/Anna Schmidt")

        store.renamePerson(from: "People/Anna", to: "People/Anna Schmidt")

        XCTAssertEqual(
            store.visiblePeople.map(\.fullPath),
            ["People/Anna Schmidt", "People/Bob"]
        )
    }

    // MARK: - Side effects

    /// Trip titles depend on the "me" person and the memory rail on the hidden
    /// set, so a rename that moves either has to say so.
    func testRenamingNotifiesTheStoreThatItsDerivedStateMoved() {
        let store = makeStore()
        store.markAsMe("People/Anna")
        var widgetExports = 0
        store.onWidgetAffectingChange = { widgetExports += 1 }

        store.renamePerson(from: "People/Anna", to: "People/Ada")

        XCTAssertGreaterThan(widgetExports, 0)
    }

    // MARK: - Attach / migrate / project

    func testDeviceIdIsAsciiLikeRust() {
        XCTAssertTrue(PersonLog.isValidDevice("ios"))
        XCTAssertTrue(PersonLog.isValidDevice("instinct-1"))
        XCTAssertTrue(PersonLog.isValidDevice("a.b_c-9"))
        XCTAssertFalse(PersonLog.isValidDevice(""))
        XCTAssertFalse(PersonLog.isValidDevice(".."))
        XCTAssertFalse(PersonLog.isValidDevice("a/b"))
        XCTAssertFalse(PersonLog.isValidDevice("Adaé"))
        XCTAssertFalse(PersonLog.isValidDevice("éAda"))
        XCTAssertFalse(PersonLog.isValidDevice("Ångstrom"))
    }

    func testLegacyKeysAreRemovedAndCursorPreventsFutureDomainReads() {
        defaults.set(["People/Old"], forKey: "hiddenPeople")
        defaults.set(["People/Old"], forKey: "pinnedPeople")
        defaults.set(["People/Old": UUID().uuidString], forKey: "featuredPhotoByPerson")
        defaults.set("People/Old", forKey: "mePersonPath")
        defaults.set(Data("not-json".utf8), forKey: "personContactLinks")

        PersonLog.Snapshot.clearLegacy(from: defaults)

        for key in [
            "hiddenPeople", "pinnedPeople", "featuredPhotoByPerson",
            "mePersonPath", "personContactLinks",
        ] {
            XCTAssertNil(defaults.object(forKey: key), "\(key) survived cutover")
        }
        XCTAssertTrue(defaults.bool(forKey: PersonLog.migrationCursorKey))

        // Even if a downgrade/restore later recreates an obsolete field, the
        // cutover build consults only its migration cursor.
        defaults.set(["People/Stale"], forKey: "hiddenPeople")
        XCTAssertTrue(PersonLog.Snapshot.legacy(in: defaults).isEmpty)
    }

    func testGalleryStoreAttachImportsAndDeletesEveryLegacyDomainField() throws {
        let cover = UUID()
        let harness = TestGalleryStore.make { defaults in
            defaults.set(["People/Hidden"], forKey: "hiddenPeople")
            defaults.set(["People/Ada"], forKey: "pinnedPeople")
            defaults.set(
                ["People/Ada": cover.uuidString],
                forKey: "featuredPhotoByPerson"
            )
            defaults.set("People/Ada", forKey: "mePersonPath")
            defaults.set(
                try! JSONEncoder().encode([
                    "People/Ada": PersonLink.manual(contactID: "CN:ada"),
                ]),
                forKey: "personContactLinks"
            )
        }
        addTeardownBlock { harness.teardown() }

        harness.store.attachPersonLog(to: harness.tempDir.url)

        XCTAssertEqual(harness.store.people.hiddenPeople, ["People/Hidden"])
        XCTAssertEqual(harness.store.people.featuredPeople, ["People/Ada"])
        XCTAssertEqual(harness.store.people.featuredPhotoByPerson["People/Ada"], cover)
        XCTAssertEqual(harness.store.people.mePersonPath, "People/Ada")
        XCTAssertEqual(
            harness.store.personContactLinks["People/Ada"],
            .manual(contactID: "CN:ada")
        )
        for key in [
            "hiddenPeople", "pinnedPeople", "featuredPhotoByPerson",
            "mePersonPath", "personContactLinks",
        ] {
            XCTAssertNil(harness.defaults.object(forKey: key), "\(key) survived attach")
        }
        XCTAssertTrue(harness.defaults.bool(forKey: PersonLog.migrationCursorKey))
    }

    func testAttachMigratesThenRelaunchProjectsWithoutUserDefaults() throws {
        let root = TempDir.make()
        defer { root.teardown() }

        let store = makeStore(attached: false)
        let photo = UUID()
        let links: [String: PersonLink] = [
            "People/Ada": .manual(contactID: "CN:ada"),
        ]
        let snapshot = PersonLog.Snapshot(
            hiddenPeople: ["People/Anna"],
            pinnedPeople: ["People/Ada"],
            featuredPhotoByPerson: ["People/Ada": photo.uuidString],
            mePersonPath: "People/Ada",
            personContactLinks: links
        )

        let attached = store.attachLibrary(root.url, snapshot: snapshot)
        XCTAssertTrue(attached.legacyMigrationComplete)
        XCTAssertEqual(store.hiddenPeople, ["People/Anna"])
        XCTAssertEqual(store.featuredPeople, ["People/Ada"])
        XCTAssertEqual(store.featuredPhotoByPerson["People/Ada"], photo)
        XCTAssertEqual(store.mePersonPath, "People/Ada")
        XCTAssertEqual(store.personContactLinks, links)

        let logDir = root.url
            .appendingPathComponent(".gallery/log/\(store.deviceId)", isDirectory: true)
        XCTAssertTrue(
            FileManager.default.fileExists(atPath: logDir.path),
            "migrate writes {library}/.gallery/log/<dev>/"
        )

        var projected = try PersonLog.project(libraryRoot: root.url)
        XCTAssertEqual(projected.hidden, ["People/Anna"])
        XCTAssertEqual(projected.featured, ["People/Ada"])
        XCTAssertEqual(projected.me, "People/Ada")
        XCTAssertEqual(projected.featuredPhoto["People/Ada"], photo.uuidString)
        XCTAssertEqual(projected.links["People/Ada"], "CN:ada")

        store.toggleFeaturePerson("People/Cy")
        XCTAssertNil(defaults.object(forKey: "pinnedPeople"))
        projected = try PersonLog.project(libraryRoot: root.url)
        XCTAssertEqual(projected.featured, ["People/Ada", "People/Cy"])

        store.renamePerson(from: "People/Anna", to: "People/Ann")
        XCTAssertEqual(store.hiddenPeople, ["People/Ann"])
        XCTAssertNil(defaults.object(forKey: "hiddenPeople"))
        projected = try PersonLog.project(libraryRoot: root.url)
        XCTAssertEqual(projected.hidden, ["People/Ann"])
        XCTAssertFalse(projected.hidden.contains("People/Anna"))

        let relaunched = makeStore(attached: false)
        XCTAssertTrue(relaunched.hiddenPeople.isEmpty, "no folder means no person projection")
        let again = relaunched.attachLibrary(
            root.url,
            snapshot: .empty
        )
        XCTAssertEqual(again.state.hidden, ["People/Ann"])
        XCTAssertEqual(relaunched.featuredPeople, ["People/Ada", "People/Cy"])
        XCTAssertEqual(relaunched.personContactLinks["People/Ada"], .manual(contactID: "CN:ada"))
    }

    func testExistingMarkerMakesEmptyProjectionAuthoritative() throws {
        let root = TempDir.make()
        defer { root.teardown() }

        defaults.set(["People/Stale"], forKey: "hiddenPeople")
        let store = makeStore(attached: false)
        try writePersonMigratedMarker(library: root.url, device: store.deviceId)

        let attached = store.attachLibrary(
            root.url,
            snapshot: PersonLog.Snapshot(
                hiddenPeople: ["People/Stale"],
                pinnedPeople: ["People/Stale"],
                featuredPhotoByPerson: [:],
                mePersonPath: "People/Stale",
                personContactLinks: [:]
            )
        )
        XCTAssertTrue(attached.legacyMigrationComplete)
        XCTAssertTrue(attached.state.isEmpty)
        XCTAssertTrue(store.hiddenPeople.isEmpty)
        XCTAssertTrue(store.featuredPeople.isEmpty)
        XCTAssertEqual(store.mePersonPath, "")
    }

    func testAppendFailureDoesNotPublishUncommittedState() throws {
        enum Expected: Error { case append }
        let live = PersonLogBackend.live
        let failing = PersonLogBackend(
            append: { _, _, _, _ in throw Expected.append },
            migrate: live.migrate,
            project: live.project
        )
        let store = makeStore(attached: false, log: failing)
        _ = store.attachLibrary(library.url, snapshot: .empty)

        XCTAssertFalse(store.hidePerson("People/Ada"))
        XCTAssertTrue(store.hiddenPeople.isEmpty)
        XCTAssertTrue(try PersonLog.project(libraryRoot: library.url).hidden.isEmpty)
        XCTAssertTrue(store.diagnostics.contains {
            if case .appendFailed(operation: "person_hidden", detail: _) = $0 { return true }
            return false
        })
    }

    func testTornTailSurfacesRecoveredProjectionWithoutStaleFallback() throws {
        let root = TempDir.make()
        defer { root.teardown() }
        let store = makeStore(attached: false)
        let dir = root.url.appendingPathComponent(
            ".gallery/log/\(store.deviceId)",
            isDirectory: true
        )
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let complete =
            "{\"id\":\"01900000-0000-7000-8000-00000000fff1\","
            + "\"ts\":\"2024-07-01T10:00:00.000000000Z\","
            + "\"dev\":\(PersonLog.jsonString(store.deviceId)),"
            + "\"type\":\"person_hidden\",\"body\":{\"path\":\"People/Recovered\"}}\n"
        try Data((complete + "{\"id\":\"torn\"").utf8).write(
            to: dir.appendingPathComponent("2024-07.ndjson")
        )

        let attached = store.attachLibrary(
            root.url,
            snapshot: PersonLog.Snapshot(
                hiddenPeople: ["People/Stale Defaults"],
                pinnedPeople: [],
                featuredPhotoByPerson: [:],
                mePersonPath: "",
                personContactLinks: [:]
            )
        )

        XCTAssertFalse(attached.legacyMigrationComplete)
        XCTAssertEqual(store.hiddenPeople, ["People/Recovered"])
        XCTAssertFalse(store.hiddenPeople.contains("People/Stale Defaults"))
        XCTAssertTrue(store.diagnostics.contains {
            if case .tornTail(path: _, offset: _, detail: _) = $0 { return true }
            return false
        })
    }

    private func writePersonMigratedMarker(library: URL, device: String) throws {
        let dir = library.appendingPathComponent(".gallery/log/\(device)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let line =
            "{\"id\":\"01900000-0000-7000-8000-00000000ffff\",\"ts\":\"2024-07-01T10:00:00.000000000Z\",\"dev\":\(PersonLog.jsonString(device)),\"type\":\"person_migrated\",\"body\":{}}\n"
        try Data(line.utf8).write(to: dir.appendingPathComponent("2024-07.ndjson"))
    }
}
