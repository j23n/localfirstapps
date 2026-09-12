import Foundation
import XCTest
@testable import LocalGallery

/// The people domain's persisted state across a rename.
///
/// Every key `PeopleStore` writes is a person's **tag path**, and the rescan
/// that follows a rename cannot tell a renamed person from a new one — so a
/// rename that does not migrate them loses the user's "me" person, their pins,
/// their hidden set and their cover photos, silently, with nothing to notice
/// until they go looking. That is what these cases pin.
@MainActor
final class PeopleStoreTests: XCTestCase {
    private var defaults: UserDefaults!

    override func setUp() {
        super.setUp()
        defaults = TestUserDefaults.make()
    }

    override func tearDown() {
        TestUserDefaults.cleanup(defaults)
        defaults = nil
        super.tearDown()
    }

    private func makeStore() -> PeopleStore {
        PeopleStore(defaults: defaults, clock: SystemClock(), index: CoreLibraryIndex())
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

    // MARK: - The four keys

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
    }

    /// The migration has to survive the process, not just the object — every
    /// one of these is written through a `didSet` into UserDefaults.
    func testTheMigratedStateIsWhatTheNextLaunchReads() {
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

    func testAttachMigratesDualWritesRenameAndProjects() throws {
        let library = TempDir.make()
        defer { library.teardown() }

        let store = makeStore()
        let photo = UUID()
        store.hidePerson("People/Anna")
        store.toggleFeaturePerson("People/Ada")
        store.setFeaturedPhoto(personPath: "People/Ada", photoID: photo)
        store.markAsMe("People/Ada")
        let links: [String: PersonLink] = [
            "People/Ada": .manual(contactID: "CN:ada"),
        ]
        let snap = snapshot(of: store, links: links)

        let state = store.attachLibrary(library.url, snapshot: snap)
        XCTAssertNotNil(state)
        XCTAssertEqual(store.hiddenPeople, ["People/Anna"])
        XCTAssertEqual(store.featuredPeople, ["People/Ada"])
        XCTAssertEqual(store.featuredPhotoByPerson["People/Ada"], photo)
        XCTAssertEqual(store.mePersonPath, "People/Ada")
        XCTAssertEqual(store.personContactLinks, links)

        let logDir = library.url
            .appendingPathComponent(".gallery/log/\(store.deviceId)", isDirectory: true)
        XCTAssertTrue(
            FileManager.default.fileExists(atPath: logDir.path),
            "migrate writes {library}/.gallery/log/<dev>/"
        )

        var projected = try PersonLog.project(libraryRoot: library.url)
        XCTAssertEqual(projected.hidden, ["People/Anna"])
        XCTAssertEqual(projected.featured, ["People/Ada"])
        XCTAssertEqual(projected.me, "People/Ada")
        XCTAssertEqual(projected.featuredPhoto["People/Ada"], photo.uuidString)
        XCTAssertEqual(projected.links["People/Ada"], "CN:ada")

        store.toggleFeaturePerson("People/Cy")
        XCTAssertEqual(defaults.array(forKey: "pinnedPeople") as? [String], ["People/Ada", "People/Cy"])
        projected = try PersonLog.project(libraryRoot: library.url)
        XCTAssertEqual(projected.featured, ["People/Ada", "People/Cy"])

        store.renamePerson(from: "People/Anna", to: "People/Ann")
        XCTAssertEqual(store.hiddenPeople, ["People/Ann"])
        XCTAssertEqual(defaults.array(forKey: "hiddenPeople") as? [String], ["People/Ann"])
        projected = try PersonLog.project(libraryRoot: library.url)
        XCTAssertEqual(projected.hidden, ["People/Ann"])
        XCTAssertFalse(projected.hidden.contains("People/Anna"))

        let relaunched = makeStore()
        XCTAssertEqual(relaunched.hiddenPeople, ["People/Ann"])
        let again = try XCTUnwrap(
            relaunched.attachLibrary(
                library.url,
                snapshot: snapshot(of: relaunched, links: links)
            )
        )
        XCTAssertEqual(again.hidden, ["People/Ann"])
        XCTAssertEqual(relaunched.featuredPeople, ["People/Ada", "People/Cy"])
        XCTAssertEqual(relaunched.personContactLinks["People/Ada"], .manual(contactID: "CN:ada"))
    }

    func testEmptyProjectDoesNotClearNonEmptySnapshot() throws {
        let library = TempDir.make()
        defer { library.teardown() }

        let store = makeStore()
        store.hidePerson("People/Anna")
        store.toggleFeaturePerson("People/Ada")
        store.markAsMe("People/Ada")

        try writePersonMigratedMarker(library: library.url, device: store.deviceId)

        let state = store.attachLibrary(library.url, snapshot: snapshot(of: store))
        XCTAssertNil(state, "empty project + non-empty snapshot must not apply")
        XCTAssertEqual(store.hiddenPeople, ["People/Anna"])
        XCTAssertEqual(store.featuredPeople, ["People/Ada"])
        XCTAssertEqual(store.mePersonPath, "People/Ada")
        XCTAssertEqual(defaults.array(forKey: "hiddenPeople") as? [String], ["People/Anna"])
        XCTAssertEqual(defaults.array(forKey: "pinnedPeople") as? [String], ["People/Ada"])
        XCTAssertEqual(defaults.string(forKey: "mePersonPath"), "People/Ada")
    }

    private func snapshot(
        of store: PeopleStore,
        links: [String: PersonLink] = [:]
    ) -> PersonLog.Snapshot {
        PersonLog.Snapshot(
            hiddenPeople: Array(store.hiddenPeople),
            pinnedPeople: store.featuredPeople,
            featuredPhotoByPerson: Dictionary(
                uniqueKeysWithValues: store.featuredPhotoByPerson.map { ($0.key, $0.value.uuidString) }
            ),
            mePersonPath: store.mePersonPath,
            personContactLinks: links
        )
    }

    private func writePersonMigratedMarker(library: URL, device: String) throws {
        let dir = library.appendingPathComponent(".gallery/log/\(device)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let line =
            "{\"id\":\"01900000-0000-7000-8000-00000000ffff\",\"ts\":\"2024-07-01T10:00:00.000000000Z\",\"dev\":\(PersonLog.jsonString(device)),\"type\":\"person_migrated\",\"body\":{}}\n"
        try Data(line.utf8).write(to: dir.appendingPathComponent("2024-07.ndjson"))
    }
}
