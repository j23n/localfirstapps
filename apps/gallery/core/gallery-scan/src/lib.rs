//! Folder traversal: the tree, the flat photo list, and the diff against the
//! last scan.
//!
//! Scan **policy** — light/full/auto resolution, the 48-hour promotion, the
//! dedupe of concurrent requests, the two-phase ordering, and the
//! sidecar-sync / memories / widget steps that follow a scan — stays in
//! `GalleryStore+Scanning.swift`. `localcore-walk` walks the tree; this crate
//! classifies what it found. Deciding *when* to walk is somebody else's job.
//!
//! ```no_run
//! use gallery_scan::{scan, ScanInput};
//! use gallery_vfs::StdVfs;
//!
//! let outcome = scan(&StdVfs::new(), "/photos", &ScanInput::default());
//! println!("{} photos, {} new", outcome.flat_photos.len(), outcome.added_paths.len());
//! ```
//!
//! # What is pinned here rather than decided here
//!
//! Traversal lives in `localcore-walk`. This crate classifies the walked
//! files (image / video / sidecar) and builds `PhotoFile`s. Conflict copies
//! never become photos.
//!
//! The conformance fixtures in `core/fixtures/scan-conformance/` are the spec.
//! Several pinned oddities remain — a standalone video's lowercased filename,
//! videos never getting a sidecar row, same-size/same-mtime rewrites staying
//! invisible. Light scans now compare listing size and mtime against the cache
//! rather than substituting cached stats. `tests/scanner_conformance.rs` runs
//! the same four passes and compares every field; the module docs on [`scan`]
//! explain each one where it happens.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod classify;
pub mod scan;

pub use classify::{classify, MediaKind, IMAGE_EXTENSIONS, VIDEO_EXTENSIONS};
pub use localcore_walk::{
    decomposed, localized_standard_compare, order, path_form, ConflictCopy, ConflictGroup,
};
pub use scan::{scan, scan_with_hooks, scan_with_progress, ScanInput, ScanOutcome, ScanStats};
