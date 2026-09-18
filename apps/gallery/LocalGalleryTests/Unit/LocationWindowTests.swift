import XCTest
@testable import LocalGallery

/// 5.8-ios-windows: folder / people / event lists bind FFI windows through
/// `CoreLibraryIndex` helpers instead of walking `PhotoFolder`.
@MainActor
final class LocationWindowTests: XCTestCase {

    private func nestedLibrary() -> (photos: [PhotoFile], root: PhotoFolder) {
        let rome = PhotoFile.fixture(
            url: URL(fileURLWithPath: "/lib/2024/rome.jpg"),
            dateTaken: date(2024, 6, 2),
            tags: ["Places/Italy/Rome"]
        )
        let alice = PhotoFile.fixture(
            url: URL(fileURLWithPath: "/lib/2024/paris/alice.jpg"),
            dateTaken: date(2025, 6, 1),
            tags: ["People/Alice"]
        )
        let picnic = PhotoFile.fixture(
            url: URL(fileURLWithPath: "/lib/2018/event.jpg"),
            dateTaken: date(2018, 7, 4),
            tags: ["Events/Picnic"]
        )
        let paris = PhotoFolder.fixture(
            url: URL(fileURLWithPath: "/lib/2024/paris"),
            name: "paris",
            photos: [alice]
        )
        let year = PhotoFolder.fixture(
            url: URL(fileURLWithPath: "/lib/2024"),
            name: "2024",
            subfolders: [paris],
            photos: [rome]
        )
        let event = PhotoFolder.fixture(
            url: URL(fileURLWithPath: "/lib/2018"),
            name: "2018",
            photos: [picnic]
        )
        let root = PhotoFolder.fixture(
            url: URL(fileURLWithPath: "/lib"),
            name: "lib",
            subfolders: [year, event]
        )
        return ([rome, alice, picnic], root)
    }

    private func builtIndex() async -> (CoreLibraryIndex, [PhotoFile], PhotoFolder) {
        let lib = nestedLibrary()
        let index = CoreLibraryIndex()
        index.build(allPhotos: lib.photos, rootFolder: lib.root)
        await index.settle()
        return (index, lib.photos, lib.root)
    }

    func testFolderListingSkipsTheScanRoot() async {
        let (index, _, root) = await builtIndex()
        let rows = index.folderListing(parentID: nil)
        XCTAssertEqual(rows.map(\.title), ["2024", "2018"])
        XCTAssertFalse(rows.contains { $0.id == root.id.uuidString })
        XCTAssertEqual(rows.first?.trailing, "2 photos")
        XCTAssertEqual(rows.last?.trailing, "1 photo")
    }

    func testNestedFolderListingAndOwnPhotoIds() async {
        let (index, photos, root) = await builtIndex()
        let year = root.subfolders[0]
        let paris = year.subfolders[0]
        let event = root.subfolders[1]

        let yearRows = index.folderListing(parentID: year.id.uuidString)
        XCTAssertEqual(yearRows.map(\.title), ["paris"])
        XCTAssertTrue(index.folderHasChildren(year.id.uuidString))
        XCTAssertFalse(index.folderHasChildren(paris.id.uuidString))
        XCTAssertFalse(index.folderHasChildren(event.id.uuidString))

        XCTAssertEqual(index.folderPhotoIDs(year.id), [photos[0].id], "own slice, not the recursive total")
        XCTAssertEqual(index.folderPhotoIDs(paris.id), [photos[1].id])
        XCTAssertEqual(index.folderPhotoIDs(event.id), [photos[2].id])
        XCTAssertTrue(index.folderPhotoIDs(root.id).isEmpty)
    }

    func testFolderListingMemoClearsOnPublish() async {
        let (index, photos, root) = await builtIndex()
        let first = index.folderListing(parentID: nil)
        XCTAssertEqual(first.count, 2)
        let again = index.folderListing(parentID: nil)
        XCTAssertEqual(again, first)

        index.build(allPhotos: [photos[2]], rootFolder: root.subfolders[1])
        await index.settle()
        let after = index.folderListing(parentID: nil)
        XCTAssertTrue(after.isEmpty, "sole remaining folder is the scan root and is not a row")
        XCTAssertEqual(index.folderPhotoIDs(root.subfolders[1].id), [photos[2].id])
    }

    func testPeopleAndCollectionWindowsDriveHubRows() async {
        let (index, photos, _) = await builtIndex()

        let people = index.peopleListing()
        XCTAssertEqual(people.map(\.id), ["People/Alice"])
        XCTAssertEqual(people.first?.title, "Alice")
        XCTAssertEqual(people.first?.trailing, "1 photo")
        XCTAssertEqual(index.personSuggestion(for: "People/Alice")?.fullPath, "People/Alice")

        let rail = index.peopleRailListing()
        XCTAssertEqual(rail.map(\.id), ["People/Alice"], "recent Alice stays on the rail")

        let events = index.collectionListing(sectionID: "events")
        XCTAssertEqual(events.map(\.title), ["Picnic"])
        XCTAssertNotNil(index.tagSuggestion(for: events[0].id))
        XCTAssertTrue(index.collectionListing(sectionID: "people").isEmpty)
        XCTAssertFalse(index.collectionListing(sectionID: "places").isEmpty)

        index.setPersonState(
            PersonStateStructure(
                hidden: ["People/Alice"],
                featured: [],
                me: "",
                featuredPhoto: [],
                links: []
            ),
            now: date(2026, 9, 18)
        )
        XCTAssertTrue(index.peopleRailListing().isEmpty, "hidden people leave the rail")
        XCTAssertEqual(index.peopleListing().map(\.id), ["People/Alice"], "see-all still lists hidden last")
        XCTAssertEqual(photos.count, 3)
    }

    func testFolderDestinationIsIdentityNotAListingTree() async {
        let (index, _, root) = await builtIndex()
        let year = try XCTUnwrap(index.folderDestination(id: root.subfolders[0].id.uuidString))
        XCTAssertEqual(year.id, root.subfolders[0].id)
        XCTAssertEqual(year.name, "2024")
        XCTAssertTrue(year.subfolders.isEmpty)
        XCTAssertTrue(year.photos.isEmpty)
        XCTAssertTrue(index.folderHasChildren(year.id.uuidString))
    }

    func testAttachFoldersPublishesAJustCreatedChild() async {
        let (index, photos, root) = await builtIndex()
        XCTAssertEqual(index.folderListing(parentID: nil).count, 2)

        let created = PhotoFolder.fixture(
            url: URL(fileURLWithPath: "/lib/new"),
            name: "new"
        )
        let updated = root.inserting(created, inParent: root.id)
        index.attachFolders(from: updated)

        let rows = index.folderListing(parentID: nil)
        XCTAssertEqual(rows.map(\.title), ["2024", "2018", "new"])
        XCTAssertEqual(index.folderPhotoIDs(root.subfolders[0].id), [photos[0].id])
    }

    func testSortedFolderRowsHonorNameOrder() async {
        let (index, _, _) = await builtIndex()
        let rows = index.folderListing(parentID: nil)
        XCTAssertEqual(
            index.sortedFolderRows(rows, order: .nameAscending).map(\.title),
            ["2018", "2024"]
        )
        XCTAssertEqual(
            index.sortedFolderRows(rows, order: .nameDescending).map(\.title),
            ["2024", "2018"]
        )
    }

    func testAppRouterDeepLinkUsesFolderWindows() async {
        let lib = nestedLibrary()
        let harness = TestGalleryStore.make()
        defer { harness.teardown() }
        harness.store.apply(.scanResult(photos: lib.photos, root: lib.root, persistCache: false))
        await harness.store.settleIndex()

        let router = AppRouter()
        let leaf = lib.root.subfolders[1]
        let url = try XCTUnwrap(WidgetDeepLink.folder(id: leaf.id.uuidString).url)
        router.handle(url, store: harness.store)

        XCTAssertEqual(router.selectedTab, .folders)
        XCTAssertNil(router.pendingFolderId)
        guard case .grid(let folder) = router.foldersPath.first else {
            return XCTFail("leaf with photos should push the grid, got \(router.foldersPath)")
        }
        XCTAssertEqual(folder.id, leaf.id)
        XCTAssertTrue(folder.subfolders.isEmpty, "route payload is destination identity")

        let year = lib.root.subfolders[0]
        let yearURL = try XCTUnwrap(WidgetDeepLink.folder(id: year.id.uuidString).url)
        router.handle(yearURL, store: harness.store)
        guard case .browser(let dest) = router.foldersPath.first else {
            return XCTFail("folder with children should push the browser, got \(router.foldersPath)")
        }
        XCTAssertEqual(dest.id, year.id)
    }

    func testAppRouterWaitsForTheIndexPublish() {
        let harness = TestGalleryStore.make()
        defer { harness.teardown() }
        let router = AppRouter()
        router.handle(
            WidgetDeepLink.folder(id: UUID().uuidString).url ?? URL(string: "localgallery://folder/missing")!,
            store: harness.store
        )
        XCTAssertNotNil(router.pendingFolderId)
        XCTAssertTrue(router.foldersPath.isEmpty)
    }
}
