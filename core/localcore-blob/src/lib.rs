//! Content-addressed SHA-256 store.
//!
//! Layout: `{root}/blobs/sha256/ab/cd/<64-hex>`
//!
//! Ported from health `internal/blobs`. The Go package stays until Phase 6.
//! I/O goes through [`localcore_vfs::Vfs`].

#![forbid(unsafe_code)]

use localcore_vfs::{EntryKind, StdVfs, Vfs, VfsError};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Result of [`put`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PutResult {
    pub hash: String,
    pub size: u64,
    pub existed: bool,
}

/// One stored blob, as returned by [`list`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobInfo {
    pub sha256: String,
    pub size: u64,
    pub path: PathBuf,
}

/// Why a blob operation failed.
#[derive(Debug)]
pub enum Error {
    /// Digest is not 64 hexadecimal characters.
    InvalidHash,
    Io(io::Error),
    /// File at the content-addressed path hashes to a different digest.
    VerifyMismatch {
        hash: String,
        actual: String,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidHash => write!(f, "sha256 must be 64 hex chars"),
            Error::Io(e) => write!(f, "{e}"),
            Error::VerifyMismatch { hash, actual } => {
                write!(f, "blob {hash}: content hashes to {actual}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<VfsError> for Error {
    fn from(e: VfsError) -> Self {
        Error::Io(io::Error::new(io::ErrorKind::Other, e.to_string()))
    }
}

/// Temp prefix for Path wrappers over [`StdVfs`].
pub const TEMP_PREFIX: &str = ".localcore-blob-tmp-";

fn std_vfs() -> StdVfs {
    StdVfs::new(TEMP_PREFIX)
}

fn root_str(root: impl AsRef<Path>) -> Result<String> {
    root.as_ref().to_str().map(str::to_owned).ok_or_else(|| {
        Error::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "blob root is not UTF-8",
        ))
    })
}

static PUT_COUNTER: AtomicU64 = AtomicU64::new(0);

fn valid_sha256(h: &str) -> bool {
    h.len() == 64
        && h.bytes()
            .all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F'))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Store path for a hex SHA-256 digest.
pub fn path(root: impl AsRef<Path>, hash: &str) -> Result<PathBuf> {
    let h = hash.to_ascii_lowercase();
    if !valid_sha256(&h) {
        return Err(Error::InvalidHash);
    }
    Ok(root
        .as_ref()
        .join("blobs")
        .join("sha256")
        .join(&h[..2])
        .join(&h[2..4])
        .join(&h))
}

/// Store path for a hex SHA-256 digest, as a UTF-8 string.
pub fn path_on(root: &str, hash: &str) -> Result<String> {
    let h = hash.to_ascii_lowercase();
    if !valid_sha256(&h) {
        return Err(Error::InvalidHash);
    }
    let root = root.trim_end_matches('/');
    Ok(format!(
        "{root}/blobs/sha256/{}/{}/{}",
        &h[..2],
        &h[2..4],
        h
    ))
}

/// Whether the blob is already stored.
pub fn exists(root: impl AsRef<Path>, hash: &str) -> Result<bool> {
    exists_on(&std_vfs(), &root_str(root)?, hash)
}

/// [`exists`] through an explicit [`Vfs`].
pub fn exists_on(vfs: &dyn Vfs, root: &str, hash: &str) -> Result<bool> {
    Ok(vfs.exists(&path_on(root, hash)?))
}

/// Write `reader` into the store. `existed` is true when the digest was already present.
pub fn put(root: impl AsRef<Path>, reader: impl Read) -> Result<PutResult> {
    put_on(&std_vfs(), &root_str(root)?, reader)
}

/// [`put`] through an explicit [`Vfs`].
pub fn put_on(vfs: &dyn Vfs, root: &str, mut reader: impl Read) -> Result<PutResult> {
    let tmp_dir = format!("{}/blobs/tmp", root.trim_end_matches('/'));
    vfs.create_dir_all(&tmp_dir)?;
    let n = PUT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = format!("{tmp_dir}/put-{n}-{nanos}");
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let got = reader.read(&mut buf)?;
        if got == 0 {
            break;
        }
        hasher.update(&buf[..got]);
        vfs.append(&tmp, &buf[..got])?;
        size += got as u64;
    }
    if size == 0 {
        vfs.append(&tmp, b"")?;
    }
    let hash = encode_hex(&hasher.finalize());
    let dest = path_on(root, &hash)?;
    if vfs.exists(&dest) {
        let _ = vfs.remove(&tmp);
        return Ok(PutResult {
            hash,
            size,
            existed: true,
        });
    }
    if let Some(parent) = dest.rsplit_once('/').map(|(p, _)| p) {
        vfs.create_dir_all(parent)?;
    }
    vfs.rename(&tmp, &dest)?;
    Ok(PutResult {
        hash,
        size,
        existed: false,
    })
}

/// Open a stored blob.
pub fn open(root: impl AsRef<Path>, hash: &str) -> Result<File> {
    Ok(File::open(path(root, hash)?)?)
}

/// Walk `blobs/sha256/ab/cd/<hash>`, skipping tmp. Sorted by hash.
pub fn list(root: impl AsRef<Path>) -> Result<Vec<BlobInfo>> {
    list_on(&std_vfs(), &root_str(root)?)
}

/// [`list`] through an explicit [`Vfs`].
pub fn list_on(vfs: &dyn Vfs, root: &str) -> Result<Vec<BlobInfo>> {
    let base = format!("{}/blobs/sha256", root.trim_end_matches('/'));
    if !vfs.exists(&base) {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    walk_blobs(vfs, &base, &mut out)?;
    out.sort_by(|a, b| a.sha256.cmp(&b.sha256));
    Ok(out)
}

fn walk_blobs(vfs: &dyn Vfs, dir: &str, out: &mut Vec<BlobInfo>) -> Result<()> {
    for ent in vfs.list(dir)? {
        let path = format!("{}/{}", dir.trim_end_matches('/'), ent.name);
        if ent.kind == EntryKind::Dir {
            walk_blobs(vfs, &path, out)?;
            continue;
        }
        if !valid_sha256(&ent.name) {
            continue;
        }
        out.push(BlobInfo {
            sha256: ent.name.to_ascii_lowercase(),
            size: ent.size,
            path: PathBuf::from(path),
        });
    }
    Ok(())
}

/// Stream `path` and return its SHA-256 hex digest and size.
pub fn hash_file(path: impl AsRef<Path>) -> Result<(String, u64)> {
    let mut f = File::open(path.as_ref())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut n = 0u64;
    loop {
        let got = f.read(&mut buf)?;
        if got == 0 {
            break;
        }
        hasher.update(&buf[..got]);
        n += got as u64;
    }
    Ok((encode_hex(&hasher.finalize()), n))
}

/// Report whether the file at the content-addressed path hashes to `hash`.
pub fn verify(root: impl AsRef<Path>, hash: &str) -> Result<()> {
    let p = path(&root, hash)?;
    let (got, _) = hash_file(p)?;
    let want = hash.to_ascii_lowercase();
    if got != want {
        return Err(Error::VerifyMismatch {
            hash: hash.to_string(),
            actual: got,
        });
    }
    Ok(())
}
