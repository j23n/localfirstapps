import Foundation
import Observation
import os

/// The app's side of the Rust core's `LibraryIndex` (Phase 4).
///
/// Replaces `SearchIndex` and `TagIndex`. Every ordering and matching decision
/// — the date-descending sort with its `url.path` tiebreak, the search corpus
/// and its canonical-equivalence substring match, the tag buckets with the
/// `Places/*` prefix expansion, and the `TagSuggestion` aggregation — now lives
/// in `gallery-index`. What stays here is the two things the core cannot do:
///
/// 1. **Resolve actions.** Grid structure is IDs and grid content arrives as
///    bounded `GalleryMediaItem` DTOs. `photoByID` is retained only to resolve
///    an explicit viewer/share/edit action back to the shell's file handle; it
///    has no ordering or matching rule of its own.
/// 2. **Keep the FFI off the main actor.** `build` runs the core call on a
///    detached task behind a generation counter (the horizon grouping); the
///    two query calls are synchronous but memoised, so a scrolling list row
///    that asks the same question on every body evaluation crosses the boundary
///    once, not once per frame.
///
/// `@Observable` so view reads chained through the Store (`store.sortedPhotos`,
/// `store.allTags`) re-render when a rebuild or `removePhotos` lands.
@Observable
@MainActor
final class CoreLibraryIndex {

    // MARK: Published state

    /// Date-descending photo list, in the core's order. Backs
    /// `store.sortedPhotos` — this order *is* the grid.
    private(set) var sortedPhotos: [PhotoFile] = []
    /// All unique tags across the library, `(count desc, id asc)`.
    private(set) var allTags: [TagSuggestion] = []
    /// The `People/*` subset, each carrying its most recent photo date.
    private(set) var peopleTags: [TagSuggestion] = []

    /// `PhotoFile.id` → `PhotoFile`. The object table the core's id answers are
    /// resolved through. Built synchronously in `build(allPhotos:)` because
    /// `photo(byID:)` is on the viewer's and the memory rail's critical path and
    /// must never be a frame behind `allPhotos`.
    @ObservationIgnored private var photoByID: [UUID: PhotoFile] = [:]

    /// False until the first rebuild publishes.
    ///
    /// `sortedPhotos` is empty for the ~400 ms between `apply(_:)` and the
    /// first publish, and an empty *sorted* list is indistinguishable from an
    /// empty *library* to a view that only sees the former — which is how a
    /// cold launch with a warm cache flashed "No photos match." over a library
    /// the user definitely has. Views that render `sortedPhotos` check this
    /// before deciding they have nothing to show.
    private(set) var hasEverPublished = false

    // MARK: Wiring

    @ObservationIgnored private let core = LibraryIndex()

    /// Opaque retained-library handle for core services such as memory
    /// generation. No photo payload is reconstructed or re-marshalled.
    func retainedLibrary() -> LibraryIndex { core }
    /// Cancels a stale rebuild's publish. Same shape as the old
    /// `tagBuildGeneration`, now covering the whole rebuild rather than just
    /// the tag half.
    @ObservationIgnored private var generation = 0
    /// The in-flight rebuild, so tests can wait for the index to settle rather
    /// than poll — and so the *next* rebuild can wait for it before touching
    /// the core (see `build(allPhotos:)`).
    @ObservationIgnored private var pending: Task<Void, Never>?
    @ObservationIgnored private var publishedFingerprint: LibraryFingerprint?

    /// Memoised `photos(forTag:)` answers, cleared when a rebuild **publishes**.
    ///
    /// Not a second tag index: the keys, the membership and the order all come
    /// from the core. This is a cache of its replies, and it exists because
    /// `PeopleListRow` and `PersonCard` ask the same question inside a
    /// `ScrollView` body.
    ///
    /// Cleared at publish rather than at the start of a rebuild, and that is
    /// the whole point: between the two the core still holds the *previous*
    /// index while `photoByID` already holds the new library, so a query in
    /// that window answers with ids the table cannot resolve — usually an empty
    /// list. Clearing first meant that answer was cached *after* the clear and
    /// outlived the rebuild entirely: blank `PersonCard`s and an empty
    /// `TagGridView` until something else happened to rescan.
    @ObservationIgnored private var tagPhotoCache: [String: [PhotoFile]] = [:]
    /// Single-entry memo for `search`, for the same reason: `TagGridView.photos`
    /// is a computed property evaluated on every body pass. Same publish-time
    /// invalidation as `tagPhotoCache`.
    @ObservationIgnored private var searchCache: (key: String, result: [PhotoFile])?

    /// Memoised location-window replies. Views ask these on every body pass;
    /// the core's `folder_structure` also writes `visible_folder_parent`, so
    /// a cache miss must not re-cross the boundary just to paint the same list.
    @ObservationIgnored private var folderListingCache: [String: [GalleryTextRow]] = [:]
    @ObservationIgnored private var folderPhotoIDCache: [String: [UUID]] = [:]
    @ObservationIgnored private var exportedFoldersCache: [ScannedFolderHost]?
    @ObservationIgnored private var peopleListingCache: [GalleryTextRow]?
    @ObservationIgnored private var peopleRailCache: [GalleryTextRow]?
    @ObservationIgnored private var collectionListingCache: [String: [GalleryTextRow]] = [:]

    /// Bumps when location memos are dropped so listing views re-render
    /// without reading `@ObservationIgnored` caches.
    private(set) var listingEpoch: UInt64 = 0

    private let listingPageSize: UInt64 = 256

    // MARK: Build

    /// Publish `allPhotos` as the object table and kick off the core rebuild.
    ///
    /// Split in two on purpose. The dictionary is built here, synchronously, so
    /// `photo(byID:)` is correct the instant `apply(_:)` returns. The sort, the
    /// corpus and the tag aggregation — the parts that were 0.25–0.3 s of main
    /// thread on a 20k library — go to the core on a detached task, and so does
    /// everything around them: marshalling 20k `PhotoFile`s into `ScannedMediaHost`
    /// records (14–22 ms) and resolving 20k ids back through `table`
    /// (`UUID(uuidString:)` per id plus a dictionary hit) are both work the main
    /// actor has no reason to do. Only the assignment of the finished arrays
    /// happens back here.
    ///
    /// **Core builds are serialised.** Cancelling `pending` stops a stale
    /// rebuild from *publishing*, but not from running: the inner detached task
    /// is not a child, and the core takes its write lock only after its build
    /// finishes, so two overlapping rebuilds swap in whichever order they
    /// happen to end. The published Swift state would be the fresh one and the
    /// core's index the stale one — and since queries go to the core, every
    /// `search` / `photos(forTag:)` would answer from a library the app no
    /// longer shows. Awaiting the previous rebuild first makes last-started =
    /// last-swapped, and costs nothing a second concurrent CPU-bound build was
    /// not costing already.
    func build(allPhotos: [PhotoFile], rootFolder: PhotoFolder? = nil) {
        generation += 1
        let generation = self.generation
        var table: [UUID: PhotoFile] = [:]
        table.reserveCapacity(allPhotos.count)
        for photo in allPhotos where table[photo.id] == nil {
            // First id wins, mirroring the old `uniquingKeysWith: { a, _ in a }`.
            table[photo.id] = photo
        }
        photoByID = table
        // An immutable copy for the detached task: a `var` local is main-actor
        // isolated to region analysis even when its type is `Sendable`, and the
        // build task resolves the core's ids through it.
        let lookup = table
        let fingerprint = LibraryFingerprint(photos: allPhotos)
        let tree = rootFolder

        let previous = pending
        previous?.cancel()
        let core = self.core
        pending = Task { [weak self] in
            // Cancelled or not, the previous rebuild's core call runs to
            // completion; waiting for it is what orders the two swaps.
            await previous?.value
            let t = CFAbsoluteTimeGetCurrent()
            let built = await Task.detached(priority: .userInitiated) { () -> Built in
                let start = CFAbsoluteTimeGetCurrent()
                let records = allPhotos.map(CoreScanner.record(of:))
                let zone = Calendar.current.timeZone
                let offsets = allPhotos.map {
                    Int32(zone.secondsFromGMT(for: $0.dateTaken ?? Date()))
                }
                let marshalled = CFAbsoluteTimeGetCurrent()
                let summary = core.buildWithTimeZoneOffsets(
                    photos: records,
                    photoTimeZoneOffsets: offsets
                )
                // Same attach GTK does after `index.build`: folder slices
                // have to be on the table before `remove_photos` can rewrite
                // them. `rebuild` alone empties that table.
                if let tree {
                    let attached = CoreScanner.folderRecords(from: tree)
                    core.setFolders(
                        folders: attached.folders,
                        photoIdsInScanOrder: attached.photoIds
                    )
                }
                // The core's records never reach the main actor: they are not
                // `Sendable` (UniFFI does not mark them) and resolving them is
                // 20k `UUID(uuidString:)` parses plus 20k dictionary hits, which
                // is exactly the kind of work this hop exists to move.
                return Built(
                    sorted: summary.sortedPhotoIds.compactMap {
                        UUID(uuidString: $0).flatMap { lookup[$0] }
                    },
                    tags: summary.tags.map(Self.suggestion(from:)),
                    people: summary.people.map(Self.suggestion(from:)),
                    fingerprint: fingerprint,
                    marshalMillis: (marshalled - start) * 1000,
                    coreMillis: Double(summary.buildMillis)
                )
            }.value
            guard !Task.isCancelled, let self, self.generation == generation else { return }

            // Everything below this line is one main-actor turn: the memos are
            // dropped and the new answers published without a suspension in
            // between, so no query can observe half of a rebuild.
            self.tagPhotoCache = [:]
            self.searchCache = nil
            self.clearListingMemos()
            self.sortedPhotos = built.sorted
            self.allTags = built.tags
            self.peopleTags = built.people
            self.publishedFingerprint = built.fingerprint
            self.hasEverPublished = true
            self.onRebuild?(self.allTags, self.peopleTags)

            let total = String(format: "%.0f", (CFAbsoluteTimeGetCurrent() - t) * 1000)
            Log.index.info("""
                Built: \(allPhotos.count) photos, \(built.tags.count) unique tags, \
                \(built.people.count) people in \(total)ms \
                (in=\(String(format: "%.0f", built.marshalMillis))ms \
                core=\(String(format: "%.0f", built.coreMillis))ms)
                """)
        }
    }

    /// Fired on the main actor after every rebuild publishes, with the freshly
    /// aggregated lists. The Store uses it to push `topPeople` into
    /// `PeopleStore` and re-export the widget snapshot — the two things the old
    /// `Task.detached` tail in `rebuildSortAndIndex` did.
    @ObservationIgnored var onRebuild: (([TagSuggestion], [TagSuggestion]) -> Void)?

    /// Drop `ids` from the core table the way GTK `apply_photos_removed`
    /// does: `LibraryIndex.removePhotos`, not another `build`. A rebuild
    /// would empty the folder table `setFolders` just attached.
    ///
    /// The id table is updated here so `photo(byID:)` is correct the instant
    /// `apply(.photosRemoved)` returns. The core call is serialized behind
    /// `pending` the same way `build` is, so a stale rebuild cannot swap
    /// the deleted rows back in.
    func removePhotos(_ ids: Set<UUID>) {
        guard !ids.isEmpty else { return }
        generation += 1
        let generation = self.generation
        for id in ids {
            photoByID.removeValue(forKey: id)
        }
        if !sortedPhotos.isEmpty {
            sortedPhotos = sortedPhotos.filter { !ids.contains($0.id) }
        }
        tagPhotoCache = [:]
        searchCache = nil
        clearListingMemos()

        let lookup = photoByID
        let idStrings = ids.map(\.uuidString)
        let previous = pending
        previous?.cancel()
        let core = self.core
        pending = Task { [weak self] in
            await previous?.value
            let t = CFAbsoluteTimeGetCurrent()
            let built = await Task.detached(priority: .userInitiated) { () -> Built in
                let start = CFAbsoluteTimeGetCurrent()
                _ = core.removePhotos(ids: idStrings)
                let suggestions = core.tagSuggestions()
                let sorted = core.sortedPhotoIds().compactMap {
                    UUID(uuidString: $0).flatMap { lookup[$0] }
                }
                return Built(
                    sorted: sorted,
                    tags: suggestions.tags.map(Self.suggestion(from:)),
                    people: suggestions.people.map(Self.suggestion(from:)),
                    fingerprint: LibraryFingerprint(photos: sorted),
                    marshalMillis: 0,
                    coreMillis: (CFAbsoluteTimeGetCurrent() - start) * 1000
                )
            }.value
            guard !Task.isCancelled, let self, self.generation == generation else { return }

            self.tagPhotoCache = [:]
            self.searchCache = nil
            self.clearListingMemos()
            self.sortedPhotos = built.sorted
            self.allTags = built.tags
            self.peopleTags = built.people
            self.publishedFingerprint = built.fingerprint
            self.hasEverPublished = true
            self.onRebuild?(self.allTags, self.peopleTags)

            let total = String(format: "%.0f", (CFAbsoluteTimeGetCurrent() - t) * 1000)
            Log.index.info("""
                Removed: \(idStrings.count) ids, \(built.sorted.count) photos left \
                in \(total)ms (core=\(String(format: "%.0f", built.coreMillis))ms)
                """)
        }
    }

    /// Push projected `.gallery/log` state into the core people lists.
    /// GTK calls the same method after attach and after every person mutation.
    func setPersonState(_ state: PersonStateStructure, now: Date = Date()) {
        core.setPersonState(state: state, now: now.timeIntervalSinceReferenceDate)
        peopleListingCache = nil
        peopleRailCache = nil
        listingEpoch += 1
    }

    /// Last `setPersonState` projection. Tests assert the Store actually
    /// pushed; production people UI still reads `PeopleStore`.
    func personState() -> PersonStateStructure {
        core.personState()
    }

    /// Re-attach the scanner folder table after a host-only tree edit
    /// (create-folder) without another photo rebuild.
    func attachFolders(from root: PhotoFolder) {
        let attached = CoreScanner.folderRecords(from: root)
        core.setFolders(
            folders: attached.folders,
            photoIdsInScanOrder: attached.photoIds
        )
        clearFolderMemos()
        listingEpoch += 1
    }

    /// This folder's own photo ids from the core folder table.
    func folderPhotoIDs(_ folderID: UUID) -> [UUID] {
        folderPhotoIDs(folderID: folderID.uuidString)
    }

    func folderPhotoIDs(folderID: String) -> [UUID] {
        if let cached = folderPhotoIDCache[folderID] { return cached }
        let ids = core.folderPhotoIds(folderId: folderID).compactMap(UUID.init(uuidString:))
        folderPhotoIDCache[folderID] = ids
        return ids
    }

    /// Wait for the in-flight rebuild, if any.
    ///
    /// **Tests only.** No production path awaits it, and none should: the app's
    /// contract is that `photo(byID:)` is correct immediately and the sorted /
    /// aggregated views arrive when they arrive, observed rather than awaited.
    /// A caller that blocked on this would be reintroducing the main-thread
    /// stall the rebuild was moved off the main actor to remove.
    func settle() async {
        await pending?.value
    }

    func ownsScheduledPhotos(_ photos: [PhotoFile]) -> Bool {
        guard let publishedFingerprint,
              publishedFingerprint.ids.count == photos.count else { return false }
        return zip(publishedFingerprint.ids, photos).allSatisfy { id, photo in
            id == photo.id
        }
    }

    /// Scheduled memories over the same core-owned photo generation that
    /// backs the grid and search index.
    ///
    /// Production calls this after an index publish, so the 20k photo records
    /// and capture-time offsets do not cross the FFI a second time. The only
    /// per-horizon payload is contacts, person-link intent, clock and the
    /// nine day-offset values.
    func computeScheduled(
        _ inputs: CoreMemories.Inputs,
        hiddenMemoryIDs: Set<String>
    ) async -> [CoreMemories.Scheduled] {
        await pending?.value
        return await CoreMemories.computeScheduled(
            inputs,
            using: core,
            hiddenMemoryIDs: hiddenMemoryIDs
        )
    }

    // MARK: Queries

    /// O(1) photo lookup by id.
    func photo(byID id: UUID) -> PhotoFile? { photoByID[id] }

    /// Patch one photo in the object table and every published copy that
    /// already holds it. Used for locality / sidecar-cache updates that
    /// must not wait for (or trigger) a full core rebuild.
    func updatePhoto(_ photo: PhotoFile) {
        updatePhotos([photo])
    }

    /// Patch the given photos by id. Membership and sort order stay put;
    /// only the structs the UI already has are replaced.
    func updatePhotos(_ photos: [PhotoFile]) {
        guard !photos.isEmpty else { return }
        for photo in photos {
            photoByID[photo.id] = photo
        }
        if !sortedPhotos.isEmpty {
            sortedPhotos = sortedPhotos.map { photoByID[$0.id] ?? $0 }
        }
        if !tagPhotoCache.isEmpty {
            for key in tagPhotoCache.keys {
                tagPhotoCache[key] = tagPhotoCache[key]?.map { photoByID[$0.id] ?? $0 }
            }
        }
        if let cached = searchCache {
            searchCache = (cached.key, cached.result.map { photoByID[$0.id] ?? $0 })
        }
    }

    /// Photos credited to `tag`, including the `Places/*` prefix expansion, in
    /// `allPhotos` order — `TagIndex.photos(forTag:)`'s contract.
    func photos(forTag tag: TagSuggestion) -> [PhotoFile] {
        let key = tag.fullPath
        if let cached = tagPhotoCache[key] { return cached }
        let resolved = resolve(core.photoIdsForTag(fullPath: key))
        tagPhotoCache[key] = resolved
        return resolved
    }

    /// Filter the sorted photo list by AND-combining required tags and an
    /// optional substring query.
    ///
    /// Unlike the Swift original this takes no `allTags` argument: the core
    /// holds the aggregated list from its own build and uses it for the
    /// exact-tag-path branch. The old signature let a caller pass an empty list
    /// and silently degrade every tag query — including the *virtual* prefix
    /// tags no photo carries — to a substring match.
    func search(query: String, requiredTags: [TagSuggestion] = []) -> [PhotoFile] {
        let paths = requiredTags.map(\.fullPath)
        let key = "\(query)\u{0}\(paths.joined(separator: "\u{0}"))"
        if let cached = searchCache, cached.key == key { return cached.result }
        let t = CFAbsoluteTimeGetCurrent()
        let resolved = resolve(core.search(query: query, requiredTagPaths: paths))
        searchCache = (key, resolved)
        Log.search.debug("""
            "\(Log.r.other(query))" tags:\(requiredTags.map { Log.r.tag($0.displayName) }) \
            → \(resolved.count) matches in \(String(format: "%.1f", (CFAbsoluteTimeGetCurrent() - t) * 1000))ms
            """)
        return resolved
    }

    // MARK: Windowed display projection

    /// Publish filter intent and return IDs/sections only. Content is fetched
    /// separately through `photoWindow`; callers must carry this generation
    /// into every bounded read.
    func photoView(query: String, requiredTags: [TagSuggestion] = []) -> ViewStructure {
        core.setPhotoView(
            query: query.trimmingCharacters(in: .whitespaces),
            requiredTagPaths: requiredTags.map(\.fullPath)
        )
    }

    func photoIDsView(
        id: String,
        photoIDs: [UUID],
        query: String,
        requiredTags: [TagSuggestion]
    ) -> ViewStructure {
        core.setPhotoIdsView(
            viewId: id,
            photoIds: photoIDs.map(\.uuidString),
            query: query.trimmingCharacters(in: .whitespaces),
            requiredTagPaths: requiredTags.map(\.fullPath)
        )
    }

    /// One display-ready range. A generation mismatch is surfaced as
    /// `ViewError.staleGeneration` rather than silently mixing two rebuilds.
    func photoWindow(
        sectionID: String,
        offset: Int,
        limit: Int,
        generation: UInt64
    ) throws -> [GalleryMediaItem] {
        try core.photoWindow(
            sectionId: sectionID,
            offset: UInt64(offset),
            limit: UInt64(limit),
            generation: generation
        )
    }

    func tagView() -> ViewStructure {
        core.tagStructure()
    }

    func tagWindow(
        sectionID: String,
        offset: Int,
        limit: Int,
        generation: UInt64
    ) throws -> [GalleryTextRow] {
        try core.tagWindow(
            sectionId: sectionID,
            offset: UInt64(offset),
            limit: UInt64(limit),
            generation: generation
        )
    }

    func photos(withIDs ids: [UUID]) -> [PhotoFile] {
        ids.compactMap { photoByID[$0] }
    }

    // MARK: Location listings

    /// Children of `parentID`. `nil` is the Folders-tab root (children of
    /// the scan root; the sole library root is not a row).
    func folderListing(parentID: String?) -> [GalleryTextRow] {
        let key = parentID ?? ""
        if let cached = folderListingCache[key] { return cached }
        let structure = core.folderStructure(parentId: parentID)
        let rows = textRows(
            structure: structure,
            sectionID: "folders",
            window: { try core.folderWindow(sectionId: $0, offset: $1, limit: $2, generation: $3) },
            refresh: { core.folderStructure(parentId: parentID) }
        )
        folderListingCache[key] = rows
        return rows
    }

    func sortedFolderRows(_ rows: [GalleryTextRow], order: FolderSortOrder) -> [GalleryTextRow] {
        switch order {
        case .nameAscending:
            return rows.sorted { $0.title.localizedStandardCompare($1.title) == .orderedAscending }
        case .nameDescending:
            return rows.sorted { $0.title.localizedStandardCompare($1.title) == .orderedDescending }
        case .dateModifiedNewest:
            return rows.sorted { folderTime($0.id, \.dateModified) > folderTime($1.id, \.dateModified) }
        case .dateModifiedOldest:
            return rows.sorted { folderTime($0.id, \.dateModified) < folderTime($1.id, \.dateModified) }
        case .dateCreatedNewest:
            return rows.sorted { folderTime($0.id, \.dateCreated) > folderTime($1.id, \.dateCreated) }
        case .dateCreatedOldest:
            return rows.sorted { folderTime($0.id, \.dateCreated) < folderTime($1.id, \.dateCreated) }
        }
    }

    func folderHasChildren(_ folderID: String) -> Bool {
        let folders = exportedFolders()
        guard let index = folders.firstIndex(where: { $0.id.caseInsensitiveCompare(folderID) == .orderedSame }) else {
            return false
        }
        let parent = UInt32(index)
        return folders.contains { $0.parentIndex == parent }
    }

    func folderHost(_ folderID: String) -> ScannedFolderHost? {
        exportedFolders().first { $0.id.caseInsensitiveCompare(folderID) == .orderedSame }
    }

    func folderCoverURL(_ folderID: String) -> URL? {
        folderHost(folderID)?.coverPhotoPath.map(CoreScanner.fileURL)
    }

    func folderExists(_ folderID: String) -> Bool {
        folderHost(folderID) != nil
    }

    /// Destination identity for navigation / move-to-folder. Not a listing tree.
    func folderDestination(id: String) -> PhotoFolder? {
        guard let host = folderHost(id),
              let uuid = UUID(uuidString: host.id) else { return nil }
        return PhotoFolder(
            id: uuid,
            url: CoreScanner.fileURL(host.path),
            name: host.name,
            subfolders: [],
            photos: [],
            coverPhotoURL: host.coverPhotoPath.map(CoreScanner.fileURL),
            totalPhotoCount: Int(host.totalPhotoCount),
            dateModified: host.dateModified.map(Date.init(timeIntervalSinceReferenceDate:)),
            dateCreated: host.dateCreated.map(Date.init(timeIntervalSinceReferenceDate:))
        )
    }

    /// The hidden scan root when the Folders tab lists its children.
    func scanRootFolderID() -> UUID? {
        let folders = exportedFolders()
        let roots = folders.filter { $0.parentIndex == nil }
        guard roots.count == 1 else { return nil }
        return UUID(uuidString: roots[0].id)
    }

    /// See-all people (`people_structure` / `people_window`).
    func peopleListing() -> [GalleryTextRow] {
        if let cached = peopleListingCache { return cached }
        let structure = core.peopleStructure()
        let rows = textRows(
            structure: structure,
            sectionID: "people",
            window: { try core.peopleWindow(sectionId: $0, offset: $1, limit: $2, generation: $3) },
            refresh: { core.peopleStructure() }
        )
        peopleListingCache = rows
        return rows
    }

    /// Collections-rail people (`people_rail_structure` / `people_rail_window`).
    func peopleRailListing() -> [GalleryTextRow] {
        if let cached = peopleRailCache { return cached }
        let structure = core.peopleRailStructure()
        let rows = textRows(
            structure: structure,
            sectionID: "people",
            window: { try core.peopleRailWindow(sectionId: $0, offset: $1, limit: $2, generation: $3) },
            refresh: { core.peopleRailStructure() }
        )
        peopleRailCache = rows
        return rows
    }

    func collectionListing(sectionID: String) -> [GalleryTextRow] {
        if let cached = collectionListingCache[sectionID] { return cached }
        let structure = core.collectionStructure()
        let rows = textRows(
            structure: structure,
            sectionID: sectionID,
            window: { try core.collectionWindow(sectionId: $0, offset: $1, limit: $2, generation: $3) },
            refresh: { core.collectionStructure() }
        )
        collectionListingCache[sectionID] = rows
        return rows
    }

    func tagSuggestion(for idOrPath: String) -> TagSuggestion? {
        let key = idOrPath.lowercased()
        return allTags.first { $0.id == key || $0.fullPath.lowercased() == key }
            ?? peopleTags.first { $0.id == key || $0.fullPath.lowercased() == key }
    }

    func personSuggestion(for idOrPath: String) -> TagSuggestion? {
        if let tag = tagSuggestion(for: idOrPath) { return tag }
        if let path = core.personFullPath(idOrPath: idOrPath) {
            return tagSuggestion(for: path)
        }
        return nil
    }

    // MARK: Bridging

    /// One rebuild's results, already in the app's own types.
    ///
    /// Exists so the detached task can return something `Sendable`: the core's
    /// `LibraryBuildStructure` is not, and making it the hop's payload would have
    /// forced the id resolution and the suggestion mapping back onto the main
    /// actor — the two costs measured at 14–22 ms and a 20k-entry
    /// dictionary walk.
    private struct Built: Sendable {
        let sorted: [PhotoFile]
        let tags: [TagSuggestion]
        let people: [TagSuggestion]
        let fingerprint: LibraryFingerprint
        let marshalMillis: Double
        let coreMillis: Double
    }

    private struct LibraryFingerprint: Equatable, Sendable {
        let ids: [UUID]

        init(photos: [PhotoFile]) {
            ids = photos.map(\.id)
        }
    }

    /// Core ids → the app's photo structs, dropping ids the table no longer
    /// knows (a rebuild that has not landed yet, or a photo removed since).
    private func resolve(_ ids: [String]) -> [PhotoFile] {
        ids.compactMap { UUID(uuidString: $0).flatMap { photoByID[$0] } }
    }

    /// `nonisolated` because it runs inside the detached build task — it is a
    /// pure field-for-field copy and has no business hopping back.
    nonisolated private static func suggestion(from record: TagStructureItem) -> TagSuggestion {
        TagSuggestion(
            id: record.id,
            displayName: record.displayName,
            fullPath: record.fullPath,
            namespace: record.namespace,
            count: Int(record.count),
            latestPhotoDate: record.latestPhotoDate.map(Date.init(timeIntervalSinceReferenceDate:))
        )
    }

    private func exportedFolders() -> [ScannedFolderHost] {
        if let cached = exportedFoldersCache { return cached }
        let folders = core.exportFolders()
        exportedFoldersCache = folders
        return folders
    }

    private func folderTime(_ folderID: String, _ keyPath: KeyPath<ScannedFolderHost, Double?>) -> Double {
        folderHost(folderID)?[keyPath: keyPath] ?? -.greatestFiniteMagnitude
    }

    private func textRows(
        structure: ViewStructure,
        sectionID: String,
        window: (String, UInt64, UInt64, UInt64) throws -> [GalleryTextRow],
        refresh: () -> ViewStructure
    ) -> [GalleryTextRow] {
        if let rows = pageTextRows(structure: structure, sectionID: sectionID, window: window) {
            return rows
        }
        return pageTextRows(structure: refresh(), sectionID: sectionID, window: window) ?? []
    }

    private func pageTextRows(
        structure: ViewStructure,
        sectionID: String,
        window: (String, UInt64, UInt64, UInt64) throws -> [GalleryTextRow]
    ) -> [GalleryTextRow]? {
        guard let section = structure.sections.first(where: { $0.id == sectionID }) else {
            return []
        }
        let total = section.itemIds.count
        var rows: [GalleryTextRow] = []
        rows.reserveCapacity(total)
        var offset = 0
        while offset < total {
            let limit = min(Int(listingPageSize), total - offset)
            do {
                let chunk = try window(
                    sectionID,
                    UInt64(offset),
                    UInt64(limit),
                    structure.generation
                )
                if chunk.isEmpty { break }
                rows.append(contentsOf: chunk)
                offset += chunk.count
            } catch let error as ViewError {
                if case .StaleGeneration = error { return nil }
                break
            } catch {
                break
            }
        }
        return rows
    }

    private func clearListingMemos() {
        clearFolderMemos()
        peopleListingCache = nil
        peopleRailCache = nil
        collectionListingCache = [:]
        listingEpoch += 1
    }

    private func clearFolderMemos() {
        folderListingCache = [:]
        folderPhotoIDCache = [:]
        exportedFoldersCache = nil
    }
}
