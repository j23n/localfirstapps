//! [`StdVfs`] wrapper that refuses paths outside a selected folder.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::path::is_under_root;
use crate::std_vfs::{resolve_symlink, StdVfs};
use crate::{Entry, ReadSeek, Stat, Vfs, VfsError, VfsResult};

const ESCAPE_REASON: &str = "path escapes the selected folder";

/// A [`StdVfs`] that refuses paths (and write/open symlink targets) outside
/// a selected folder.
///
/// [`StdVfs`] itself stays unconfined — gallery XMP sidecars still write
/// through file symlinks via the raw type. Contacts and music wrap that
/// type here so a path or link cannot leave the user-selected root.
#[derive(Debug, Clone)]
pub struct ConfinedVfs {
    inner: StdVfs,
    root: String,
}

impl ConfinedVfs {
    /// Confine writes and reads to `root`.
    ///
    /// Trailing separators are trimmed. When `std::fs::canonicalize` succeeds
    /// with a UTF-8 path, that spelling is stored; otherwise the cleaned
    /// string is kept. [`StdVfs::new`] is unchanged — this is a wrapper.
    pub fn new(temp_prefix: &'static str, root: impl Into<String>) -> VfsResult<Self> {
        let raw = root.into();
        if raw.is_empty() {
            return Err(VfsError::InvalidPath {
                path: raw,
                reason: "empty root".into(),
            });
        }
        let cleaned = clean_root(&raw);
        let stored = match fs::canonicalize(&cleaned) {
            Ok(canon) => match canon.to_str() {
                Some(s) => s.to_string(),
                None => cleaned,
            },
            Err(_) => cleaned,
        };
        Ok(Self {
            inner: StdVfs::new(temp_prefix),
            root: stored,
        })
    }

    /// The cleaned (and possibly canonical) folder this wrapper stays inside.
    pub fn root(&self) -> &str {
        &self.root
    }

    fn inside(&self, path: &str) -> bool {
        is_under_root(&self.root, path)
    }

    fn deny_if_outside(&self, path: &str) -> VfsResult<()> {
        if self.inside(path) {
            return Ok(());
        }
        if let Ok(canon) = fs::canonicalize(path) {
            if let Some(s) = canon.to_str() {
                if self.inside(s) {
                    return Ok(());
                }
            }
        }
        Err(VfsError::InvalidPath {
            path: path.to_string(),
            reason: ESCAPE_REASON.into(),
        })
    }

    fn deny_if_outside_resolved(&self, path: &str) -> VfsResult<()> {
        let resolved = resolve_components(Path::new(path))?;
        let Some(resolved_str) = resolved.to_str() else {
            return Err(VfsError::InvalidPath {
                path: path.to_string(),
                reason: "path is not valid UTF-8".into(),
            });
        };
        if self.inside(resolved_str) {
            return Ok(());
        }
        // Lexical fallback only when nothing resolved differently
        // (a brand-new path). A symlink that left the folder must fail.
        if resolved.as_path() == Path::new(path) {
            return self.deny_if_outside(path);
        }
        Err(VfsError::InvalidPath {
            path: path.to_string(),
            reason: ESCAPE_REASON.into(),
        })
    }
}

/// Follow every existing component, including intermediate directory symlinks.
///
/// A missing leaf is left as joined onto the resolved prefix so a new file
/// under a symlink-spelled root (macOS `/tmp` → `/private/tmp`) still
/// compares against the stored canonical folder.
fn resolve_components(path: &Path) -> VfsResult<PathBuf> {
    let mut resolved = PathBuf::new();
    for component in path.components() {
        resolved.push(component);
        resolved = resolve_symlink(&resolved)?;
    }
    Ok(resolved)
}

fn clean_root(root: &str) -> String {
    let trimmed = root.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        if root.contains('\\') && !root.contains('/') {
            return "\\".into();
        }
        return "/".into();
    }
    trimmed.to_string()
}

impl Vfs for ConfinedVfs {
    fn open(&self, path: &str) -> VfsResult<Box<dyn ReadSeek + Send>> {
        self.deny_if_outside_resolved(path)?;
        self.inner.open(path)
    }

    fn stat(&self, path: &str) -> VfsResult<Stat> {
        self.deny_if_outside_resolved(path)?;
        self.inner.stat(path)
    }

    fn list(&self, dir: &str) -> VfsResult<Vec<Entry>> {
        self.deny_if_outside_resolved(dir)?;
        self.inner.list(dir)
    }

    fn stat_entry(&self, path: &str) -> VfsResult<Entry> {
        self.deny_if_outside_resolved(path)?;
        self.inner.stat_entry(path)
    }

    fn create_dir_all(&self, dir: &str) -> VfsResult<()> {
        self.deny_if_outside_resolved(dir)?;
        self.inner.create_dir_all(dir)
    }

    fn append(&self, path: &str, bytes: &[u8]) -> VfsResult<()> {
        self.deny_if_outside_resolved(path)?;
        self.inner.append(path, bytes)
    }

    fn write_atomic(&self, path: &str, bytes: &[u8]) -> VfsResult<()> {
        self.deny_if_outside_resolved(path)?;
        self.inner.write_atomic(path, bytes)
    }

    fn write_atomic_from(&self, path: &str, reader: &mut dyn Read) -> VfsResult<u64> {
        self.deny_if_outside_resolved(path)?;
        self.inner.write_atomic_from(path, reader)
    }

    fn exists(&self, path: &str) -> bool {
        self.deny_if_outside_resolved(path).is_ok() && self.inner.exists(path)
    }

    fn remove(&self, path: &str) -> VfsResult<()> {
        self.deny_if_outside_resolved(path)?;
        self.inner.remove(path)
    }

    fn rename(&self, from: &str, to: &str) -> VfsResult<()> {
        self.deny_if_outside_resolved(from)?;
        self.deny_if_outside_resolved(to)?;
        self.inner.rename(from, to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::join;
    use std::fs;

    const PREFIX: &str = ".gallery-tmp-";

    fn confined(dir: &tempfile::TempDir) -> ConfinedVfs {
        ConfinedVfs::new(PREFIX, dir.path().to_str().unwrap()).unwrap()
    }

    #[test]
    fn write_and_read_under_root_succeed() {
        let dir = tempfile::tempdir().unwrap();
        let vfs = confined(&dir);
        let path = join(vfs.root(), "note.txt");
        vfs.write_atomic(&path, b"hi").unwrap();
        assert_eq!(vfs.read(&path).unwrap(), b"hi");
    }

    #[test]
    fn relative_dotdot_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let vfs = confined(&dir);
        let escaped = join(vfs.root(), "../evil.txt");
        let err = vfs.write_atomic(&escaped, b"x").unwrap_err();
        match err {
            VfsError::InvalidPath { reason, .. } => assert_eq!(reason, ESCAPE_REASON),
            other => panic!("{other:?}"),
        }
        assert!(!vfs.exists(&escaped));
    }

    #[cfg(unix)]
    #[test]
    fn write_through_a_symlink_out_of_root_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let real = outside.path().join("real.xmp");
        fs::write(&real, b"old").unwrap();
        let link = dir.path().join("link.xmp");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let vfs = confined(&dir);
        let err = vfs
            .write_atomic(link.to_str().unwrap(), b"new")
            .unwrap_err();
        match err {
            VfsError::InvalidPath { reason, .. } => assert_eq!(reason, ESCAPE_REASON),
            other => panic!("{other:?}"),
        }
        assert_eq!(fs::read(&real).unwrap(), b"old");
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the symlink must survive a refused write"
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_through_a_symlink_inside_root_still_follows() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.xmp");
        let link = dir.path().join("link.xmp");
        fs::write(&real, b"old").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let vfs = confined(&dir);
        vfs.write_atomic(link.to_str().unwrap(), b"new").unwrap();
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read(&real).unwrap(), b"new");
    }

    #[test]
    fn new_rejects_an_empty_root() {
        let err = ConfinedVfs::new(PREFIX, "").unwrap_err();
        assert!(matches!(err, VfsError::InvalidPath { .. }), "{err:?}");
    }

    #[cfg(unix)]
    #[test]
    fn new_file_under_a_symlinked_root_uses_the_link_spelling() {
        let real = tempfile::tempdir().unwrap();
        let holder = tempfile::tempdir().unwrap();
        let link = holder.path().join("alias");
        std::os::unix::fs::symlink(real.path(), &link).unwrap();

        let vfs = ConfinedVfs::new(PREFIX, link.to_str().unwrap()).unwrap();
        let path = join(link.to_str().unwrap(), "note.txt");
        vfs.write_atomic(&path, b"hi").unwrap();
        assert_eq!(fs::read(real.path().join("note.txt")).unwrap(), b"hi");
    }

    #[cfg(unix)]
    #[test]
    fn intermediate_directory_symlink_out_of_root_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let nested = dir.path().join(".contacts");
        std::os::unix::fs::symlink(outside.path(), &nested).unwrap();

        let vfs = confined(&dir);
        let path = join(
            join(dir.path().to_str().unwrap(), ".contacts").as_str(),
            "escape.txt",
        );
        let err = vfs.write_atomic(&path, b"x").unwrap_err();
        match err {
            VfsError::InvalidPath { reason, .. } => assert_eq!(reason, ESCAPE_REASON),
            other => panic!("{other:?}"),
        }
        assert!(!outside.path().join("escape.txt").exists());
    }
}
