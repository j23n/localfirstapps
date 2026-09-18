//! Folder walk and rebuildable music projection.

use std::collections::BTreeMap;

use localcore_conflict::ConflictGroup;
use localcore_vfs::{Vfs, VfsError};
use localcore_walk::{walk_with_hooks_and_policy, FileSymlinkPolicy, WalkOutcome};
use unicode_normalization::UnicodeNormalization;

use crate::cache::{
    read_library_cache, write_library_cache, CachedTrack, LibrarySnapshot, LIBRARY_CACHE_VERSION,
};
use crate::display::{StatusRow, StatusSeverity, TextRow};
use crate::model::{classify_name, FileClass, MetadataUpdate, Playlist, Track};
use crate::playlist::{
    hydrate_entries, hydrate_entries_with_paths, parse_playlist, track_path_index,
    PLAYLIST_READ_CAP,
};
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
    track_by_id: BTreeMap<String, usize>,
    playlist_by_id: BTreeMap<String, usize>,
    conflict_groups: Vec<ConflictGroup>,
    scan_issues: Vec<StatusRow>,
    projection: LibraryProjection,
}

impl Store {
    /// Walk audio and playlist files, excluding every Syncthing copy from
    /// content and identity.
    pub fn open(vfs: &dyn Vfs, root: &str) -> Result<Self, StoreError> {
        Self::open_with_hooks(vfs, root, None, None)
            .map(|store| store.expect("open without cancel cannot be cancelled"))
    }

    /// [`open`], with walk progress and an optional cancel hook.
    ///
    /// `Ok(None)` means the caller cancelled. There is no partial store.
    pub fn open_with_hooks(
        vfs: &dyn Vfs,
        root: &str,
        on_progress: Option<&dyn Fn(usize)>,
        cancelled: Option<&dyn Fn() -> bool>,
    ) -> Result<Option<Self>, StoreError> {
        let _span = localcore_trace::span_always("music", "Store::open");
        let Some(outcome) = walk_with_hooks_and_policy(
            vfs,
            root,
            &|name| classify_name(name).is_some(),
            on_progress,
            cancelled,
            FileSymlinkPolicy::Skip,
        ) else {
            return Ok(None);
        };
        Ok(Some(Self::from_walk(vfs, root, outcome)?))
    }

    fn from_walk(vfs: &dyn Vfs, root: &str, outcome: WalkOutcome) -> Result<Self, StoreError> {
        localcore_trace::event(
            "music",
            format!(
                "walk files={} folders={} failed={}",
                outcome.files.len(),
                outcome.directories.len(),
                outcome.failed_directory_paths.len()
            ),
        );
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
                Some(FileClass::Audio) => {
                    tracks.push(Track::from_file(path, file.size, file.mtime));
                }
                Some(FileClass::Playlist(_)) => match vfs.read_capped(&path, PLAYLIST_READ_CAP) {
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
        let by_path = track_path_index(&tracks);
        for playlist in &mut playlists {
            hydrate_entries_with_paths(playlist, &by_path);
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
        let mut store = Self {
            root: root.to_owned(),
            tracks,
            playlists,
            track_by_id: BTreeMap::new(),
            playlist_by_id: BTreeMap::new(),
            conflict_groups,
            scan_issues,
            projection,
        };
        store.reindex_tracks();
        store.reindex_playlists();
        Ok(store)
    }

    fn empty(root: &str) -> Self {
        Self {
            root: root.to_owned(),
            tracks: Vec::new(),
            playlists: Vec::new(),
            track_by_id: BTreeMap::new(),
            playlist_by_id: BTreeMap::new(),
            conflict_groups: Vec::new(),
            scan_issues: Vec::new(),
            projection: LibraryProjection::new(&[]),
        }
    }

    fn reindex_tracks(&mut self) {
        self.track_by_id = self
            .tracks
            .iter()
            .enumerate()
            .map(|(index, track)| (track.id.clone(), index))
            .collect();
    }

    fn reindex_playlists(&mut self) {
        self.playlist_by_id = self
            .playlists
            .iter()
            .enumerate()
            .map(|(index, playlist)| (playlist.id.clone(), index))
            .collect();
    }

    /// Reload while retaining metadata supplied by the host for unchanged ids
    /// and retaining the current query/sort intent.
    pub fn reload(&mut self, vfs: &dyn Vfs) -> Result<(), StoreError> {
        self.reload_with_hooks(vfs, None, None)
            .map(|done| done.expect("reload without cancel cannot be cancelled"))
    }

    /// [`reload`], with walk progress and an optional cancel hook.
    pub fn reload_with_hooks(
        &mut self,
        vfs: &dyn Vfs,
        on_progress: Option<&dyn Fn(usize)>,
        cancelled: Option<&dyn Fn() -> bool>,
    ) -> Result<Option<()>, StoreError> {
        let _span = localcore_trace::span_always("music", "Store::reload");
        let metadata: BTreeMap<String, (u64, Option<localcore_vfs::FileTime>, MetadataUpdate)> =
            self.tracks
                .iter()
                .filter(|track| track.metadata_loaded)
                .map(|track| {
                    (
                        track.id.clone(),
                        (
                            track.source_size,
                            track.source_mtime,
                            MetadataUpdate {
                                id: track.id.clone(),
                                title: track.title.clone(),
                                artist: track.artist.clone(),
                                album: track.album.clone(),
                                duration_ms: track.duration_ms,
                                has_artwork: track.has_artwork,
                                has_lyrics: track.has_lyrics,
                            },
                        ),
                    )
                })
                .collect();
        let query = self.projection.query.clone();
        let sort = self.projection.sort;
        let mut next = match Self::open_with_hooks(vfs, &self.root, on_progress, cancelled)? {
            Some(next) => next,
            None => return Ok(None),
        };
        let unchanged = metadata
            .into_values()
            .filter_map(|(source_size, source_mtime, update)| {
                next.track(&update.id)
                    .is_some_and(|track| {
                        source_size == track.source_size
                            && source_mtime.is_some()
                            && source_mtime == track.source_mtime
                    })
                    .then_some(update)
            })
            .collect::<Vec<_>>();
        if !unchanged.is_empty() {
            next.apply_metadata_batch(unchanged)?;
        }
        next.set_library_view(query, sort);
        *self = next;
        Ok(Some(()))
    }

    /// Install cached tracks so the projection can paint before a folder walk.
    ///
    /// Playlists are not restored from the snapshot; the next walk hydrates
    /// `.m3u` files as authority.
    pub fn from_cache(snapshot: LibrarySnapshot) -> Result<Self, StoreError> {
        if snapshot.version != LIBRARY_CACHE_VERSION {
            return Err(StoreError::InvalidCommand(
                "Unsupported library cache version".into(),
            ));
        }
        if snapshot.root.trim().is_empty() {
            return Err(StoreError::InvalidCommand(
                "Library cache is missing a folder root".into(),
            ));
        }
        let root = crate::cache::canonical_library_root(&snapshot.root);
        let mut tracks = snapshot
            .tracks
            .into_iter()
            .map(|row| {
                Track::from_cached(
                    row.path,
                    row.source_size,
                    row.source_mtime.map(Into::into),
                    row.title,
                    row.artist,
                    row.album,
                    row.duration_ms,
                    row.has_artwork,
                    row.has_lyrics,
                    row.metadata_loaded,
                )
            })
            .collect::<Vec<_>>();
        tracks.sort_by(|left, right| left.id.cmp(&right.id));
        let projection = LibraryProjection::new(&tracks);
        let mut store = Self {
            root,
            tracks,
            playlists: Vec::new(),
            track_by_id: BTreeMap::new(),
            playlist_by_id: BTreeMap::new(),
            conflict_groups: Vec::new(),
            scan_issues: Vec::new(),
            projection,
        };
        store.reindex_tracks();
        store.reindex_playlists();
        Ok(store)
    }

    /// Replace this store from a snapshot, keeping the current query/sort.
    pub fn hydrate(&mut self, snapshot: LibrarySnapshot) -> Result<(), StoreError> {
        let query = self.projection.query.clone();
        let sort = self.projection.sort;
        let mut next = Self::from_cache(snapshot)?;
        next.set_library_view(query, sort);
        *self = next;
        Ok(())
    }

    /// Disposable JSON document for the next warm launch.
    #[must_use]
    pub fn to_cache(&self) -> LibrarySnapshot {
        LibrarySnapshot {
            version: LIBRARY_CACHE_VERSION,
            root: crate::cache::canonical_library_root(&self.root),
            tracks: self
                .tracks
                .iter()
                .map(|track| CachedTrack {
                    id: track.id.clone(),
                    path: track.path.clone(),
                    source_size: track.source_size,
                    source_mtime: track.source_mtime.map(Into::into),
                    title: track.title.clone(),
                    artist: track.artist.clone(),
                    album: track.album.clone(),
                    duration_ms: track.duration_ms,
                    has_artwork: track.has_artwork,
                    has_lyrics: track.has_lyrics,
                    metadata_loaded: track.metadata_loaded,
                })
                .collect(),
        }
    }

    /// Load a private cache file. Version / payload / root mismatch evicts it.
    #[must_use]
    pub fn load_cache(vfs: &dyn Vfs, path: &str, expected_root: &str) -> Option<Self> {
        let snapshot = read_library_cache(vfs, path, expected_root)?;
        Self::from_cache(snapshot).ok()
    }

    /// Write the private cache after a walk and host metadata.
    pub fn save_cache(&self, vfs: &dyn Vfs, path: &str) -> Result<(), StoreError> {
        write_library_cache(vfs, path, &self.to_cache())
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
        self.track_by_id.get(id).map(|&index| &self.tracks[index])
    }

    /// Playlist by opaque id.
    #[must_use]
    pub fn playlist(&self, id: &str) -> Option<&Playlist> {
        self.playlist_by_id
            .get(id)
            .map(|&index| &self.playlists[index])
    }

    pub(crate) fn playlist_mut(&mut self, id: &str) -> Option<&mut Playlist> {
        let index = *self.playlist_by_id.get(id)?;
        Some(&mut self.playlists[index])
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
        self.reindex_playlists();
    }

    pub(crate) fn sort_playlists(&mut self) {
        self.playlists.sort_by(|left, right| {
            canon(&left.name)
                .cmp(&canon(&right.name))
                .then_with(|| left.id.cmp(&right.id))
        });
        self.reindex_playlists();
    }

    /// Apply display metadata returned by the host media reader.
    pub fn apply_metadata(&mut self, update: MetadataUpdate) -> Result<u64, StoreError> {
        self.apply_metadata_batch(vec![update])
    }

    /// Apply one bounded host-enrichment window and rebuild the projection once.
    pub fn apply_metadata_batch(
        &mut self,
        updates: Vec<MetadataUpdate>,
    ) -> Result<u64, StoreError> {
        let _span =
            localcore_trace::span("music", "Store::apply_metadata_batch").extra("n", updates.len());
        for update in updates {
            let index = *self
                .track_by_id
                .get(&update.id)
                .ok_or(StoreError::NotFound)?;
            let track = &mut self.tracks[index];
            if !update.title.trim().is_empty() {
                track.title = update.title.trim().to_owned();
            }
            track.artist = unknown_if_empty(&update.artist, "Unknown Artist");
            track.album = unknown_if_empty(&update.album, "Unknown Album");
            track.duration_ms = update.duration_ms;
            track.has_artwork = update.has_artwork;
            track.has_lyrics = update.has_lyrics;
            track.metadata_loaded = true;
            track.refresh_display_keys();
        }
        self.projection.rebuild(&self.tracks);
        Ok(self.projection.generation)
    }

    /// Tracks still waiting on host metadata.
    #[must_use]
    pub fn pending_metadata_count(&self) -> usize {
        self.tracks
            .iter()
            .filter(|track| !track.metadata_loaded)
            .count()
    }

    /// Windowed host metadata work, never a whole Track domain record.
    #[must_use]
    pub fn metadata_requests(&self, offset: usize, limit: usize) -> Vec<MetadataRequest> {
        self.tracks
            .iter()
            .filter(|track| !track.metadata_loaded)
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

    /// Playable sources in one playlist's order.
    ///
    /// Missing, remote, and unsupported entries remain visible in detail
    /// rows but are deliberately omitted from this host-port window.
    pub fn playlist_media_sources(
        &self,
        playlist_id: &str,
    ) -> Result<Vec<MediaSource>, StoreError> {
        let playlist = self.playlist(playlist_id).ok_or(StoreError::NotFound)?;
        Ok(playlist
            .entries
            .iter()
            .filter_map(|entry| entry.track_id.as_deref())
            .filter_map(|id| self.media_source(id).ok())
            .collect())
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
