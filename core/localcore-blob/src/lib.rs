//! Content-addressed SHA-256 store.
//!
//! Layout: `{root}/blobs/sha256/ab/cd/<64-hex>`
//!
//! Ported from health `internal/blobs`. The Go package stays until Phase 6.

#![forbid(unsafe_code)]

use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

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
    VerifyMismatch { hash: String, actual: String },
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

fn mkdir_private(path: &Path) -> io::Result<()> {
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    b.mode(0o700);
    b.create(path)
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

/// Whether the blob is already stored.
pub fn exists(root: impl AsRef<Path>, hash: &str) -> Result<bool> {
    let p = path(root, hash)?;
    match fs::metadata(p) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

struct Tee<W> {
    inner: W,
    hasher: Sha256,
    n: u64,
}

impl<W: Write> Write for Tee<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.n += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

struct RemoveOnDrop(Option<PathBuf>);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = fs::remove_file(p);
        }
    }
}

impl RemoveOnDrop {
    fn disarm(&mut self) {
        self.0.take();
    }
}

fn create_temp(tmp_dir: &Path) -> io::Result<(File, PathBuf)> {
    let pid = process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for n in 0..1024u32 {
        let p = tmp_dir.join(format!("put-{pid}-{nanos}-{n}"));
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        opts.mode(0o600);
        match opts.open(&p) {
            Ok(f) => return Ok((f, p)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a unique temp file in blobs/tmp",
    ))
}

/// Write `reader` into the store. `existed` is true when the digest was already present.
pub fn put(root: impl AsRef<Path>, mut reader: impl Read) -> Result<PutResult> {
    let root = root.as_ref();
    let tmp_dir = root.join("blobs").join("tmp");
    mkdir_private(&tmp_dir)?;
    let (tmp, tmp_name) = create_temp(&tmp_dir)?;
    let mut guard = RemoveOnDrop(Some(tmp_name.clone()));
    let mut tee = Tee {
        inner: tmp,
        hasher: Sha256::new(),
        n: 0,
    };
    io::copy(&mut reader, &mut tee)?;
    tee.flush()?;
    let Tee {
        inner,
        hasher,
        n: size,
    } = tee;
    inner.sync_all()?;
    drop(inner);
    let hash = encode_hex(&hasher.finalize());
    let dest = path(root, &hash)?;
    match fs::metadata(&dest) {
        Ok(_) => {
            return Ok(PutResult {
                hash,
                size,
                existed: true,
            });
        }
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
        Err(_) => {}
    }
    if let Some(parent) = dest.parent() {
        mkdir_private(parent)?;
    }
    fs::rename(&tmp_name, &dest)?;
    guard.disarm();
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
    let base = root.as_ref().join("blobs").join("sha256");
    match fs::metadata(&base) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
        Ok(_) => {}
    }
    let mut out = Vec::new();
    walk_blobs(&base, &mut out)?;
    out.sort_by(|a, b| a.sha256.cmp(&b.sha256));
    Ok(out)
}

fn walk_blobs(dir: &Path, out: &mut Vec<BlobInfo>) -> io::Result<()> {
    for ent in fs::read_dir(dir)? {
        let ent = ent?;
        let path = ent.path();
        let ft = ent.file_type()?;
        if ft.is_dir() {
            walk_blobs(&path, out)?;
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_owned(),
            None => continue,
        };
        if !valid_sha256(&name) {
            continue;
        }
        let size = ent.metadata()?.len();
        out.push(BlobInfo {
            sha256: name.to_ascii_lowercase(),
            size,
            path,
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
