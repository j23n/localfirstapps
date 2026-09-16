//! Compatibility facade over localcore-vfs. Gallery temp prefix is `.gallery-tmp-`.

#![forbid(unsafe_code)]

pub use localcore_vfs::{
    take_unsupported_names, Entry, EntryKind, FileTime, MemVfs, ReadSeek, Stat, Vfs, VfsError,
    VfsResult,
};

/// Name of the temp file [`Vfs::write_atomic`] uses, so listings can skip it.
pub const TEMP_PREFIX: &str = ".gallery-tmp-";

/// [`localcore_vfs::StdVfs`] pinned to [`TEMP_PREFIX`].
///
/// Zero-sized so `&StdVfs` works in rustdoc and tests the way the pre-extract
/// unit struct did. `StdVfs::new()` is the same value.
#[derive(Debug, Clone, Copy, Default)]
pub struct StdVfs;

impl StdVfs {
    /// Construct one. Stateless; cloning is free.
    pub fn new() -> Self {
        Self
    }

    pub(crate) fn inner(self) -> localcore_vfs::StdVfs {
        localcore_vfs::StdVfs::new(TEMP_PREFIX)
    }
}

impl Vfs for StdVfs {
    fn open(&self, path: &str) -> VfsResult<Box<dyn ReadSeek + Send>> {
        self.inner().open(path)
    }
    fn stat(&self, path: &str) -> VfsResult<Stat> {
        self.inner().stat(path)
    }
    fn list(&self, dir: &str) -> VfsResult<Vec<Entry>> {
        self.inner().list(dir)
    }
    fn stat_entry(&self, path: &str) -> VfsResult<Entry> {
        self.inner().stat_entry(path)
    }
    fn create_dir_all(&self, dir: &str) -> VfsResult<()> {
        self.inner().create_dir_all(dir)
    }
    fn append(&self, path: &str, bytes: &[u8]) -> VfsResult<()> {
        self.inner().append(path, bytes)
    }
    fn write_atomic(&self, path: &str, bytes: &[u8]) -> VfsResult<()> {
        self.inner().write_atomic(path, bytes)
    }
    fn try_exists(&self, path: &str) -> VfsResult<bool> {
        self.inner().try_exists(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.inner().exists(path)
    }
    fn read(&self, path: &str) -> VfsResult<Vec<u8>> {
        self.inner().read(path)
    }
    fn remove(&self, path: &str) -> VfsResult<()> {
        self.inner().remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> VfsResult<()> {
        self.inner().rename(from, to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_std_vfs_uses_the_gallery_prefix() {
        assert_eq!(StdVfs.inner().temp_prefix(), TEMP_PREFIX);
        assert_eq!(StdVfs::new().inner().temp_prefix(), TEMP_PREFIX);
    }

    #[test]
    fn facade_exposes_fallible_existence_checks() {
        let dir = tempfile::tempdir().unwrap();
        let present = dir.path().join("present");
        std::fs::write(&present, b"x").unwrap();

        assert!(StdVfs.try_exists(present.to_str().unwrap()).unwrap());
        assert!(!StdVfs
            .try_exists(dir.path().join("missing").to_str().unwrap())
            .unwrap());
    }
}
