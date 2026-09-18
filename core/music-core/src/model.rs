//! Music domain records. These stay inside `music-core`.

use localcore_vfs::FileTime;
use unicode_normalization::UnicodeNormalization;

/// Audio files projected by the folder walk.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "m4a", "aac", "wav", "aiff", "aif", "flac", "caf", "opus",
];

/// User-facing playlist files projected by the folder walk.
pub const PLAYLIST_EXTENSIONS: &[&str] = &["m3u", "m3u8", "pls"];

/// A supported playlist serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlaylistFormat {
    /// Extended M3U with an implementation-defined legacy encoding fallback.
    M3u,
    /// UTF-8 M3U.
    M3u8,
    /// PLS version 2.
    Pls,
}

impl PlaylistFormat {
    /// Classify a lower- or mixed-case extension without its dot.
    #[must_use]
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "m3u" => Some(Self::M3u),
            "m3u8" => Some(Self::M3u8),
            "pls" => Some(Self::Pls),
            _ => None,
        }
    }

    /// Canonical extension.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::M3u => "m3u",
            Self::M3u8 => "m3u8",
            Self::Pls => "pls",
        }
    }
}

/// How a folder entry participates in the music projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileClass {
    /// A locally readable audio file.
    Audio,
    /// A user-facing playlist.
    Playlist(PlaylistFormat),
}

/// Classify a basename using the same extension table as the existing Swift
/// `MetadataLoader`.
#[must_use]
pub fn classify_name(name: &str) -> Option<FileClass> {
    let extension = name.rsplit_once('.').map_or("", |(_, ext)| ext);
    let lower = extension.to_ascii_lowercase();
    if AUDIO_EXTENSIONS.contains(&lower.as_str()) {
        Some(FileClass::Audio)
    } else {
        PlaylistFormat::from_extension(&lower).map(FileClass::Playlist)
    }
}

/// Core-owned projection of one audio path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    /// NFC path-derived identity matching Swift `Track.stableID`.
    pub id: String,
    /// Standardized host path. It crosses only in host-port DTOs.
    pub path: String,
    /// File size captured by the authoritative walk.
    pub(crate) source_size: u64,
    /// Modification time captured by the authoritative walk.
    pub(crate) source_mtime: Option<FileTime>,
    /// Whether a platform metadata reader has enriched this projection.
    pub metadata_loaded: bool,
    /// Display title.
    pub title: String,
    /// Display artist.
    pub artist: String,
    /// Display album.
    pub album: String,
    /// NFC/lowercase [`Self::title`] used by sort and search.
    pub(crate) title_key: String,
    /// NFC/lowercase [`Self::artist`] used by sort and search.
    pub(crate) artist_key: String,
    /// NFC/lowercase [`Self::album`] used by sort and search.
    pub(crate) album_key: String,
    /// Duration in integral milliseconds.
    pub duration_ms: u64,
    /// Whether the host found embedded artwork.
    pub has_artwork: bool,
    /// Whether the host found embedded lyrics.
    pub has_lyrics: bool,
}

impl Track {
    /// Build the projection available before the media metadata host responds.
    #[must_use]
    pub fn from_path(path: String) -> Self {
        Self::from_file(path, 0, None)
    }

    /// Build a projection with the walk fingerprint used across reloads.
    #[must_use]
    pub(crate) fn from_file(
        path: String,
        source_size: u64,
        source_mtime: Option<FileTime>,
    ) -> Self {
        let title = crate::path::file_stem(&path);
        let mut track = Self {
            id: localcore_id::derive(&path).to_string(),
            path,
            source_size,
            source_mtime,
            metadata_loaded: false,
            title,
            artist: "Unknown Artist".into(),
            album: "Unknown Album".into(),
            title_key: String::new(),
            artist_key: String::new(),
            album_key: String::new(),
            duration_ms: 0,
            has_artwork: false,
            has_lyrics: false,
        };
        track.refresh_display_keys();
        track
    }

    /// Restore a projection row from a disposable snapshot (ADR 0005 R3).
    pub(crate) fn from_cached(
        path: String,
        source_size: u64,
        source_mtime: Option<FileTime>,
        title: String,
        artist: String,
        album: String,
        duration_ms: u64,
        has_artwork: bool,
        has_lyrics: bool,
        metadata_loaded: bool,
    ) -> Self {
        let mut track = Self::from_file(crate::path::standardize(&path), source_size, source_mtime);
        if !title.trim().is_empty() {
            track.title = title.trim().to_owned();
        }
        track.artist = if artist.trim().is_empty() {
            "Unknown Artist".into()
        } else {
            artist.trim().to_owned()
        };
        track.album = if album.trim().is_empty() {
            "Unknown Album".into()
        } else {
            album.trim().to_owned()
        };
        track.duration_ms = duration_ms;
        track.has_artwork = has_artwork;
        track.has_lyrics = has_lyrics;
        track.metadata_loaded = metadata_loaded;
        track.refresh_display_keys();
        track
    }

    /// Refresh cached NFC/lowercase display keys after title/artist/album change.
    pub(crate) fn refresh_display_keys(&mut self) {
        self.title_key = display_key(&self.title);
        self.artist_key = display_key(&self.artist);
        self.album_key = display_key(&self.album);
    }
}

fn display_key(value: &str) -> String {
    value.trim().to_lowercase().nfc().collect()
}

/// Metadata returned by the platform media reader and applied to a track id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataUpdate {
    /// Opaque track id from a metadata request.
    pub id: String,
    /// Empty keeps the filename fallback.
    pub title: String,
    /// Empty becomes `Unknown Artist`.
    pub artist: String,
    /// Empty becomes `Unknown Album`.
    pub album: String,
    /// Duration in integral milliseconds.
    pub duration_ms: u64,
    /// Embedded artwork is available through the host.
    pub has_artwork: bool,
    /// Embedded lyrics are available through the host.
    pub has_lyrics: bool,
}

/// One line from a playlist, including unresolved and unsupported paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistEntry {
    /// Stable only for the current playlist content token.
    pub id: String,
    /// Byte-decoded path as written by the foreign file.
    pub raw_path: String,
    /// Standardized local path when the entry names supported audio.
    pub resolved_path: Option<String>,
    /// Track id when that local path exists in this projection.
    pub track_id: Option<String>,
    /// Entry-scoped M3U directives such as `#EXTINF`, retained with the path.
    pub directives: Vec<String>,
    /// Entry-scoped PLS fields as `(key-without-index, value)`.
    pub pls_fields: Vec<(String, String)>,
}

/// Parsed user-facing playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    /// Stable NFC path-derived id.
    pub id: String,
    /// Authoritative file path.
    pub path: String,
    /// Filename without its final extension.
    pub name: String,
    /// Serialization selected by the extension.
    pub format: PlaylistFormat,
    /// Ordered playlist entries. Missing files remain represented.
    pub entries: Vec<PlaylistEntry>,
    /// Foreign comments/directives retained during canonical writes.
    pub preserved_lines: Vec<String>,
    /// SHA-256 token of the authoritative bytes used to parse this value.
    pub content_token: String,
}
