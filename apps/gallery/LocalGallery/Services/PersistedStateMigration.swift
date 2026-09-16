import Foundation
import os

/// One-shot migrations for state whose keys were derived from filesystem
/// paths. Authoritative media and XMP sidecars are never touched here: every
/// mutated file is a rebuildable app/widget cache or an install-local cursor.
@MainActor
enum PersistedStateMigration {
    static let markerKey = "galleryPersistedStateMigrationVersion"
    static let currentVersion = 1

    /// Ordered so tests can simulate process death between durable steps.
    enum Stage: Int, CaseIterable {
        case sidecarCache
        case thumbnailCache
        case derivedSnapshots
        case libraryCache
    }

    enum MigrationError: Error, Equatable {
        case interrupted(after: Stage)
    }

    struct Outcome: Equatable {
        var migrated: Bool
        var photoIDChanges: Int
        var duplicatePhotosRemoved: Int
    }

    private struct LibraryEnvelope: Codable {
        var version: Int
        var value: LibrarySnapshot
    }

    private struct LibraryPlan {
        var envelope: LibraryEnvelope
        var photoIDs: [UUID: UUID]
        var folderIDs: [UUID: UUID]
        var validPhotoIDs: Set<UUID>
        var photoIDChanges: Int
        var duplicatePhotosRemoved: Int
    }

    /// Re-key the pre-M1 byte-exact state to NFC-derived ids. Dependent
    /// caches land first and `library_cache.json` last. Therefore every
    /// interruption can safely replay from either the old or the new library
    /// snapshot, and the marker is only persisted after all writes completed.
    @discardableResult
    static func run(
        paths: GalleryPaths,
        defaults: UserDefaults,
        interruptAfter: Stage? = nil
    ) throws -> Outcome {
        guard defaults.integer(forKey: markerKey) < currentVersion else {
            return Outcome(migrated: false, photoIDChanges: 0, duplicatePhotosRemoved: 0)
        }

        let plan = loadPlan(at: paths.libraryCacheURL)
        let photoIDs = plan?.photoIDs ?? [:]
        let folderIDs = plan?.folderIDs ?? [:]
        let validPhotoIDs = plan?.validPhotoIDs ?? []

        try migrateSidecarCache(
            at: paths.sidecarCacheURL,
            photoIDs: photoIDs,
            keeping: validPhotoIDs
        )
        try interruptIfRequested(.sidecarCache, requested: interruptAfter)

        try migrateThumbnails(
            in: paths.thumbnailDir,
            photoIDs: photoIDs,
            keeping: validPhotoIDs
        )
        try interruptIfRequested(.thumbnailCache, requested: interruptAfter)

        // Memories and widget payloads are projections containing old photo /
        // folder ids. Regeneration is safer than trying to repair every deep
        // reference. The user's memory suppression/recency maps are small and
        // can be re-keyed exactly for folder-derived memory ids.
        try removeIfPresent(paths.memoriesCacheURL)
        migrateInstallLocalReferences(
            defaults: defaults,
            photoIDs: photoIDs,
            folderIDs: folderIDs
        )
        if let widgetDataDir = paths.widgetDataDir {
            try removeIfPresent(widgetDataDir)
        }
        try interruptIfRequested(.derivedSnapshots, requested: interruptAfter)

        if let plan {
            let data = try JSONEncoder().encode(plan.envelope)
            try data.write(to: paths.libraryCacheURL, options: .atomic)
        }
        try interruptIfRequested(.libraryCache, requested: interruptAfter)

        defaults.set(currentVersion, forKey: markerKey)
        let outcome = Outcome(
            migrated: true,
            photoIDChanges: plan?.photoIDChanges ?? 0,
            duplicatePhotosRemoved: plan?.duplicatePhotosRemoved ?? 0
        )
        Log.cache.info(
            "M1 persisted-state migration complete: \(outcome.photoIDChanges) photo ids re-keyed, "
                + "\(outcome.duplicatePhotosRemoved) duplicate rows removed"
        )
        return outcome
    }

    private static func interruptIfRequested(_ stage: Stage, requested: Stage?) throws {
        if requested == stage {
            throw MigrationError.interrupted(after: stage)
        }
    }

    // MARK: Library snapshot

    /// Decode the path-bearing snapshot before looking at any stored ids.
    /// A malformed/version-mismatched library cache has no usable key map;
    /// the ordinary cache loader will evict it, while this migration safely
    /// invalidates every dependent derived cache.
    private static func loadPlan(at url: URL) -> LibraryPlan? {
        guard let data = try? Data(contentsOf: url),
              let decoded = try? JSONDecoder().decode(LibraryEnvelope.self, from: data),
              decoded.version == LibrarySnapshot.version else {
            return nil
        }

        var photoIDs: [UUID: UUID] = [:]
        var canonicalPhotos: [UUID: PhotoFile] = [:]
        var photoOrder: [UUID] = []
        for photo in decoded.value.allPhotos {
            let migrated = photo.relocated(to: photo.url, livePhotoVideoURL: photo.livePhotoVideoURL)
            photoIDs[photo.id] = migrated.id
            if let existing = canonicalPhotos[migrated.id] {
                canonicalPhotos[migrated.id] = preferred(existing, migrated)
            } else {
                canonicalPhotos[migrated.id] = migrated
                photoOrder.append(migrated.id)
            }
        }
        let allPhotos = photoOrder.compactMap { canonicalPhotos[$0] }

        var folderIDs: [UUID: UUID] = [:]
        collectFolderIDs(decoded.value.rootFolder, into: &folderIDs)
        let root = migrateFolder(
            decoded.value.rootFolder,
            canonicalPhotos: canonicalPhotos
        )

        let validPhotoIDs = Set(allPhotos.map(\.id))
        var seenManifest = Set<UUID>()
        let manifest = decoded.value.sidecarManifest.map { rows in
            rows.compactMap { row -> SidecarCandidate? in
                let newID = photoIDs[row.photoID] ?? row.photoID
                guard validPhotoIDs.contains(newID), seenManifest.insert(newID).inserted else {
                    return nil
                }
                return SidecarCandidate(
                    photoID: newID,
                    sidecarURL: row.sidecarURL,
                    currentVersion: row.currentVersion,
                    downloadStatus: row.downloadStatus
                )
            }
        }

        let changes = photoIDs.reduce(into: 0) { count, pair in
            if pair.key != pair.value { count += 1 }
        }
        let migrated = LibrarySnapshot(
            rootFolder: root,
            allPhotos: allPhotos,
            sidecarManifest: manifest
        )
        return LibraryPlan(
            envelope: LibraryEnvelope(version: decoded.version, value: migrated),
            photoIDs: photoIDs,
            folderIDs: folderIDs,
            validPhotoIDs: validPhotoIDs,
            photoIDChanges: changes,
            duplicatePhotosRemoved: decoded.value.allPhotos.count - allPhotos.count
        )
    }

    private static func preferred(_ lhs: PhotoFile, _ rhs: PhotoFile) -> PhotoFile {
        let lhsDate = lhs.fileModificationDate ?? lhs.enrichedFileDate ?? .distantPast
        let rhsDate = rhs.fileModificationDate ?? rhs.enrichedFileDate ?? .distantPast
        if lhsDate != rhsDate { return lhsDate > rhsDate ? lhs : rhs }
        let lhsRichness = lhs.hierarchicalTags.count + lhs.faceRegions.count
            + (lhs.countryCode == nil ? 0 : 1) + (lhs.dateTaken == nil ? 0 : 1)
        let rhsRichness = rhs.hierarchicalTags.count + rhs.faceRegions.count
            + (rhs.countryCode == nil ? 0 : 1) + (rhs.dateTaken == nil ? 0 : 1)
        return lhsRichness >= rhsRichness ? lhs : rhs
    }

    private static func collectFolderIDs(_ folder: PhotoFolder, into ids: inout [UUID: UUID]) {
        ids[folder.id] = PhotoFolder.stableID(for: folder.url)
        for child in folder.subfolders {
            collectFolderIDs(child, into: &ids)
        }
    }

    private static func migrateFolder(
        _ source: PhotoFolder,
        canonicalPhotos: [UUID: PhotoFile]
    ) -> PhotoFolder {
        var seenPhotos = Set<UUID>()
        let photos = source.photos.compactMap { old -> PhotoFile? in
            let id = PhotoFile.stableID(for: old.url)
            guard seenPhotos.insert(id).inserted else { return nil }
            return canonicalPhotos[id]
                ?? old.relocated(to: old.url, livePhotoVideoURL: old.livePhotoVideoURL)
        }

        var children: [PhotoFolder] = []
        var childIndex: [UUID: Int] = [:]
        for oldChild in source.subfolders {
            let child = migrateFolder(oldChild, canonicalPhotos: canonicalPhotos)
            if let index = childIndex[child.id] {
                children[index] = mergeFolders(children[index], child)
            } else {
                childIndex[child.id] = children.count
                children.append(child)
            }
        }
        return makeFolder(
            source: source,
            photos: photos,
            subfolders: children
        )
    }

    private static func mergeFolders(_ lhs: PhotoFolder, _ rhs: PhotoFolder) -> PhotoFolder {
        var seenPhotos = Set<UUID>()
        let photos = (lhs.photos + rhs.photos).filter { seenPhotos.insert($0.id).inserted }
        var children = lhs.subfolders
        var childIndex = Dictionary(
            uniqueKeysWithValues: children.enumerated().map { ($0.element.id, $0.offset) }
        )
        for child in rhs.subfolders {
            if let index = childIndex[child.id] {
                children[index] = mergeFolders(children[index], child)
            } else {
                childIndex[child.id] = children.count
                children.append(child)
            }
        }
        return makeFolder(source: lhs, photos: photos, subfolders: children)
    }

    private static func makeFolder(
        source: PhotoFolder,
        photos: [PhotoFile],
        subfolders: [PhotoFolder]
    ) -> PhotoFolder {
        let cover = source.coverPhotoURL
            ?? photos.first?.url
            ?? subfolders.lazy.compactMap(\.coverPhotoURL).first
        return PhotoFolder(
            id: PhotoFolder.stableID(for: source.url),
            url: source.url,
            name: source.name,
            subfolders: subfolders,
            photos: photos,
            coverPhotoURL: cover,
            totalPhotoCount: photos.count + subfolders.reduce(0) { $0 + $1.totalPhotoCount },
            dateModified: source.dateModified,
            dateCreated: source.dateCreated
        )
    }

    // MARK: Dependent caches

    /// Rewrite the parsed-XMP cache's JSON object keys. Values are copied as
    /// opaque JSON, and unknown ids are garbage-collected. This file is only a
    /// cache; the adjacent authoritative `.xmp` files are outside every path
    /// this type can reach.
    private static func migrateSidecarCache(
        at url: URL,
        photoIDs: [UUID: UUID],
        keeping validPhotoIDs: Set<UUID>
    ) throws {
        guard FileManager.default.fileExists(atPath: url.path) else { return }
        guard let data = try? Data(contentsOf: url),
              let object = try? JSONSerialization.jsonObject(with: data),
              var root = object as? [String: Any],
              let entries = root["value"] as? [String: Any] else {
            try removeIfPresent(url)
            return
        }

        var migrated: [String: Any] = [:]
        for (rawID, value) in entries {
            guard let oldID = UUID(uuidString: rawID) else { continue }
            let newID = photoIDs[oldID] ?? oldID
            guard validPhotoIDs.contains(newID) else { continue }
            // Canonically equivalent duplicate paths converge. Keep the first
            // parsed document; the next sidecar sync re-reads authoritative XMP.
            if migrated[newID.uuidString] == nil {
                migrated[newID.uuidString] = value
            }
        }
        root["value"] = migrated
        let output = try JSONSerialization.data(
            withJSONObject: root,
            options: [.sortedKeys, .withoutEscapingSlashes]
        )
        try output.write(to: url, options: .atomic)
    }

    private static func migrateThumbnails(
        in directory: URL,
        photoIDs: [UUID: UUID],
        keeping validPhotoIDs: Set<UUID>
    ) throws {
        let fm = FileManager.default
        guard fm.fileExists(atPath: directory.path) else { return }

        for (oldID, newID) in photoIDs where oldID != newID {
            for ext in ["jpg", "stamp", "nothumb"] {
                let old = directory.appendingPathComponent(oldID.uuidString).appendingPathExtension(ext)
                guard fm.fileExists(atPath: old.path) else { continue }
                let new = directory.appendingPathComponent(newID.uuidString).appendingPathExtension(ext)
                if fm.fileExists(atPath: new.path) {
                    try fm.removeItem(at: old)
                } else {
                    do {
                        try fm.moveItem(at: old, to: new)
                    } catch {
                        // A thumbnail is derived data. If a provider/filesystem
                        // cannot rename it atomically, invalidating it is safer
                        // than leaving an old-id orphan.
                        try removeIfPresent(old)
                    }
                }
            }
        }

        let entries = try fm.contentsOfDirectory(
            at: directory,
            includingPropertiesForKeys: nil
        )
        for entry in entries {
            let base = entry.deletingPathExtension().lastPathComponent
            guard let id = UUID(uuidString: base), !validPhotoIDs.contains(id) else { continue }
            try removeIfPresent(entry)
        }
    }

    private static func migrateInstallLocalReferences(
        defaults: UserDefaults,
        photoIDs: [UUID: UUID],
        folderIDs: [UUID: UUID]
    ) {
        if let covers = defaults.dictionary(forKey: "featuredPhotoByPerson") as? [String: String] {
            let migrated = covers.compactMapValues { raw -> String? in
                guard let old = UUID(uuidString: raw) else { return nil }
                return (photoIDs[old] ?? old).uuidString
            }
            defaults.set(migrated, forKey: "featuredPhotoByPerson")
        }

        if let hidden = defaults.array(forKey: "hiddenMemories") as? [String] {
            defaults.set(hidden.map { migrateMemoryKey($0, folderIDs: folderIDs) }, forKey: "hiddenMemories")
        }
        for key in ["seenMemoryIDs", "surfacedClusters"] {
            guard let data = defaults.data(forKey: key),
                  let values = try? JSONDecoder().decode([String: Date].self, from: data) else {
                continue
            }
            var migrated: [String: Date] = [:]
            for (oldKey, date) in values {
                let newKey = migrateMemoryKey(oldKey, folderIDs: folderIDs)
                migrated[newKey] = max(migrated[newKey] ?? .distantPast, date)
            }
            if let encoded = try? JSONEncoder().encode(migrated) {
                defaults.set(encoded, forKey: key)
            }
        }
    }

    private static func migrateMemoryKey(_ key: String, folderIDs: [UUID: UUID]) -> String {
        let prefix = "folder-"
        guard key.hasPrefix(prefix),
              let old = UUID(uuidString: String(key.dropFirst(prefix.count))),
              let new = folderIDs[old] else {
            return key
        }
        return prefix + new.uuidString
    }

    private static func removeIfPresent(_ url: URL) throws {
        guard FileManager.default.fileExists(atPath: url.path) else { return }
        try FileManager.default.removeItem(at: url)
    }
}
