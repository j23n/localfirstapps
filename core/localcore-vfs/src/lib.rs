//! Filesystem abstraction for `localcore`.
//!
//! App cores never touch `std::fs` directly; they go through [`Vfs`] so the
//! same code runs against a real directory ([`StdVfs`]) and an in-memory tree
//! ([`MemVfs`], tests).
//!
//! The trait is path-based: on iOS/simulator Swift resolves the
//! security-scoped root and starts access before calling in, so the core
//! only ever sees plain paths under an active scope.
//!
//! # Writes
//!
//! [`Vfs::write_atomic`] is the content write: temp file plus rename, so
//! readers see the old file or the complete new one. Sidecars are read
//! concurrently by `SidecarSyncService` and by cloud daemons, and a
//! half-written `.xmp` is indistinguishable from a corrupt one.
//!
//! [`Vfs::append`] is the log primitive (ADR 0005 R4). A log file is never
//! rewritten as a whole; a crash may leave a torn tail (R16).

#![forbid(unsafe_code)]

mod error;
mod mem;
mod std_vfs;

use std::io::{Read, Seek};

pub use error::{VfsError, VfsResult};
pub use mem::MemVfs;
pub use std_vfs::{take_unsupported_names, StdVfs};

/// A readable, seekable byte stream. Blanket-implemented, so `File`,
/// `Cursor<Vec<u8>>`, … all qualify.
pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// A wall-clock instant with sub-second precision, as the platform reports it.
///
/// Whole seconds plus nanoseconds rather than a single float because the two
/// consumers want different things: the snapshot encodes Apple's
/// seconds-since-2001 `Double`, and a `stat` is naturally integral.
///
/// # Why sub-seconds are carried
///
/// [`Stat::modified_unix`] drops them — it predates the scanner and only ever
/// fed 1-second comparisons. [`Entry`] must not: the light-scan cache-hit rule
/// is Swift's `cached.fileModificationDate == entry.modified`, and a Swift
/// `Date` is a `Double` of seconds, so two mtimes 500 ms apart are **not**
/// equal there. Truncating to whole seconds would make the Rust scanner
/// silently treat a modified file as unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct FileTime {
    /// Whole seconds since the Unix epoch. Negative before 1970.
    pub secs: i64,
    /// Sub-second remainder in nanoseconds, `0..1_000_000_000`.
    pub subsec_nanos: u32,
}

impl FileTime {
    /// Split a `SystemTime`-derived `(secs, nanos)` pair.
    pub fn new(secs: i64, subsec_nanos: u32) -> Self {
        FileTime { secs, subsec_nanos }
    }

    /// Seconds since the Unix epoch as a float, sub-seconds included.
    ///
    /// This is the form the scanner compares in, because it is the form
    /// Foundation's `Date` compares in: `Date` is a `Double`, so anything
    /// finer than its ~microsecond resolution at present-day magnitudes is
    /// invisible to Swift too.
    pub fn as_secs_f64(self) -> f64 {
        self.secs as f64 + f64::from(self.subsec_nanos) / 1e9
    }
}

/// Metadata about one filesystem entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stat {
    /// Size in bytes (0 for directories).
    pub size: u64,
    /// Last-modified time as whole seconds since the Unix epoch, when the
    /// platform reports one. Sub-second precision is deliberately dropped —
    /// it is not portable and the sidecar writer only ever compares at 1s.
    /// Directory enumeration goes through [`Entry`], which keeps them.
    pub modified_unix: Option<i64>,
    /// Whether the entry is a directory.
    pub is_dir: bool,
}

/// What a directory entry *is*.
///
/// Symlinks are reported as [`EntryKind::Symlink`] rather than resolved.
///
/// The scanner treats that split as policy, not decoration: a **file**
/// symlink is still a photo (size and mtime follow the target), and the
/// selected root may itself be a symlink, but a **directory** symlink is
/// never descended. Following one is how a walk leaves the selected root
/// or closes a cycle.
///
/// The *kind* is the only thing that stays unresolved. An entry's size and
/// timestamps come from the link's target — see [`Entry::size`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A directory.
    Dir,
    /// A symbolic link (target not resolved).
    Symlink,
}

/// One row of a directory listing.
///
/// Everything the scanner needs about a file arrives here, in **one** call per
/// directory. The trait stays per-directory so an implementation is free to
/// batch or parallelise metadata reads; the core never asks for one file at a
/// time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Final path component, byte-exact as the platform reports it.
    ///
    /// **Not normalized.** APFS hands back the spelling the file was created
    /// with, and `stable_uuid` hashes UTF-8 bytes, so normalizing here would
    /// change every id for NFC-named files arriving from outside.
    pub name: String,
    /// File, directory, or symlink.
    pub kind: EntryKind,
    /// Size in bytes. 0 for directories.
    ///
    /// For a [`EntryKind::Symlink`] this is the **target's** size, not the
    /// link's — `lstat` would report the length of the target path string, so
    /// a symlinked photo would arrive claiming to be a dozen bytes and its
    /// `(size, mtime)` change signal could never fire. The Swift baseline read
    /// these through `resourceValues(forKeys:)`, which follows. A dangling
    /// link falls back to the link's own values.
    pub size: u64,
    /// Last-modified time, when the platform reports one. Followed through a
    /// symlink, like [`Entry::size`].
    pub modified: Option<FileTime>,
    /// Creation ("birth") time, when the platform reports one.
    ///
    /// Not in the original Phase-3 sketch, and load-bearing anyway: the
    /// scanner's fallback capture date is `min(creation, modification)`
    /// (`MetadataReader.earliestFilesystemDate`), which cannot be computed
    /// without it.
    pub created: Option<FileTime>,
}

/// The filesystem seam.
///
/// Implementations must be `Send + Sync`: the core runs file IO on its own
/// thread pool and shares one `Vfs` across all workers.
pub trait Vfs: Send + Sync {
    /// Open `path` for reading.
    fn open(&self, path: &str) -> VfsResult<Box<dyn ReadSeek + Send>>;

    /// Metadata for `path`. Symlinks are followed.
    fn stat(&self, path: &str) -> VfsResult<Stat>;

    /// Everything directly inside `dir`, one round trip.
    ///
    /// # Ordering
    ///
    /// **Unspecified**, and deliberately so. `FolderScanner` consumes
    /// `contentsOfDirectory` in whatever order the filesystem hands it back
    /// and sorts only the *subdirectories* (ascending
    /// `localizedStandardCompare`, applied by the caller, not here). The
    /// conformance fixtures pin folder order and explicitly leave
    /// within-folder photo order free; an implementation that sorts is legal
    /// but buys nothing.
    ///
    /// # Errors
    ///
    /// A directory that cannot be listed must fail rather than return an
    /// empty listing. The scanner's carry-forward depends on telling the two
    /// apart: an empty directory means "these photos are gone", a failed
    /// listing means "ask again later" (fixture landmine 20 — a transient I/O
    /// error must not look like a deletion).
    ///
    /// A single *entry* that cannot be read is the opposite case and must be
    /// **skipped**, not propagated: a file unlinked between the `readdir` and
    /// its `stat` is one row missing, and failing the directory over it would
    /// report every photo beside it as removed. `contentsOfDirectory` had no
    /// per-entry failure mode at all, and the baseline's per-file
    /// `resourceValues` was wrapped in `try?`.
    fn list(&self, dir: &str) -> VfsResult<Vec<Entry>>;

    /// The same record [`Vfs::list`] returns, for a single path.
    ///
    /// Deliberately *not* the bulk read path — calling it per file is the
    /// mistake docs/adr/0002 is about. The scanner calls it once per
    /// **directory**, to pick up the folder node's own timestamps, exactly
    /// where the Swift baseline calls `dirURL.resourceValues(forKeys:)`.
    /// [`Stat`] cannot serve: it has no creation time and no sub-seconds.
    fn stat_entry(&self, path: &str) -> VfsResult<Entry>;

    /// Create `dir` and any missing parents. Idempotent.
    ///
    /// Default errors so existing test `Vfs` impls still compile.
    fn create_dir_all(&self, dir: &str) -> VfsResult<()> {
        Err(VfsError::Io {
            path: dir.to_string(),
            message: "not implemented".into(),
        })
    }

    /// Append `bytes` to `path`, creating the file and parents if needed.
    ///
    /// The log primitive (ADR 0005 R4): the file is never rewritten as a
    /// whole. A crash may leave a torn tail (R16). Content writes stay on
    /// [`Vfs::write_atomic`].
    ///
    /// Default errors so existing test `Vfs` impls still compile.
    fn append(&self, path: &str, bytes: &[u8]) -> VfsResult<()> {
        let _ = bytes;
        Err(VfsError::Io {
            path: path.to_string(),
            message: "not implemented".into(),
        })
    }

    /// Write `bytes` to `path` such that readers see either the old contents
    /// or the complete new contents — never anything in between.
    ///
    /// Implementations write to a temporary file **in the same directory** as
    /// `path` and rename it into place; a rename across filesystems is not
    /// atomic, and `/tmp` is frequently a different volume.
    ///
    /// # Guarantees when the file already exists
    ///
    /// - **Permissions are preserved.** A fresh temp file would otherwise pick
    ///   up the process umask, so replacing a 0600 sidecar would widen it.
    /// - **Symlinks are followed, not replaced.** If `path` is a symlink the
    ///   bytes go to the file it points at; the link survives as a link.
    ///   Replacing it would leave the real file holding stale metadata while a
    ///   second, divergent copy appears where nothing looks for it.
    /// - **The rename is fsync'd**, contents first and then the containing
    ///   directory, so a crash cannot leave the name pointing at unwritten
    ///   blocks.
    ///
    /// # Known limitations (deliberately not fixed)
    ///
    /// - **Extended attributes are lost.** macOS quarantine flags, Finder tags
    ///   and cloud-provider xattrs live on the *inode*, and the replacement is
    ///   a new inode. Copying them across needs platform-specific calls
    ///   (`listxattr`/`getxattr`/`setxattr`) that this crate does not yet make.
    ///   Nothing in the app reads sidecar xattrs today; a Finder tag on an
    ///   `.xmp` is the realistic casualty.
    /// - **A crash mid-write orphans the temp file.** It is named
    ///   `<temp_prefix><pid>-<n>-<nanos>`, so listings skip it, but nothing
    ///   sweeps it afterwards. TODO: a startup sweep of leftover temps older
    ///   than an hour.
    fn write_atomic(&self, path: &str, bytes: &[u8]) -> VfsResult<()>;

    /// Whether `path` exists. Never fails — a path that cannot be stat'd for
    /// any reason reads as absent.
    fn exists(&self, path: &str) -> bool;

    /// Read all of `path` into memory.
    ///
    /// Provided so callers that only need the bytes (the sidecar reader) do
    /// not each reimplement the `open` + `read_to_end` dance. Implementations
    /// backed by a whole-file store may override it.
    fn read(&self, path: &str) -> VfsResult<Vec<u8>> {
        let mut reader = self.open(path)?;
        let mut buf = Vec::new();
        reader
            .read_to_end(&mut buf)
            .map_err(|e| VfsError::from_io(path, &e))?;
        Ok(buf)
    }

    /// Delete `path`. Directories are removed recursively.
    ///
    /// The default answers [`VfsError::Io`] with `"not implemented"` so
    /// existing test `Vfs` impls still compile.
    fn remove(&self, path: &str) -> VfsResult<()> {
        Err(VfsError::Io {
            path: path.to_string(),
            message: "not implemented".into(),
        })
    }

    /// Rename `from` to `to`.
    ///
    /// The default answers [`VfsError::Io`] with `"not implemented"` so
    /// existing test `Vfs` impls still compile.
    fn rename(&self, from: &str, to: &str) -> VfsResult<()> {
        let _ = to;
        Err(VfsError::Io {
            path: from.to_string(),
            message: "not implemented".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `Vfs` impl must satisfy this. Called from each impl's test module.
    pub(crate) fn assert_vfs_contract(vfs: &dyn Vfs, dir: &str) {
        let file = format!("{dir}/contract.txt");

        assert!(!vfs.exists(&file));
        assert_eq!(
            vfs.stat(&file),
            Err(VfsError::NotFound { path: file.clone() })
        );

        vfs.write_atomic(&file, b"hello").unwrap();
        assert!(vfs.exists(&file));
        assert_eq!(vfs.read(&file).unwrap(), b"hello");
        assert_eq!(vfs.stat(&file).unwrap().size, 5);
        assert!(!vfs.stat(&file).unwrap().is_dir);

        // Overwrite replaces wholesale, not appends.
        vfs.write_atomic(&file, b"bye").unwrap();
        assert_eq!(vfs.read(&file).unwrap(), b"bye");
        assert_eq!(vfs.stat(&file).unwrap().size, 3);

        // Empty writes are legal.
        vfs.write_atomic(&file, b"").unwrap();
        assert_eq!(vfs.read(&file).unwrap(), b"");

        assert!(vfs.stat(dir).unwrap().is_dir);

        // `list` sees what was written, with the size the stat reports.
        let listing = vfs.list(dir).unwrap();
        let entry = listing
            .iter()
            .find(|e| e.name == "contract.txt")
            .expect("the file just written is missing from the listing");
        assert_eq!(entry.kind, EntryKind::File);
        assert_eq!(entry.size, 0);

        // Listing a file, or something absent, is an error — never an empty
        // listing. The scanner tells "gone" from "unreadable" by that.
        assert!(vfs.list(&file).is_err());
        assert!(vfs.list(&format!("{dir}/nope")).is_err());

        // `stat_entry` agrees with the listing, and knows about directories.
        let single = vfs.stat_entry(&file).unwrap();
        assert_eq!(single.name, "contract.txt");
        assert_eq!(single.kind, EntryKind::File);
        assert_eq!(single.size, 0);
        assert_eq!(vfs.stat_entry(dir).unwrap().kind, EntryKind::Dir);
        assert!(vfs.stat_entry(&format!("{dir}/nope")).is_err());

        let moved = format!("{dir}/renamed.txt");
        vfs.rename(&file, &moved).unwrap();
        assert!(!vfs.exists(&file));
        assert!(vfs.exists(&moved));
        assert_eq!(vfs.read(&moved).unwrap(), b"");

        vfs.remove(&moved).unwrap();
        assert!(!vfs.exists(&moved));
        assert!(vfs
            .list(dir)
            .unwrap()
            .iter()
            .all(|e| e.name != "renamed.txt"));

        let log = format!("{dir}/log.txt");
        vfs.append(&log, b"one\n").unwrap();
        vfs.append(&log, b"two\n").unwrap();
        assert_eq!(vfs.read(&log).unwrap(), b"one\ntwo\n");
        vfs.remove(&log).unwrap();
    }

    #[test]
    fn error_carries_path() {
        let e = VfsError::NotFound {
            path: "/a/b".into(),
        };
        assert_eq!(e.path(), "/a/b");
        assert_eq!(e.to_string(), "not found: /a/b");
    }
}
