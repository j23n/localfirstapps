//! Folder walk and rebuildable music projection.

use std::collections::BTreeMap;

use localcore_conflict::ConflictGroup;
use localcore_vfs::{Vfs, VfsError};
use localcore_walk::walk;
use unicode_normalization::UnicodeNormalization;

use crate::display::{StatusRow, StatusSeverity, TextRow};
use crate::model::{classify_name, FileClass, MetadataUpdate, Playlist, Track};
use crate::playlist::{hydrate_entries, parse_playlist};
use crate::projection::{LibraryContentState, LibraryProjection, SortOption};

/// Temp prefix already excluded by `apps/music/.stignore`.
pub const TEMP_PREFIX: &str = ".music-tmp-";

/// Failures returned by every typed action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// Local file operation failed.
    Io(String),
    /// Playlist bytes could not be interpreted without loss.
    InvalidPlaylist {
        /// Affected path.
        path: String,
        /// Display-ready reason.
        message: String,
    },
    /// An opaque id no longer names an item.
    NotFound,
    /// A typed command is inconsistent.
    InvalidCommand(String),
    /// Playlist bytes changed after the editor read its token.
    StalePlaylist {
        /// Display-ready recovery text.
        message: String,
    },
    /// A visible window was requested from an older projection.
    StaleGeneration {
        /// Generation supplied by the shell.
        requested: u64,
        /// Current generation.
        current: u64,
    },
    /// Ordered edits overlap and require a whole-document choice.
    NeedsChoice,
    /// The conflict is not a writable playlist conflict.
    UnsupportedConflict,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) | Self::InvalidCommand(message) => formatter.write_str(message),
            Self::InvalidPlaylist { path, message } => {
                write!(formatter, "Cannot read playlist {path}: {message}")
            }
            Self::NotFound => formatter.write_str("The item is no longer available"),
            Self::StalePlaylist { message } => formatter.write_str(message),
            Self::StaleGeneration { .. } => {
                formatter.write_str("The library changed. Reload the visible rows")
            }
            Self::NeedsChoice => formatter.write_str("Choose which playlist order to keep"),
            Self::UnsupportedConflict => {
                formatter.write_str("This conflict cannot be resolved as a playlist")
            }
        }
    }
}

impl std::error::Error for StoreError {}

impl From<VfsError> for StoreError {
    fn from(error: VfsError) -> Self {
        Self::Io(error.to_string())
    }
}

/// Minimum input a platform metadata reader needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataRequest {
    /// Opaque track id returned with the result.
    pub id: String,
    /// Local audio path under the active host folder grant.
    pub path: String,
}

/// Minimum input a platform playback service needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaSource {
    /// Opaque track id.
    pub id: String,
    /// Local audio path under the active host folder grant.
    pub path: String,
}

/// Headless store. Files are authority; every collection here is rebuildable.
#[derive(Debug, Clone)]
pub struct Store {
    /// Folder the walk started at.
    pub root: String,
    tracks: Vec<Track>,
    playlists: Vec<Playlist>,
    conflict_groups: Vec<ConflictGroup>,
    scan_issues: Vec<StatusRow>,
    projection: LibraryProjection,
}

impl Store {
    /// Walk audio and playlist files, excluding every Syncthing copy from
    /// content and identity.
    pub fn open(vfs: &dyn Vfs, root: &str) -> Result<Self, StoreError> {
        let outcome = walk(vfs, root, |name| classify_name(name).is_some());
        if outcome.directories.is_empty() {
            if vfs.try_exists(root)? {
                return Ok(Self::empty(root));
            }
            return Err(StoreError::Io(format!("Folder is not available: {root}")));
        }
        if outcome
            .failed_directory_paths
            .iter()
            .any(|path| canon(path) == canon(root))
        {
            return Err(StoreError::Io(format!("Folder cannot be read: {root}")));
        }

        let mut tracks = Vec::new();
        let mut playlists = Vec::new();
        let mut scan_issues = outcome
            .failed_directory_paths
            .iter()
            .map(|path| StatusRow {
                id: path.clone(),
                message: format!(
                    "Skipped unreadable folder: {}",
                    crate::path::relative_to(root, path)
                ),
                severity: StatusSeverity::Warning,
            })
            .collect::<Vec<_>>();

        for file in outcome.files {
            let path = crate::path::standardize(&file.path);
            match classify_name(&file.name) {
                Some(FileClass::Audio) => tracks.push(Track::from_path(path)),
                Some(FileClass::Playlist(_)) => match vfs.read(&path) {
                    Ok(bytes) => match parse_playlist(&path, &bytes) {
                        Ok(playlist) => playlists.push(playlist),
                        Err(error) => scan_issues.push(StatusRow {
                            id: path.clone(),
                            message: error.to_string(),
                            severity: StatusSeverity::Warning,
                        }),
                    },
                    Err(error) => scan_issues.push(StatusRow {
                        id: path.clone(),
                        message: format!("Skipped unreadable playlist: {error}"),
                        severity: StatusSeverity::Warning,
                    }),
                },
                None => {}
            }
        }

        tracks.sort_by(|left, right| left.id.cmp(&right.id));
        for playlist in &mut playlists {
            hydrate_entries(playlist, &tracks);
        }
        playlists.sort_by(|left, right| {
            canon(&left.name)
                .cmp(&canon(&right.name))
                .then_with(|| left.id.cmp(&right.id))
        });
        let conflict_groups = outcome
            .conflict_groups
            .into_iter()
            .filter(|group| classify_name(&group.canonical_name).is_some())
            .collect();
        let projection = LibraryProjection::new(&tracks);
        Ok(Self {
            root: root.to_owned(),
            tracks,
            playlists,
            conflict_groups,
            scan_issues,
            projection,
        })
    }

    fn empty(root: &str) -> Self {
        Self {
            root: root.to_owned(),
            tracks: Vec::new(),
            playlists: Vec::new(),
            conflict_groups: Vec::new(),
            scan_issues: Vec::new(),
            projection: LibraryProjection::new(&[]),
        }
    }

    /// Reload while retaining metadata supplied by the host for unchanged ids
    /// and retaining the current query/sort intent.
    pub fn reload(&mut self, vfs: &dyn Vfs) -> Result<(), StoreError> {
        let metadata: BTreeMap<String, MetadataUpdate> = self
            .tracks
            .iter()
            .map(|track| {
                (
                    track.id.clone(),
                    MetadataUpdate {
                        id: track.id.clone(),
                        title: track.title.clone(),
                        artist: track.artist.clone(),
                        album: track.album.clone(),
                        duration_ms: track.duration_ms,
                        has_artwork: track.has_artwork,
                        has_lyrics: track.has_lyrics,
                    },
                )
            })
            .collect();
        let query = self.projection.query.clone();
        let sort = self.projection.sort;
        let mut next = Self::open(vfs, &self.root)?;
        for update in metadata.into_values() {
            if next.track(&update.id).is_some() {
                next.apply_metadata(update)?;
            }
        }
        next.set_library_view(query, sort);
        *self = next;
        Ok(())
    }

    /// Projected tracks, in stable-id order. Shells use windowed rows instead.
    #[must_use]
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// Projected playlists in core-owned display order.
    #[must_use]
    pub fn playlists(&self) -> &[Playlist] {
        &self.playlists
    }

    /// Unresolved music conflict groups.
    #[must_use]
    pub fn conflict_groups(&self) -> &[ConflictGroup] {
        &self.conflict_groups
    }

    /// Display-ready non-fatal scan issues.
    #[must_use]
    pub fn scan_issue_rows(&self) -> Vec<StatusRow> {
        self.scan_issues.clone()
    }

    /// Track by opaque id.
    #[must_use]
    pub fn track(&self, id: &str) -> Option<&Track> {
        self.tracks.iter().find(|track| track.id == id)
    }

    /// Playlist by opaque id.
    #[must_use]
    pub fn playlist(&self, id: &str) -> Option<&Playlist> {
        self.playlists.iter().find(|playlist| playlist.id == id)
    }

    pub(crate) fn playlist_mut(&mut self, id: &str) -> Option<&mut Playlist> {
        self.playlists.iter_mut().find(|playlist| playlist.id == id)
    }

    pub(crate) fn replace_playlist(&mut self, mut playlist: Playlist) {
        hydrate_entries(&mut playlist, &self.tracks);
        if let Some(current) = self.playlist_mut(&playlist.id) {
            *current = playlist;
        } else {
            self.playlists.push(playlist);
        }
        self.sort_playlists();
    }

    pub(crate) fn remove_playlist(&mut self, id: &str) {
        self.playlists.retain(|playlist| playlist.id != id);
    }

    pub(crate) fn sort_playlists(&mut self) {
        self.playlists.sort_by(|left, right| {
            canon(&left.name)
                .cmp(&canon(&right.name))
                .then_with(|| left.id.cmp(&right.id))
        });
    }

    /// Apply display metadata returned by the host media reader.
    pub fn apply_metadata(&mut self, update: MetadataUpdate) -> Result<u64, StoreError> {
        let track = self
            .tracks
            .iter_mut()
            .find(|track| track.id == update.id)
            .ok_or(StoreError::NotFound)?;
        if !update.title.trim().is_empty() {
            track.title = update.title.trim().to_owned();
        }
        track.artist = unknown_if_empty(&update.artist, "Unknown Artist");
        track.album = unknown_if_empty(&update.album, "Unknown Album");
        track.duration_ms = update.duration_ms;
        track.has_artwork = update.has_artwork;
        track.has_lyrics = update.has_lyrics;
        self.projection.rebuild(&self.tracks);
        Ok(self.projection.generation)
    }

    /// Windowed host metadata work, never a whole Track domain record.
    #[must_use]
    pub fn metadata_requests(&self, offset: usize, limit: usize) -> Vec<MetadataRequest> {
        self.tracks
            .iter()
            .skip(offset)
            .take(limit.min(500))
            .map(|track| MetadataRequest {
                id: track.id.clone(),
                path: track.path.clone(),
            })
            .collect()
    }

    /// One host playback source.
    pub fn media_source(&self, id: &str) -> Result<MediaSource, StoreError> {
        let track = self.track(id).ok_or(StoreError::NotFound)?;
        Ok(MediaSource {
            id: track.id.clone(),
            path: track.path.clone(),
        })
    }

    /// Update core-owned search, sort, and sections.
    pub fn set_library_view(&mut self, query: String, sort: SortOption) -> u64 {
        self.projection.set_view(&self.tracks, query, sort);
        self.projection.generation
    }

    /// Current content state.
    #[must_use]
    pub fn library_content_state(&self) -> LibraryContentState {
        self.projection.content_state()
    }

    /// Current monotonically increasing generation.
    #[must_use]
    pub fn library_generation(&self) -> u64 {
        self.projection.generation
    }

    /// Cheap library structure: section rows only.
    #[must_use]
    pub fn library_section_rows(&self) -> Vec<TextRow> {
        self.projection.section_rows()
    }

    /// Display rows for one visible section window.
    pub fn library_track_rows(
        &self,
        section_id: &str,
        offset: usize,
        limit: usize,
        generation: u64,
    ) -> Result<Vec<crate::display::MediaItem>, StoreError> {
        self.projection
            .rows(&self.tracks, section_id, offset, limit, generation)
    }

    /// Folder-relative conflict key.
    #[must_use]
    pub(crate) fn conflict_id(&self, group: &ConflictGroup) -> String {
        crate::path::relative_to(&self.root, &group.id())
    }

    /// Conflict group by opaque folder-relative key.
    #[must_use]
    pub(crate) fn conflict_group(&self, id: &str) -> Option<&ConflictGroup> {
        self.conflict_groups
            .iter()
            .find(|group| self.conflict_id(group) == id)
    }
}

fn canon(value: &str) -> String {
    value.to_lowercase().nfc().collect()
}

fn unknown_if_empty(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.into()
    } else {
        value.to_owned()
    }
}
