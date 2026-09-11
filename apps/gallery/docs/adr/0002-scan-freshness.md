# ADR 0002: Scan freshness

- Status: Accepted
- Date: 2026-09-09
- Revised: 2026-09-12 (Phase 1 — file-provider surface retired)

## Context

Light scans that never `stat` known paths cannot see in-place edits.
Persisting the sidecar manifest inside the library snapshot avoids
re-statting every `.xmp` on each launch without a schema bump that
would force a full rescan.

Linux reuses a snapshot whenever one exists, so a "trust cache forever"
rule can hide edits indefinitely.

The original write-up was about iCloud File Provider XPC cost. That
surface is gone (family ADR 0005 R2). The remaining decisions are about
freshness, not providers.

## Decision

1. **The scanner reports the tree; hosts own policy.** Light / auto /
   full, the 48-hour promotion, request dedupe, and post-scan sidecar
   sync stay in the iOS store. Linux opens from the snapshot and
   watches the folder.
2. ~~**Provider attributes are the only Swift VFS callback.**~~
   **Struck (Phase 2).** `ProviderAttrs` / `probe_provider` are off the
   Rust `Vfs` trait. The scanner is always local. Generated
   `VfsProviderAttrs` and `ScannerSession(probe:)` remain on the FFI
   surface until a later commit drops them; the probe is ignored.
   Family ADR 0005 R2.
3. **A light pass may reuse a cached row only after a live size+mtime
   check.** Size or mtime change rebuilds the row. Content-preserving
   rewrites that keep both are invisible; a full / explicit rescan is
   the backstop. This *is* family ADR 0002 R5's definition of `light`.
4. **`sidecarManifest` is an optional field on snapshot v20.** A file
   written without it decodes as `nil` and pays one re-stat. Do not
   bump the snapshot version to add a cache hint.
5. **Sidecar cache eviction requires a successful parent listing.** A
   directory that failed to list must not look like "those sidecars
   were deleted." (Listing failure, not provider XPC.)
6. ~~**The probe must not return on the light path.**~~
   **Disposed (Phase 1).** There is no production probe. The light path
   still must not do extra per-file work "for uniformity"; size+mtime
   (decision 3) is the live check.

## Consequences

- Launch stays fast on an unchanged library; edits need a stat (light)
  or an explicit full rescan.
- Placeholders do not enter the projection. `ContentVersion` is
  size+mtime only; `downloadStatus` is omitted when `local`. Snapshot
  stays v20. Photos are local files; there is no on-demand materialize.
- Linux must not treat "snapshot exists" as "never look at disk again."
