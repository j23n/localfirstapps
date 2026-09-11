//! Compatibility facade over localcore-vfs. Gallery temp prefix is `.gallery-tmp-`.

#![forbid(unsafe_code)]

pub use localcore_vfs::{
    Entry, EntryKind, FileTime, MemVfs, ReadSeek, Stat, Vfs, VfsError, VfsResult,
    take_unsupported_names,
};

/// Name of the temp file [`Vfs::write_atomic`] uses, so listings can skip it.
pub const TEMP_PREFIX: &str = ".gallery-tmp-";

/// [`localcore_vfs::StdVfs`] pinned to [`TEMP_PREFIX`].
#[derive(Debug, Clone, Copy)]
pub struct StdVfs(localcore_vfs::StdVfs);

impl StdVfs {
    /// Construct one. Stateless; cloning is free.
    pub fn new() -> Self {
        Self(localcore_vfs::StdVfs::new(TEMP_PREFIX))
    }
}

impl Default for StdVfs {
    fn default() -> Self {
        Self::new()
    }
}

impl Vfs for StdVfs {
    fn open(&self, path: &str) -> VfsResult<Box<dyn ReadSeek + Send>> {
        self.0.open(path)
    }
    fn stat(&self, path: &str) -> VfsResult<Stat> {
        self.0.stat(path)
    }
    fn list(&self, dir: &str) -> VfsResult<Vec<Entry>> {
        self.0.list(dir)
    }
    fn stat_entry(&self, path: &str) -> VfsResult<Entry> {
        self.0.stat_entry(path)
    }
    fn write_atomic(&self, path: &str, bytes: &[u8]) -> VfsResult<()> {
        self.0.write_atomic(path, bytes)
    }
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn read(&self, path: &str) -> VfsResult<Vec<u8>> {
        self.0.read(path)
    }
    fn remove(&self, path: &str) -> VfsResult<()> {
        self.0.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> VfsResult<()> {
        self.0.rename(from, to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_std_vfs_uses_the_gallery_prefix() {
        let vfs = StdVfs::new();
        assert_eq!(vfs.0.temp_prefix(), TEMP_PREFIX);
    }
}
