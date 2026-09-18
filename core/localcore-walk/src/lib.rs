//! Generic directory walk (ADR 0002 R5 / R7).
//!
//! App cores supply a **content predicate** — typically an extension table.
//! This crate walks the tree, applies symlink policy, splits Syncthing
//! conflict copies out of the content stream, and reports per-entry
//! metadata. Classification beyond "is this content?" stays in the caller.
//!
//! ```
//! use localcore_vfs::MemVfs;
//! use localcore_walk::walk;
//!
//! let vfs = MemVfs::new();
//! vfs.insert("/lib/a.jpg", b"x");
//! vfs.insert("/lib/a.sync-conflict-20200901-120000-PHONE01.jpg", b"y");
//!
//! let out = walk(&vfs, "/lib", |name| name.ends_with(".jpg"));
//! assert_eq!(out.files.len(), 1);
//! assert_eq!(out.files[0].name, "a.jpg");
//! assert_eq!(out.conflict_groups.len(), 1);
//! ```
//!
//! # What the walk does
//!
//! - Depth-first, explicit stack. One [`Vfs::list`] per directory.
//! - A **file** symlink is a file (size and times follow the target). A
//!   **directory** symlink is not descended. The selected root may itself
//!   be a symlink: it is the start path, never a child entry.
//! - Names starting with `.` are skipped (not descended, not reported).
//!   Hidden-file policy lives here rather than on [`Vfs`].
//! - [`localcore_conflict::parse_name`] copies are reported as
//!   [`ConflictGroup`]s and are **never** content — they never go through
//!   the predicate and they are not counted as progress.
//! - `PermissionDenied` / other I/O: the directory is **failed**. The path
//!   is stored [decomposed](path_form::decomposed) (NFD). A node is still
//!   emitted so an unlistable root does not look missing.
//! - `NotFound`: not failed, treated as deletion. No node.
//! - Subfolders are ordered with [`localized_standard_compare`] (landmine 21).
//! - Progress is the count of *content* files seen, not conflicts.
//! - The cancel hook is checked at every directory boundary. `None` means
//!   stop; there is no partial outcome.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod order;
pub mod path_form;

use localcore_conflict::{groups, parse_name};
use localcore_vfs::{EntryKind, FileTime, Vfs, VfsError};

pub use localcore_conflict::{is_conflict_name, ConflictCopy, ConflictGroup};
pub use order::{localized_standard_compare, sort_names};
pub use path_form::decomposed;

/// How many content files accumulate between progress callbacks.
///
/// Matches the gallery scanner's batch: the callback often hops threads, and
/// firing per file made those hops dominate a large tree.
const PROGRESS_BATCH: usize = 500;

/// One visited directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkDirectory {
    /// Index of this node in [`WalkOutcome::directories`].
    pub index: usize,
    /// Parent directory index, or `None` for the walk root.
    pub parent_index: Option<usize>,
    /// Path as listed, not normalized.
    pub path: String,
    /// Final path component.
    pub name: String,
    /// Listing kind of the directory itself. Usually [`EntryKind::Dir`];
    /// a symlinked *root* may report [`EntryKind::Symlink`] if `stat_entry`
    /// does not follow.
    pub kind: EntryKind,
    /// Size from [`Vfs::stat_entry`]. Directories are typically 0.
    pub size: u64,
    /// Last-modified time of the directory, when the platform reports one.
    pub mtime: Option<FileTime>,
    /// Creation ("birth") time of the directory, when reported.
    pub created: Option<FileTime>,
    /// Child directory indices, already in [`localized_standard_compare`]
    /// order.
    pub child_indices: Vec<usize>,
    /// Indices into [`WalkOutcome::files`] for content in this directory,
    /// in listing order.
    pub file_indices: Vec<usize>,
}

/// One content file the caller's predicate accepted.
///
/// Conflict copies are not represented here. Dotfiles are not either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkFile {
    /// Path as joined from the parent listing, not normalized.
    pub path: String,
    /// Final path component, byte-exact as the listing reported it.
    pub name: String,
    /// Listing kind: [`EntryKind::File`] or a file [`EntryKind::Symlink`].
    pub kind: EntryKind,
    /// Size in bytes. For a file symlink this is the target's size.
    pub size: u64,
    /// Last-modified time, when reported. Followed through a symlink.
    pub mtime: Option<FileTime>,
    /// Creation time, when reported. Followed through a symlink.
    pub created: Option<FileTime>,
    /// Index of the containing [`WalkDirectory`].
    pub directory_index: usize,
}

/// Timings and counters for one walk.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WalkStats {
    /// Directories visited, listable or not.
    pub folders: u64,
    /// Wall time inside [`Vfs::list`].
    pub list_micros: u64,
    /// Content files reported (not conflicts, not skipped names).
    pub content_files: u64,
}

/// Everything one walk produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkOutcome {
    /// Visited directories, in visit order. Empty when the root is
    /// [`VfsError::NotFound`].
    pub directories: Vec<WalkDirectory>,
    /// Content files, in visit order (listing order within a directory).
    pub files: Vec<WalkFile>,
    /// Conflict copies grouped by directory and surviving basename. Empty
    /// when none were seen.
    pub conflict_groups: Vec<ConflictGroup>,
    /// **Decomposed** (NFD) paths of directories whose listing failed
    /// with a transient error (`PermissionDenied`, other I/O). `NotFound`
    /// is omitted.
    pub failed_directory_paths: Vec<String>,
    /// Folder and list timings, plus the content-file count.
    pub stats: WalkStats,
}

/// Walk `root` and report directories, content files, and conflict groups.
///
/// `is_content` is called with a basename. It is never invoked for
/// conflict copies, dotfiles, or directories.
pub fn walk(vfs: &dyn Vfs, root: &str, is_content: impl Fn(&str) -> bool) -> WalkOutcome {
    walk_with_progress(vfs, root, &is_content, None)
}

/// [`walk`], with a callback invoked every [`PROGRESS_BATCH`] content files
/// and once at the end with the true total.
///
/// The number is content files seen, not conflicts.
pub fn walk_with_progress(
    vfs: &dyn Vfs,
    root: &str,
    is_content: &dyn Fn(&str) -> bool,
    on_progress: Option<&dyn Fn(usize)>,
) -> WalkOutcome {
    walk_with_hooks(vfs, root, is_content, on_progress, None)
        .expect("a walk with no cancel hook cannot be cancelled")
}

/// [`walk_with_progress`], plus a cancellation hook checked at every
/// directory boundary.
///
/// `None` means the caller asked to stop. There is no partial outcome: a
/// half-walked tree is indistinguishable from a tree whose second half was
/// deleted.
pub fn walk_with_hooks(
    vfs: &dyn Vfs,
    root: &str,
    is_content: &dyn Fn(&str) -> bool,
    on_progress: Option<&dyn Fn(usize)>,
    cancelled: Option<&dyn Fn() -> bool>,
) -> Option<WalkOutcome> {
    let _span = localcore_trace::span_always("walk", "walk_with_hooks");
    let mut walker = Walker::new(vfs, is_content);
    let mut stack: Vec<(String, Option<usize>)> = vec![(root.to_string(), None)];

    while let Some((dir, parent)) = stack.pop() {
        if cancelled.is_some_and(|c| c()) {
            localcore_trace::event("walk", "cancelled");
            return None;
        }
        let Some(mut subdirs) = walker.visit_directory(&dir, parent) else {
            continue;
        };
        if let Some(callback) = on_progress {
            walker.report(callback, false);
        }
        // Sorted ascending, pushed in reverse, so they pop ascending. Swift
        // sorts descending and pushes in order; same result, said once.
        subdirs.sort_by(|a, b| localized_standard_compare(&a.0, &b.0));
        let node_index = walker.directories.len() - 1;
        for (_, path) in subdirs.into_iter().rev() {
            stack.push((path, Some(node_index)));
        }
    }

    if let Some(callback) = on_progress {
        walker.report(callback, true);
    }
    let outcome = walker.finish();
    localcore_trace::event(
        "walk",
        format!(
            "done files={} folders={} failed={} conflicts={}",
            outcome.files.len(),
            outcome.directories.len(),
            outcome.failed_directory_paths.len(),
            outcome.conflict_groups.len()
        ),
    );
    Some(outcome)
}

struct Walker<'a> {
    vfs: &'a dyn Vfs,
    is_content: &'a dyn Fn(&str) -> bool,
    directories: Vec<WalkDirectory>,
    files: Vec<WalkFile>,
    conflict_paths: Vec<String>,
    failed_directory_paths: Vec<String>,
    progress_tick: usize,
    stats: WalkStats,
}

impl<'a> Walker<'a> {
    fn new(vfs: &'a dyn Vfs, is_content: &'a dyn Fn(&str) -> bool) -> Self {
        Walker {
            vfs,
            is_content,
            directories: Vec::new(),
            files: Vec::new(),
            conflict_paths: Vec::new(),
            failed_directory_paths: Vec::new(),
            progress_tick: 0,
            stats: WalkStats::default(),
        }
    }

    fn report(&mut self, callback: &dyn Fn(usize), force: bool) {
        if force || self.progress_tick >= PROGRESS_BATCH {
            callback(self.files.len());
            self.progress_tick = 0;
        }
    }

    /// Visit one directory, append its node, and return its subdirectories as
    /// `(name, path)` pairs for the caller to order and push.
    ///
    /// `None` means the directory is gone (`NotFound`): no node is appended.
    fn visit_directory(
        &mut self,
        dir: &str,
        parent_index: Option<usize>,
    ) -> Option<Vec<(String, String)>> {
        let mut subdirs: Vec<(String, String)> = Vec::new();
        let mut file_indices: Vec<usize> = Vec::new();

        self.stats.folders += 1;
        let listing_started = std::time::Instant::now();
        let listing = self.vfs.list(dir);
        self.stats.list_micros += listing_started.elapsed().as_micros() as u64;

        match listing {
            Ok(entries) => {
                for entry in entries {
                    if entry.name.starts_with('.') {
                        continue;
                    }
                    let path = join(dir, &entry.name);

                    if parse_name(&entry.name).is_some() {
                        // Never content, never descended, never an id.
                        self.conflict_paths.push(path);
                        continue;
                    }

                    let is_dir = match entry.kind {
                        EntryKind::Dir => true,
                        EntryKind::Symlink => {
                            self.vfs.stat(&path).map(|s| s.is_dir).unwrap_or(false)
                        }
                        EntryKind::File => false,
                    };
                    if is_dir {
                        if entry.kind == EntryKind::Symlink {
                            continue;
                        }
                        subdirs.push((entry.name, path));
                        continue;
                    }
                    if !(self.is_content)(&entry.name) {
                        continue;
                    }
                    let index = self.files.len();
                    self.files.push(WalkFile {
                        path,
                        name: entry.name,
                        kind: entry.kind,
                        size: entry.size,
                        mtime: entry.modified,
                        created: entry.created,
                        directory_index: self.directories.len(),
                    });
                    file_indices.push(index);
                }
            }
            Err(VfsError::NotFound { .. }) => return None,
            Err(_) => {
                self.failed_directory_paths.push(decomposed(dir));
            }
        }

        let dir_entry = self.vfs.stat_entry(dir).ok();
        let node_index = self.directories.len();
        self.directories.push(WalkDirectory {
            index: node_index,
            parent_index,
            path: dir.to_string(),
            name: last_component(dir).to_string(),
            kind: dir_entry.as_ref().map(|e| e.kind).unwrap_or(EntryKind::Dir),
            size: dir_entry.as_ref().map(|e| e.size).unwrap_or(0),
            mtime: dir_entry.as_ref().and_then(|e| e.modified),
            created: dir_entry.as_ref().and_then(|e| e.created),
            child_indices: Vec::new(),
            file_indices,
        });
        if let Some(parent) = parent_index {
            self.directories[parent].child_indices.push(node_index);
        }
        let added = self.directories[node_index].file_indices.len();
        self.progress_tick += added;
        self.stats.content_files += added as u64;
        Some(subdirs)
    }

    fn finish(self) -> WalkOutcome {
        let conflict_groups = groups(self.conflict_paths.iter().map(String::as_str));
        WalkOutcome {
            directories: self.directories,
            files: self.files,
            conflict_groups,
            failed_directory_paths: self.failed_directory_paths,
            stats: self.stats,
        }
    }
}

fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

fn last_component(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(i) => &trimmed[i + 1..],
        None => trimmed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use localcore_vfs::{Entry, MemVfs, StdVfs, Vfs};

    fn any_file(_: &str) -> bool {
        true
    }

    fn images_and_xmp(name: &str) -> bool {
        let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        matches!(
            ext.as_str(),
            "heic" | "jpg" | "jpeg" | "xmp" | "mov" | "png"
        )
    }

    fn names(files: &[WalkFile]) -> Vec<&str> {
        files.iter().map(|f| f.name.as_str()).collect()
    }

    #[test]
    fn the_predicate_selects_content_and_junk_is_dropped() {
        let vfs = MemVfs::new();
        vfs.insert("/lib/a.jpg", b"a");
        vfs.insert("/lib/readme.txt", b"no");
        vfs.insert("/lib/Nested/n.jpg", b"n");
        let out = walk(&vfs, "/lib", images_and_xmp);
        let mut found = names(&out.files);
        found.sort_unstable();
        assert_eq!(found, vec!["a.jpg", "n.jpg"]);
        assert_eq!(out.stats.content_files, 2);
        assert!(out.conflict_groups.is_empty());
    }

    #[test]
    fn conflict_copies_are_grouped_and_never_content() {
        let vfs = MemVfs::new();
        vfs.insert("/lib/photo.heic", b"ok");
        vfs.insert("/lib/photo.heic.xmp", b"x");
        vfs.insert(
            "/lib/photo.sync-conflict-20200901-120000-PHONE01.heic",
            b"c1",
        );
        vfs.insert(
            "/lib/photo.sync-conflict-20200902-130000-LAPTOP02.heic",
            b"c2",
        );
        vfs.insert(
            "/lib/photo.heic.sync-conflict-20200901-120000-PHONE01.xmp",
            b"x1",
        );

        let seen = std::cell::RefCell::new(Vec::new());
        let out = walk_with_progress(
            &vfs,
            "/lib",
            &images_and_xmp,
            Some(&|n| seen.borrow_mut().push(n)),
        );

        assert_eq!(names(&out.files), vec!["photo.heic", "photo.heic.xmp"]);
        assert_eq!(out.stats.content_files, 2);
        assert_eq!(
            seen.borrow().last().copied(),
            Some(2),
            "progress counts content, not the three conflict copies"
        );
        assert_eq!(out.conflict_groups.len(), 2);
        let canonical: Vec<&str> = out
            .conflict_groups
            .iter()
            .map(|g| g.canonical_name.as_str())
            .collect();
        assert_eq!(canonical, vec!["photo.heic", "photo.heic.xmp"]);
        for group in &out.conflict_groups {
            for copy in &group.copies {
                assert!(
                    out.files.iter().all(|f| f.path != copy.path),
                    "conflict {} leaked into content",
                    copy.path
                );
            }
        }
    }

    #[test]
    fn equal_conflict_basenames_in_subfolders_stay_separate() {
        let vfs = MemVfs::new();
        vfs.insert(
            "/lib/2024/photo.sync-conflict-20200901-120000-PHONE01.heic",
            b"one",
        );
        vfs.insert(
            "/lib/Archive/photo.sync-conflict-20200901-120000-PHONE01.heic",
            b"two",
        );

        let out = walk(&vfs, "/lib", images_and_xmp);
        let ids: Vec<_> = out.conflict_groups.iter().map(ConflictGroup::id).collect();
        assert_eq!(
            ids,
            vec![
                "/lib/2024/photo.heic".to_string(),
                "/lib/Archive/photo.heic".to_string(),
            ]
        );
    }

    #[test]
    fn gallery_minimal_fixture_splits_the_surviving_photo_from_the_copies() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../localcore-conflict/fixtures/trees/gallery-minimal");
        let root = root.to_str().expect("fixture path is UTF-8");
        let out = walk(&StdVfs::new(".walk-tmp-"), root, images_and_xmp);
        let mut found = names(&out.files);
        found.sort_unstable();
        assert_eq!(found, vec!["photo.heic", "photo.heic.xmp"]);
        assert_eq!(out.conflict_groups.len(), 2);
        assert_eq!(
            out.stats.content_files, 2,
            "the four conflict copies must not count as content"
        );
    }

    #[test]
    fn a_leading_dot_is_skipped_even_when_the_predicate_would_accept_it() {
        let vfs = MemVfs::new();
        vfs.insert("/lib/.hidden.jpg", b"h");
        vfs.insert("/lib/visible.jpg", b"v");
        let out = walk(&vfs, "/lib", any_file);
        assert_eq!(names(&out.files), vec!["visible.jpg"]);
    }

    #[test]
    fn subfolders_come_out_in_localized_standard_order() {
        let vfs = MemVfs::new();
        vfs.insert("/lib/Nested/n.jpg", b"n");
        vfs.insert("/lib/Media/m.jpg", b"m");
        vfs.insert("/lib/Junk/j.jpg", b"j");
        let out = walk(&vfs, "/lib", images_and_xmp);
        let root = &out.directories[0];
        let kids: Vec<&str> = root
            .child_indices
            .iter()
            .map(|&i| out.directories[i].name.as_str())
            .collect();
        assert_eq!(kids, vec!["Junk", "Media", "Nested"]);
    }

    #[test]
    fn a_missing_root_is_empty_and_not_failed() {
        let vfs = MemVfs::new();
        vfs.insert("/other/a.jpg", b"a");
        let out = walk(&vfs, "/lib", any_file);
        assert!(out.directories.is_empty());
        assert!(out.files.is_empty());
        assert!(out.failed_directory_paths.is_empty());
        assert_eq!(out.stats.folders, 1);
    }

    #[test]
    fn a_permission_denied_listing_records_a_decomposed_failed_directory() {
        struct DeniedVfs {
            inner: MemVfs,
            denied: String,
        }
        impl Vfs for DeniedVfs {
            fn open(
                &self,
                path: &str,
            ) -> localcore_vfs::VfsResult<Box<dyn localcore_vfs::ReadSeek + Send>> {
                self.inner.open(path)
            }
            fn stat(&self, path: &str) -> localcore_vfs::VfsResult<localcore_vfs::Stat> {
                self.inner.stat(path)
            }
            fn list(&self, dir: &str) -> localcore_vfs::VfsResult<Vec<Entry>> {
                if dir == self.denied {
                    return Err(VfsError::PermissionDenied {
                        path: dir.to_string(),
                    });
                }
                self.inner.list(dir)
            }
            fn stat_entry(&self, path: &str) -> localcore_vfs::VfsResult<Entry> {
                self.inner.stat_entry(path)
            }
            fn write_atomic(&self, path: &str, bytes: &[u8]) -> localcore_vfs::VfsResult<()> {
                self.inner.write_atomic(path, bytes)
            }
            fn exists(&self, path: &str) -> bool {
                self.inner.exists(path)
            }
        }

        let inner = MemVfs::new();
        let cafe = "/lib/caf\u{e9}";
        inner.insert(&format!("{cafe}/a.jpg"), b"a");
        let vfs = DeniedVfs {
            inner,
            denied: cafe.to_string(),
        };
        let out = walk(&vfs, "/lib", images_and_xmp);
        assert_eq!(
            out.failed_directory_paths,
            vec![decomposed(cafe)],
            "failed paths are NFD so they match standardizedFileURL.path"
        );
        assert!(out.files.is_empty());
        assert_eq!(out.directories.len(), 2, "unlistable dir still gets a node");
    }

    #[test]
    fn a_cancelled_walk_hands_back_nothing() {
        let vfs = MemVfs::new();
        vfs.insert("/lib/a.jpg", b"a");
        vfs.insert("/lib/Nested/n.jpg", b"n");
        assert_eq!(
            walk_with_hooks(&vfs, "/lib", &any_file, None, Some(&|| true)),
            None
        );
        let out = walk_with_hooks(&vfs, "/lib", &any_file, None, Some(&|| false));
        assert_eq!(out.unwrap().files.len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_symlink_is_not_descended_and_a_file_symlink_is_content() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("lib");
        let outside = dir.path().join("outside");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(root.join("inside.jpg"), b"in").unwrap();
        std::fs::write(outside.join("secret.jpg"), b"out").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();
        std::os::unix::fs::symlink(root.join("inside.jpg"), root.join("alias.jpg")).unwrap();
        std::os::unix::fs::symlink(&root, root.join("loop")).unwrap();

        let out = walk(
            &StdVfs::new(".walk-tmp-"),
            root.to_str().unwrap(),
            images_and_xmp,
        );
        let mut found = names(&out.files);
        found.sort_unstable();
        assert_eq!(found, vec!["alias.jpg", "inside.jpg"]);
        assert!(
            out.files
                .iter()
                .all(|f| !f.path.contains("/escape/") && !f.path.contains("/loop/")),
            "{:?}",
            names(&out.files)
        );
        let alias = out.files.iter().find(|f| f.name == "alias.jpg").unwrap();
        assert_eq!(alias.size, 2);
        assert_eq!(alias.kind, EntryKind::Symlink);
    }

    #[test]
    fn last_component_and_join_match_the_gallery_helpers() {
        assert_eq!(last_component("/a/b/c.jpg"), "c.jpg");
        assert_eq!(last_component("/"), "");
        assert_eq!(join("/lib", "a.jpg"), "/lib/a.jpg");
        assert_eq!(join("/lib/", "a.jpg"), "/lib/a.jpg");
    }
}
