//! Compatibility facade over localcore-vfs. Gallery temp prefix is `.gallery-tmp-`.

#![forbid(unsafe_code)]

pub use localcore_vfs::{
    Entry, EntryKind, FileTime, MemVfs, ReadSeek, Stat, Vfs, VfsError, VfsResult,
    take_unsupported_names,
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
    fn write_atomic(&self, path: &str, bytes: &[u8]) -> VfsResult<()> {
        self.inner().write_atomic(path, bytes)
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
}
