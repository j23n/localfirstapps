import Foundation
import XCTest
@testable import LocalGallery

@MainActor
final class CorePlacesTests: XCTestCase {
    private func makeTemp() -> TempDir {
        let temp = TempDir.make()
        addTeardownBlock { temp.teardown() }
        return temp
    }

    func testEligibilityComesFromTheCore() {
        let still = PhotoFile.fixture(
            url: URL(fileURLWithPath: "/lib/a.jpg"),
            gps: (lat: 48.8566, lon: 2.3522)
        )
        XCTAssertTrue(CorePlaces.isEligible(still))

        let video = PhotoFile.fixture(
            url: URL(fileURLWithPath: "/lib/a.mov"),
            isVideo: true,
            gps: (lat: 48.8566, lon: 2.3522)
        )
        XCTAssertFalse(CorePlaces.isEligible(video))
        XCTAssertFalse(CorePlaces.isEligible(
            PhotoFile.fixture(url: URL(fileURLWithPath: "/lib/no-gps.jpg"))
        ))
    }

    func testRunWritesAndReportsTheSharedPlacesResult() async {
        let temp = makeTemp()
        let image = temp.appending("eiffel.jpg")
        XCTAssertTrue(FileManager.default.createFile(
            atPath: image.path,
            contents: Data("jpeg".utf8)
        ))
        let photo = PhotoFile.fixture(
            url: image,
            gps: (lat: 48.8566, lon: 2.3522)
        )
        let service = CorePlaces(cacheURL: temp.appending("geo-cache.json"))
        var outcomes: [ScanActivityEntry.Outcome] = []
        service.onPlaceRecorded = { _, _, outcome in outcomes.append(outcome) }

        let first = await service.geocode([photo])
        XCTAssertEqual(first.processed, 1, "processed; lastError=\(service.lastError ?? "nil")")
        XCTAssertEqual(first.written, 1, "written; lastError=\(service.lastError ?? "nil")")
        XCTAssertEqual(outcomes, [.written])
        XCTAssertTrue(FileManager.default.fileExists(atPath: image.path + ".xmp"))

        outcomes.removeAll()
        let second = await service.geocode([photo])
        XCTAssertEqual(second.processed, 1)
        XCTAssertEqual(second.skipped, 1)
        XCTAssertEqual(outcomes, [.skipped])
    }
}
