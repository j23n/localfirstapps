import XCTest
@testable import LocalGallery

/// Sidecar-cache mutations must land on `allPhotos`, the index object
/// table, and the folder tree together.
@MainActor
final class GalleryStoreRuntimeStateTests: XCTestCase {

    private func makeHarness() -> TestGalleryStore.Harness {
        let inner = TestGalleryStore.make()
        addTeardownBlock { @MainActor in inner.teardown() }
        return inner
    }

    func testClearingSidecarCacheResetsIndexAndTree() {
        let h = makeHarness()
        var photo = PhotoFile.fixture(url: h.tempDir.appending("photo.jpg"))
        photo.sidecarStatus = .cached(ContentVersion(size: 8))
        let folder = PhotoFolder.fixture(url: h.tempDir.url, photos: [photo])
        h.store.apply(.scanResult(photos: [photo], root: folder, persistCache: false))
        XCTAssertEqual(h.store.photo(byID: photo.id)?.sidecarStatus, photo.sidecarStatus)

        h.store.apply(.sidecarCacheCleared)

        XCTAssertEqual(h.store.allPhotos.first?.sidecarStatus, .absent)
        XCTAssertEqual(h.store.photo(byID: photo.id)?.sidecarStatus, .absent)
        XCTAssertEqual(h.store.rootFolder?.photos.first?.sidecarStatus, .absent)
    }
}
