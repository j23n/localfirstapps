//! Private, versioned, disposable library snapshot (ADR 0005 R3).
//!
//! Tags live on the cached rows the way `PhotoFile` does on Gallery. Playlists
//! stay file-authoritative and are omitted: a later walk still hydrates `.m3u`.
//! The file is never written into the selected music folder.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Snapshot document version. Bump when the JSON shape is not backward-compatible.
pub const LIBRARY_CACHE_VERSION: u32 = 1;

/// Host cache directory name under `$XDG_CACHE_HOME` / `~/.cache`.
pub const CACHE_APP_DIR: &str = "localmusic";

/// Wall-clock file fingerprint stored on a snapshot row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedFileTime {
    /// Whole seconds since the Unix epoch.
    pub secs: i64,
    /// Sub-second remainder in nanoseconds.
    pub subsec_nanos: u32,
}

impl From<localcore_vfs::FileTime> for CachedFileTime {
    fn from(value: localcore_vfs::FileTime) -> Self {
        Self {
            secs: value.secs,
            subsec_nanos: value.subsec_nanos,
        }
    }
}

impl From<CachedFileTime> for localcore_vfs::FileTime {
    fn from(value: CachedFileTime) -> Self {
        localcore_vfs::FileTime::new(value.secs, value.subsec_nanos)
    }
}

/// One cached track. Enough to paint and to reuse host metadata on reload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedTrack {
    /// Opaque path-derived id.
    pub id: String,
    /// Standardized host path.
    pub path: String,
    /// Walk fingerprint: size in bytes.
    pub source_size: u64,
    /// Walk fingerprint: modification time.
    #[serde(default)]
    pub source_mtime: Option<CachedFileTime>,
    /// Display title.
    pub title: String,
    /// Display artist.
    pub artist: String,
    /// Display album.
    pub album: String,
    /// Duration in integral milliseconds.
    pub duration_ms: u64,
    /// Embedded artwork is available through the host.
    pub has_artwork: bool,
    /// Embedded lyrics are available through the host.
    pub has_lyrics: bool,
    /// Whether a platform reader has already enriched this row.
    pub metadata_loaded: bool,
}

/// Versioned disposable JSON document. Authority for nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibrarySnapshot {
    /// [`LIBRARY_CACHE_VERSION`] when written.
    pub version: u32,
    /// Canonical selected folder. A different folder is a cache miss.
    pub root: String,
    /// Cached tracks. Playlists are omitted; `.m3u` files remain authority.
    #[serde(default)]
    pub tracks: Vec<CachedTrack>,
}

impl LibrarySnapshot {
    /// Decode JSON. Unknown extra fields are ignored; a missing `version` fails.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }

    /// Pretty JSON for a private cache file.
    pub fn to_bytes(&self) -> Result<Vec<u8>, crate::StoreError> {
        serde_json::to_vec_pretty(self).map_err(|error| crate::StoreError::Io(error.to_string()))
    }

    /// Current format and the expected selected folder.
    #[must_use]
    pub fn is_usable(&self, expected_root: &str) -> bool {
        self.version == LIBRARY_CACHE_VERSION && self.root_matches(expected_root)
    }

    /// Compare lexically standardized roots so `/music` and `/music/` agree.
    #[must_use]
    pub fn root_matches(&self, expected_root: &str) -> bool {
        canonical_library_root(&self.root) == canonical_library_root(expected_root)
    }
}

/// Parse bytes and accept only the current version. Corrupt / foreign JSON is `None`.
#[must_use]
pub fn parse_library_snapshot(bytes: &[u8]) -> Option<LibrarySnapshot> {
    LibrarySnapshot::from_bytes(bytes).filter(|snap| snap.version == LIBRARY_CACHE_VERSION)
}

/// Lexically standardized folder used as the snapshot `root` and cache key.
#[must_use]
pub fn canonical_library_root(root: &str) -> String {
    crate::path::standardize(root.trim())
}

/// File name for one selected folder. Two roots never share a file.
#[must_use]
pub fn library_cache_file_name(root: &str) -> String {
    let digest = Sha256::digest(canonical_library_root(root).as_bytes());
    format!("library-{}.json", hex_lower(&digest[..16]))
}

/// `{cache_dir}/library-<hash>.json`.
#[must_use]
pub fn library_cache_path(cache_dir: &str, root: &str) -> String {
    crate::path::join(
        &crate::path::standardize(cache_dir.trim()),
        &library_cache_file_name(root),
    )
}

/// `$XDG_CACHE_HOME/localmusic`, else `~/.cache/localmusic`, else `./localmusic`.
#[must_use]
pub fn default_cache_dir() -> String {
    let base = std::env::var("XDG_CACHE_HOME")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .filter(|value| !value.is_empty())
                .map(|home| crate::path::join(&home, ".cache"))
        })
        .unwrap_or_else(|| ".".into());
    crate::path::join(&base, CACHE_APP_DIR)
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Read a cache file. Version / payload / root mismatch deletes the file.
pub fn read_library_cache(
    vfs: &dyn localcore_vfs::Vfs,
    path: &str,
    expected_root: &str,
) -> Option<LibrarySnapshot> {
    let bytes = vfs.read(path).ok()?;
    match parse_library_snapshot(&bytes) {
        Some(snap) if snap.root_matches(expected_root) => Some(snap),
        _ => {
            evict_library_cache(vfs, path);
            None
        }
    }
}

/// Delete a disposable cache file. Ignore failure; the file is authority for nothing.
pub fn evict_library_cache(vfs: &dyn localcore_vfs::Vfs, path: &str) {
    let _ = vfs.remove(path);
}

/// Atomically replace the cache file.
pub fn write_library_cache(
    vfs: &dyn localcore_vfs::Vfs,
    path: &str,
    snapshot: &LibrarySnapshot,
) -> Result<(), crate::StoreError> {
    let parent = crate::path::parent(path);
    if !parent.is_empty() && parent != "." {
        vfs.create_dir_all(&parent)?;
    }
    vfs.write_atomic(path, &snapshot.to_bytes()?)?;
    Ok(())
}
