import CoreGraphics
import XCTest
@testable import LocalGallery

final class PersonThumbnailCropTests: XCTestCase {
    func testCropFollowsTheFaceBoxNotASquareOfTheLongerSide() {
        let region = FaceRegion(name: nil, centerX: 0.5, centerY: 0.5, width: 0.1, height: 0.2)
        let rect = PersonThumbnailView.cropRect(
            imageSize: CGSize(width: 1000, height: 1000),
            region: region
        )
        let pad = 1 + 2 * PersonThumbnailView.cropPadding
        XCTAssertEqual(rect.width, 100 * pad, accuracy: 0.5)
        XCTAssertEqual(rect.height, 200 * pad, accuracy: 0.5)
        XCTAssertEqual(rect.midX, 500, accuracy: 0.5)
        XCTAssertEqual(rect.midY, 500, accuracy: 0.5)
    }

    func testTwentyFacesInARowDoesNotReachTheNeighbour() {
        // Packed lineup: each face is 5% of the width, centres 5% apart.
        // A square crop of the (taller) head would be ~12% wide and swallow
        // the people on either side.
        let region = FaceRegion(name: nil, centerX: 0.5, centerY: 0.5, width: 0.05, height: 0.12)
        let size = CGSize(width: 4000, height: 2000)
        let rect = PersonThumbnailView.cropRect(imageSize: size, region: region)
        let nextFaceCentre = 0.55 * size.width
        XCTAssertLessThan(rect.maxX, nextFaceCentre)
        XCTAssertEqual(rect.width, 0.05 * size.width * (1 + 2 * PersonThumbnailView.cropPadding), accuracy: 0.5)
    }

    func testASmallFaceIsNotExpandedToAFractionOfThePhoto() {
        let region = FaceRegion(name: nil, centerX: 0.2, centerY: 0.3, width: 0.04, height: 0.05)
        let rect = PersonThumbnailView.cropRect(
            imageSize: CGSize(width: 4000, height: 3000),
            region: region
        )
        XCTAssertLessThan(rect.width, 3000 * 0.35)
        XCTAssertEqual(rect.width, 160 * (1 + 2 * PersonThumbnailView.cropPadding), accuracy: 0.5)
    }

    func testSourceDecodeIsCappedAndBigEnoughForTheCell() {
        let region = FaceRegion(name: nil, centerX: 0.5, centerY: 0.5, width: 0.1, height: 0.12)
        let needed = PersonThumbnailView.sourcePixelSize(
            cellSize: 76, region: region, scale: 3
        )
        // Longest face edge is height 0.12: 76pt × 3 / 0.12 = 1900.
        XCTAssertEqual(needed, 1900, accuracy: 1)
        let tiny = PersonThumbnailView.sourcePixelSize(
            cellSize: 18, region: region, scale: 3
        )
        XCTAssertEqual(tiny, 450, accuracy: 1)
    }

    /// Cell-recycle identity is photo id + region, not `url.path#region`.
    func testCropTaskIDIsStableIDNotPath() {
        let nfc = CoreScanner.fileURL("/lib/café.jpg".precomposedStringWithCanonicalMapping)
        let nfd = CoreScanner.fileURL("/lib/café.jpg".decomposedStringWithCanonicalMapping)
        let region = FaceRegion(name: nil, centerX: 0.2, centerY: 0.3, width: 0.1, height: 0.12)
        let a = PersonThumbnailView.cropTaskID(url: nfc, region: region, size: 76)
        let b = PersonThumbnailView.cropTaskID(url: nfd, region: region, size: 76)
        XCTAssertEqual(a, b)
        XCTAssertTrue(a.hasPrefix(PhotoFile.stableID(for: nfc).uuidString + "#"))
        XCTAssertFalse(a.contains("/lib/"))
        XCTAssertEqual(
            PersonThumbnailView.cropTaskID(url: nfc, region: nil, size: 76),
            PhotoFile.stableID(for: nfc).uuidString
        )
    }

    func testAFaceNearTheEdgeShiftsInsteadOfGoingOutOfBounds() {
        let region = FaceRegion(name: nil, centerX: 0.05, centerY: 0.05, width: 0.08, height: 0.08)
        let rect = PersonThumbnailView.cropRect(
            imageSize: CGSize(width: 1000, height: 1000),
            region: region
        )
        XCTAssertGreaterThanOrEqual(rect.minX, 0)
        XCTAssertGreaterThanOrEqual(rect.minY, 0)
        XCTAssertLessThanOrEqual(rect.maxX, 1000)
        XCTAssertLessThanOrEqual(rect.maxY, 1000)
    }
}
