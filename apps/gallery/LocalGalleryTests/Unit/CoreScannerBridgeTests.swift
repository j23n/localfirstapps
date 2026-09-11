import Foundation
import XCTest
@testable import LocalGallery

/// The Swift half of the scanner: path↔URL bridge, cancellation, and
/// the fact that Swift and Rust agree on the persisted snapshot at runtime.
final class CoreScannerBridgeTests: XCTestCase {

    private var temp: TempDir!

    override func setUp() {
        super.setUp()
        temp = TempDir.make()
    }

    override func tearDown() {
        temp?.teardown()
        temp = nil
        super.tearDown()
    }

    /// Poll `condition` until it holds, or give up at `timeout`.
    ///
    /// The alternative — `Task.sleep(for: .milliseconds(500))` — is wrong in
    /// both directions at once: too short on a loaded CI machine, where the
    /// test fails for reasons that have nothing to do with the code, and too
    /// long everywhere else, where it is dead time in every run. Polling the
    /// thing actually being waited for is neither.
    @MainActor
    private func waitUntil(
        _ what: String,
        timeout: Duration = .seconds(10),
        _ condition: @MainActor () -> Bool,
        file: StaticString = #filePath,
        line: UInt = #line
    ) async {
        let deadline = ContinuousClock.now + timeout
        while ContinuousClock.now < deadline {
            if condition() { return }
            try? await Task.sleep(for: .milliseconds(5))
        }
        XCTAssertTrue(condition(), "timed out waiting for \(what)", file: file, line: line)
    }

    // MARK: - Cancellation

    /// A cancelled scan must produce *nothing*, and say so. An empty outcome
    /// and a library that really is empty have the same shape, and the Store
    /// publishes one of them straight into `allPhotos` + `saveCache()`.
    func testACancelledScanReportsThatItDidNotComplete() async throws {
        let root = temp.appending("Library", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        try Data(repeating: 0x41, count: 10).write(to: root.appendingPathComponent("a.jpg"))

        let scanner = CoreScanner()
        let task = Task { () -> CoreScanner.Result in
            // Deterministic: the scan does not start until cancellation has
            // landed, so this tests the behaviour rather than the scheduler.
            while !Task.isCancelled { await Task.yield() }
            return await scanner.scan(at: root, reuseCached: false)
        }
        task.cancel()
        let cancelled = await task.value

        XCTAssertTrue(cancelled.didNotComplete)
        XCTAssertTrue(cancelled.flatPhotos.isEmpty)
        XCTAssertNil(cancelled.rootFolder)

        // …and the session is not wedged by it: the next scan runs normally.
        let after = await scanner.scan(at: root, reuseCached: false)
        XCTAssertFalse(after.didNotComplete)
        XCTAssertEqual(after.flatPhotos.count, 1)
    }

    // MARK: - The path bridge

    /// `URL(fileURLWithPath:)` DECOMPOSES its input (`PathNormalizationTests`),
    /// so using it to rebuild a URL from the core's path string would give an
    /// externally-created NFC file a different `stableID` than the core just
    /// derived — a silent identity split. Compared on `unicodeScalars`, because
    /// `String ==` is canonical equivalence and could never see it.
    func testFileURLPreservesTheOnDiskSpelling() {
        let nfc = "/lib/café.jpg".precomposedStringWithCanonicalMapping
        let nfd = "/lib/café.jpg".decomposedStringWithCanonicalMapping
        XCTAssertNotEqual(Array(nfc.unicodeScalars), Array(nfd.unicodeScalars), "vacuous otherwise")

        for path in [nfc, nfd, "/lib/spaces and (parens).jpg", "/lib/emoji 🌵 cactus.jpg",
                     "/lib/hash#and?query.jpg", "/lib/plain.jpg"] {
            let url = CoreScanner.fileURL(path)
            XCTAssertTrue(url.isFileURL, path)
            XCTAssertEqual(Array(url.path.unicodeScalars), Array(path.unicodeScalars),
                           "path round trip changed the scalars for \(path)")
            XCTAssertEqual(PhotoFile.stableID(for: url), StableUUID.derive(from: path))
        }

        XCTAssertNotEqual(
            Array(URL(fileURLWithPath: nfc).path.unicodeScalars), Array(nfc.unicodeScalars),
            "if this ever starts preserving, the fast constructor becomes usable again"
        )
    }

    /// The core writes `URL.absoluteString` into the snapshot itself, so its
    /// percent-encoding table has to be Foundation's exactly. `:` is the entry
    /// that looks wrong and is not: Foundation leaves it literal, and escaping
    /// it would give every path containing one — `12:30 clip.mov` off a
    /// camcorder, anything copied off a Windows share — a snapshot key Swift
    /// never writes, so those photos would read as new on every launch.
    ///
    /// `file_url.rs` asserts the same strings from the other side.
    func testTheEncoderAgreesWithFoundationOnEveryReservedCharacter() {
        for path in ["/a/12:30 clip.mov", "/a/b@c.jpg", "/a/b:c.jpg",
                     "/a/spaces and (parens).jpg", "/a/hash#and?query.jpg",
                     "/a/100% real.jpg", "/a/plus+and,comma;semi=eq.jpg"] {
            XCTAssertEqual(
                CoreScanner.fileURL(path).absoluteString,
                URL(fileURLWithPath: path).absoluteString,
                "the bridge and Foundation disagree about \(path)"
            )
            XCTAssertEqual(Array(CoreScanner.fileURL(path).path.unicodeScalars),
                           Array(path.unicodeScalars))
        }
        XCTAssertTrue(CharacterSet.urlPathAllowed.contains(":"),
                      "if this ever changes, core/gallery-model/src/file_url.rs must change with it")
    }

    // MARK: - Snapshot agreement

    /// Why the core drops non-finite GPS coordinates at the source, stated as
    /// the failure it prevents.
    ///
    /// `JSONEncoder` throws on a non-finite `Double`. `JSONDiskCache.save`
    /// catches, logs and carries on — so one photo whose EXIF carries a `0/0`
    /// rational (NaN) or `1/0` (infinity), both of which real cameras write,
    /// means the library snapshot is never written again for the life of the
    /// install: no error surfaces, and every launch full-rescans.
    ///
    /// `gallery_meta`'s `read_gps` drops them where they enter and the core's
    /// snapshot encoder writes them as absent, so nothing the core produces can
    /// poison the cache. This pins the *reason*, which is the part that would
    /// otherwise be lost.
    func testANonFiniteCoordinateIsWhatWouldBreakTheLibraryCache() throws {
        let root = PhotoFolder.fixture(url: temp.url)
        let clean = PhotoFile.fixture(url: temp.appending("a.jpg"), gps: (lat: 48.8581, lon: 2.2945))
        XCTAssertNoThrow(try JSONEncoder().encode(
            LibrarySnapshot(rootFolder: root, allPhotos: [clean], sidecarManifest: [])
        ))

        for poison in [Double.nan, .infinity, -.infinity] {
            let bad = PhotoFile.fixture(url: temp.appending("b.jpg"), gps: (lat: poison, lon: 0))
            XCTAssertThrowsError(
                try JSONEncoder().encode(
                    LibrarySnapshot(rootFolder: root, allPhotos: [bad], sidecarManifest: [])
                ),
                "if this ever stops throwing, the guards in gallery-meta are only belt"
            )
        }
    }

    /// An empty manifest is persisted as an empty manifest.
    ///
    /// `nil` and `[]` mean different things to the load path: `nil` is "written
    /// by a build from before this field existed" and costs a legacy re-probe
    /// of every `.xmp`, `[]` is "scanned, and this library has no sidecars" —
    /// the normal state for anyone not using digiKam. Folding one into the
    /// other made those libraries re-probe on every launch, forever, to
    /// rediscover a fact they had already written down.
    @MainActor
    func testAnEmptyManifestIsPersistedAsEmptyRatherThanAbsent() async throws {
        let paths = GalleryPaths(
            libraryCacheURL: temp.appending("library_cache.json"),
            memoriesCacheURL: temp.appending("memories_cache.json"),
            sidecarCacheURL: temp.appending("sidecar_cache.json"),
            thumbnailDir: temp.appending("thumbnails", isDirectory: true),
            mlCacheDatabaseURL: temp.appending("gallery-cache.sqlite"),
            modelPacksDirectoryURL: temp.appending("ModelPacks", isDirectory: true),
            bundledModelPackURL: nil,
            geocodeCacheURL: temp.appending("geocode-cache.json"),
            bookmarkKey: "rootFolderBookmark"
        )
        let defaults = TestUserDefaults.make()
        defer { TestUserDefaults.cleanup(defaults) }
        let store = GalleryStore(paths: paths, defaults: defaults, clock: SystemClock(),
                                 contactsService: StubContactsService(contacts: []))

        let photo = PhotoFile.fixture(url: temp.appending("a.jpg"))
        store.lastSidecarManifest = []
        store.apply(.scanResult(photos: [photo], root: PhotoFolder.fixture(url: temp.url),
                                persistCache: true))

        await waitUntil("the library cache to reach disk") {
            FileManager.default.fileExists(atPath: paths.libraryCacheURL.path)
        }
        let loaded = try XCTUnwrap(
            JSONDiskCache<LibrarySnapshot>(url: paths.libraryCacheURL,
                                           version: LibrarySnapshot.version, label: "probe").load()
        )
        XCTAssertEqual(loaded.sidecarManifest, [],
                       "an empty manifest must not be persisted as 'this build is too old'")
    }

    /// The committed fixture pins the wire format statically. This pins it
    /// *dynamically*: what `JSONDiskCache` writes today, the core reads today,
    /// and vice versa. A serde change that the fixture happened not to cover
    /// would turn every warm relaunch into a full rescan, silently.
    @MainActor
    func testSwiftAndRustAgreeOnTheSnapshotAtRuntime() async throws {
        let root = temp.appending("Library", isDirectory: true)
        for (rel, size) in [("a.jpg", 10), ("Sub/b.jpg", 11), ("Sub/b.jpg.xmp", 12)] {
            let url = root.appendingPathComponent(rel)
            try FileManager.default.createDirectory(
                at: url.deletingLastPathComponent(), withIntermediateDirectories: true
            )
            try Data(repeating: 0x41, count: size).write(to: url)
        }
        let scanned = await CoreScanner().scan(at: root, reuseCached: false)
        let snapshot = LibrarySnapshot(
            rootFolder: try XCTUnwrap(scanned.rootFolder),
            allPhotos: scanned.flatPhotos,
            sidecarManifest: scanned.sidecarManifest
        )

        // Swift writes, Rust reads.
        let swiftWritten = temp.appending("swift.json")
        let cache = JSONDiskCache<LibrarySnapshot>(
            url: swiftWritten, version: LibrarySnapshot.version, label: "test"
        )
        cache.save(snapshot)
        await waitUntil("the app's snapshot to reach disk") {
            FileManager.default.fileExists(atPath: swiftWritten.path)
        }
        XCTAssertEqual(try probeSnapshotVersion(path: swiftWritten.path),
                       Int64(LibrarySnapshot.version),
                       "the core and the app disagree about the schema version")
        let roundTripped = try loadSnapshot(path: swiftWritten.path)
        XCTAssertEqual(roundTripped.allPhotos.map(\.path).sorted(),
                       snapshot.allPhotos.map(\.url.path).sorted())
        XCTAssertEqual(roundTripped.sidecarManifest?.count, 1)
        XCTAssertEqual(roundTripped.sidecarManifest?.first?.sidecarPath,
                       snapshot.sidecarManifest?.first?.sidecarURL.path)

        // Rust writes, Swift reads.
        let rustWritten = temp.appending("rust.json")
        try saveSnapshot(path: rustWritten.path, snapshot: roundTripped)
        let reloaded = try XCTUnwrap(
            JSONDiskCache<LibrarySnapshot>(
                url: rustWritten, version: LibrarySnapshot.version, label: "test"
            ).load()
        )
        XCTAssertEqual(reloaded.allPhotos.map(\.url), snapshot.allPhotos.map(\.url))
        XCTAssertEqual(reloaded.allPhotos.map(\.dateTaken), snapshot.allPhotos.map(\.dateTaken))
        XCTAssertEqual(reloaded.rootFolder, snapshot.rootFolder)
        XCTAssertEqual(reloaded.sidecarManifest, snapshot.sidecarManifest)
        XCTAssertTrue(FileManager.default.fileExists(atPath: rustWritten.path),
                      "a core-written snapshot must not be evicted by the app's load path")
    }

    /// The manifest survives a save/load cycle through the Store's own cache,
    /// which is the whole of `_plans/06` Finding 2: without it the first light
    /// scan of every session re-probes every `.xmp` in the library.
    @MainActor
    func testTheStorePersistsAndRestoresTheSidecarManifest() async throws {
        // Two stores over one set of paths — the second is the "relaunch".
        let paths = GalleryPaths(
            libraryCacheURL: temp.appending("library_cache.json"),
            memoriesCacheURL: temp.appending("memories_cache.json"),
            sidecarCacheURL: temp.appending("sidecar_cache.json"),
            thumbnailDir: temp.appending("thumbnails", isDirectory: true),
            mlCacheDatabaseURL: temp.appending("gallery-cache.sqlite"),
            modelPacksDirectoryURL: temp.appending("ModelPacks", isDirectory: true),
            bundledModelPackURL: nil,
            geocodeCacheURL: temp.appending("geocode-cache.json"),
            bookmarkKey: "rootFolderBookmark"
        )
        let defaults = TestUserDefaults.make()
        defer { TestUserDefaults.cleanup(defaults) }
        let store = GalleryStore(paths: paths, defaults: defaults, clock: SystemClock(),
                                 contactsService: StubContactsService(contacts: []))

        let root = temp.appending("Library", isDirectory: true)
        for (rel, size) in [("a.jpg", 10), ("a.jpg.xmp", 12)] {
            let url = root.appendingPathComponent(rel)
            try FileManager.default.createDirectory(
                at: url.deletingLastPathComponent(), withIntermediateDirectories: true
            )
            try Data(repeating: 0x41, count: size).write(to: url)
        }
        let scanned = await CoreScanner().scan(at: root, reuseCached: false)
        XCTAssertEqual(scanned.sidecarManifest.count, 1)

        store.lastSidecarManifest = scanned.sidecarManifest
        store.apply(.scanResult(photos: scanned.flatPhotos, root: scanned.rootFolder,
                                persistCache: true))
        // `JSONDiskCache.save` is fire-and-forget on a detached task, so wait
        // for the file to say what the relaunch is about to read rather than
        // for a fixed interval. `load()` is a no-op on a missing file and
        // never evicts one, so polling with it is safe.
        await waitUntil("the library cache to carry the manifest") {
            JSONDiskCache<LibrarySnapshot>(
                url: paths.libraryCacheURL, version: LibrarySnapshot.version, label: "probe"
            ).load()?.sidecarManifest?.count == 1
        }

        let reopened = GalleryStore(paths: paths, defaults: defaults, clock: SystemClock(),
                                    contactsService: StubContactsService(contacts: []))
        XCTAssertEqual(reopened.allPhotos.count, 1, "the library cache did not survive")
        XCTAssertEqual(reopened.lastSidecarManifest, scanned.sidecarManifest,
                       "the manifest must be seeded before the launch scan starts")
    }
}
