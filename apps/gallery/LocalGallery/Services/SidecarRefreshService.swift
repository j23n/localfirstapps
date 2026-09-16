import Foundation
import os

/// Indirection between the `BGAppRefreshTask` handler in `AppDelegate` and the
/// `GalleryStore`. Mirror of `MemoryRefreshService` for the sidecar-sync
/// background task — same attach pattern, different work.
///
/// Foreground sync runs the first-time bulk fetch with prompts; this BG
/// service handles incremental top-up only. Per the plan, hard-cap at 200
/// sidecars per BG window and concurrency 4 (lower than foreground 8) to
/// stay polite under iOS BG limits. `runRefresh` awaits the fetch so the
/// `BGAppRefreshTask` is not marked complete until the work or its
/// cancellation has settled.
@MainActor
final class SidecarRefreshService {
    private weak var store: GalleryStore?

    func attach(_ store: GalleryStore) {
        self.store = store
    }

    func runRefresh() async {
        guard let store else {
            Log.bg.warning("SidecarRefreshService has no attached store; nothing to do")
            return
        }
        guard !store.lastSidecarManifest.isEmpty else {
            Log.bg.info("No sidecar manifest yet (no foreground scan); skipping BG sidecar refresh")
            return
        }
        // Re-probe every known sidecar URL. The persisted manifest can be
        // hours old; trusting its versions would skip a changed `.xmp` or
        // try to fetch one that is already gone.
        let refreshed: (manifest: [SidecarCandidate], gone: Set<UUID>)
        do {
            refreshed = try Self.refreshedManifest(store.lastSidecarManifest)
        } catch {
            Log.bg.error("Sidecar re-probe failed: \(Log.r.error(error))")
            return
        }
        store.lastSidecarManifest = refreshed.manifest
        if !refreshed.gone.isEmpty {
            Log.bg.info("Sidecar re-probe: \(refreshed.gone.count) sidecar(s) gone from disk")
        }

        // Auto-approve in BG — we can't surface a UI prompt from here.
        // Background hard limits still apply.
        let allIDs = Set(store.allPhotos.map(\.id))
        await store.sidecarSync.planAndRun(
            manifest: refreshed.manifest,
            allPhotoIDs: allIDs,
            autoApprove: true,
            policy: .background,
            listing: SidecarSyncService.Listing(
                isComplete: false,
                confirmedGone: refreshed.gone
            )
        )
    }

    /// Re-stat each candidate. Missing files become `gone`; surviving ones
    /// carry a fresh `ContentVersion` so the diff cannot trust a stale
    /// prior manifest. Filesystem probes are injectable for deterministic tests.
    nonisolated static func refreshedManifest(
        _ manifest: [SidecarCandidate],
        versionOf: @Sendable (URL) -> ContentVersion = {
            ContentVersion.ofFile(at: $0)
        },
        fileExists: @Sendable (URL) throws -> Bool = {
            try authoritativeExists($0)
        }
    ) throws -> (manifest: [SidecarCandidate], gone: Set<UUID>) {
        var fresh: [SidecarCandidate] = []
        var gone: Set<UUID> = []
        fresh.reserveCapacity(manifest.count)
        for candidate in manifest {
            guard try fileExists(candidate.sidecarURL) else {
                gone.insert(candidate.photoID)
                continue
            }
            let probed = versionOf(candidate.sidecarURL)
            let version = probed.isEmpty ? candidate.currentVersion : probed
            fresh.append(
                SidecarCandidate(
                    photoID: candidate.photoID,
                    sidecarURL: candidate.sidecarURL,
                    currentVersion: version
                )
            )
        }
        return (fresh, gone)
    }

    /// `false` means the path is missing. Permission and other I/O failures
    /// remain errors so background refresh cannot erase an authoritative row.
    private nonisolated static func authoritativeExists(_ url: URL) throws -> Bool {
        do {
            _ = try FileManager.default.attributesOfItem(atPath: url.path)
            return true
        } catch {
            let nsError = error as NSError
            if nsError.domain == NSCocoaErrorDomain,
               nsError.code == CocoaError.fileNoSuchFile.rawValue
                || nsError.code == CocoaError.fileReadNoSuchFile.rawValue
            {
                return false
            }
            throw error
        }
    }
}
