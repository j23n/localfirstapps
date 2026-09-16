//! R6-green UniFFI surface for `music-core`.
//!
//! Display rows, typed commands, and minimal media host-port DTOs cross.
//! Track and Playlist domain records and serialized playlist bytes do not.

uniffi::setup_scaffolding!("MusicCore");

use std::sync::{Mutex, MutexGuard};

use music_core::{
    add_tracks_logged, conflict_choice_rows as core_conflict_choice_rows,
    conflict_rows as core_conflict_rows, create_playlist_logged, delete_playlist_logged,
    is_conflict_name as core_is_conflict_name, move_entry_logged,
    playlist_action_rows as core_playlist_action_rows,
    playlist_entry_rows as core_playlist_entry_rows, playlist_rows as core_playlist_rows,
    remove_entries_logged, resolve_conflict_logged, set_library_view, ActionRole as CoreActionRole,
    AddTracksCommand as CoreAddTracksCommand, ConflictDisposition as CoreConflictDisposition,
    CreatePlaylistCommand as CoreCreateCommand, DeletePlaylistCommand as CoreDeleteCommand,
    MetadataUpdate, MovePlaylistEntryCommand as CoreMoveCommand,
    RemovePlaylistEntriesCommand as CoreRemoveCommand,
    ResolveConflictCommand as CoreResolveCommand, SetLibraryViewCommand as CoreSetViewCommand,
    SortOption as CoreSortOption, StatusSeverity as CoreStatusSeverity, StdVfs, Store, StoreError,
    TEMP_PREFIX,
};

/// ADR 0004 `text-row`.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct TextRow {
    /// Opaque id handed back to typed commands.
    pub id: String,
    /// Primary display text.
    pub title: String,
    /// Secondary display text.
    pub subtitle: Option<String>,
    /// Trailing display text.
    pub trailing: Option<String>,
}

/// ADR 0004 `media-item`.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct MediaItem {
    /// Opaque track id.
    pub id: String,
    /// Host-resolved artwork reference or semantic symbol reference.
    pub thumbnail_ref: String,
    /// Display label.
    pub label: Option<String>,
    /// Display-ready artist/album/duration badge.
    pub badge: Option<String>,
}

/// ADR 0004 action role.
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicActionRole {
    /// Non-destructive.
    Normal,
    /// Destructive and confirmation-gated.
    Destructive,
}

/// ADR 0004 `action-row`.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ActionRow {
    /// Opaque action id.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Semantic role.
    pub role: MusicActionRole,
    /// Current enablement.
    pub enabled: bool,
}

/// ADR 0004 status severity.
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicStatusSeverity {
    /// Informational.
    Info,
    /// Recoverable warning.
    Warning,
    /// Failed operation.
    Error,
}

/// ADR 0004 `status-row`.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct StatusRow {
    /// Opaque issue id.
    pub id: String,
    /// Display-ready issue.
    pub message: String,
    /// Semantic severity.
    pub severity: MusicStatusSeverity,
}

/// Library content state after a synchronous folder open.
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryContentState {
    /// No supported local audio.
    EmptyFolder,
    /// Audio exists but no rows match.
    NoMatches,
    /// At least one visible track.
    Content,
}

/// Core-owned sort and section policy.
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOption {
    /// Title.
    Title,
    /// Artist then title.
    Artist,
    /// Album then title.
    Album,
    /// Duration then title.
    Duration,
}

/// Typed conflict disposition.
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictDisposition {
    /// Deterministic ordered union.
    Auto,
    /// Whole-document choice needed.
    Choice,
    /// Surviving file is absent; a copy retains data.
    DeletedVersusModified,
    /// Visible but outside M3U rewrite policy.
    ManualOnly,
}

/// Display-ready conflict row plus typed disposition.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ConflictRow {
    /// Folder-relative group id.
    pub id: String,
    /// Surviving filename.
    pub title: String,
    /// Human-readable copy count.
    pub subtitle: String,
    /// Human-readable disposition.
    pub trailing: String,
    /// Typed control flow.
    pub disposition: ConflictDisposition,
}

/// R6 role: host-port DTO.
///
/// Minimum input for AVFoundation/gstreamer metadata extraction.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct MetadataRequest {
    /// Opaque track id returned in [`MetadataResult`].
    pub id: String,
    /// Local path under the active host folder grant.
    pub path: String,
}

/// R6 role: host-port DTO.
///
/// Metadata returned by the platform reader; no artwork/lyrics payload bytes.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct MetadataResult {
    /// Opaque request id.
    pub id: String,
    /// Empty keeps the filename fallback.
    pub title: String,
    /// Empty becomes `Unknown Artist`.
    pub artist: String,
    /// Empty becomes `Unknown Album`.
    pub album: String,
    /// Integral duration.
    pub duration_ms: u64,
    /// Artwork can be requested from the host cache.
    pub has_artwork: bool,
    /// Lyrics can be requested from the host cache.
    pub has_lyrics: bool,
}

/// R6 role: host-port DTO.
///
/// Minimum input for AVFoundation/gstreamer playback.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct MediaSource {
    /// Opaque track id.
    pub id: String,
    /// Local audio path.
    pub path: String,
}

/// R6 role: command DTO.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct SetLibraryViewCommand {
    /// Search text.
    pub query: String,
    /// Sort/section policy.
    pub sort: SortOption,
}

/// R6 role: command DTO.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct CreatePlaylistCommand {
    /// User-facing name.
    pub name: String,
}

/// R6 role: command DTO.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct AddTracksCommand {
    /// Opaque playlist id.
    pub playlist_id: String,
    /// Authoritative content token.
    pub content_token: String,
    /// Opaque tracks in requested order.
    pub track_ids: Vec<String>,
}

/// R6 role: command DTO.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct RemovePlaylistEntriesCommand {
    /// Opaque playlist id.
    pub playlist_id: String,
    /// Authoritative content token.
    pub content_token: String,
    /// Entry ids to remove.
    pub entry_ids: Vec<String>,
}

/// R6 role: command DTO.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct MovePlaylistEntryCommand {
    /// Opaque playlist id.
    pub playlist_id: String,
    /// Authoritative content token.
    pub content_token: String,
    /// Entry to move.
    pub entry_id: String,
    /// Destination entry; absent means end.
    pub before_entry_id: Option<String>,
}

/// R6 role: command DTO.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct DeletePlaylistCommand {
    /// Opaque playlist id.
    pub playlist_id: String,
    /// Authoritative content token.
    pub content_token: String,
}

/// R6 role: command DTO.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ResolveConflictCommand {
    /// Folder-relative group id.
    pub group_id: String,
    /// Source id from conflict choice rows.
    pub selected_source: Option<String>,
}

/// Typed, display-ready failures (ADR 0003 R8).
#[derive(uniffi::Error, Debug, Clone, PartialEq, Eq)]
pub enum MusicError {
    /// Local file operation failed.
    Io {
        /// Display-ready message.
        message: String,
        /// Retry or choose another folder.
        user_actionable: bool,
    },
    /// Playlist bytes cannot be interpreted safely.
    InvalidPlaylist {
        /// Display-ready message.
        message: String,
        /// The file can be repaired externally.
        user_actionable: bool,
    },
    /// Opaque id no longer resolves.
    NotFound {
        /// Display-ready message.
        message: String,
        /// Reloading can recover.
        user_actionable: bool,
    },
    /// Typed command is invalid.
    InvalidCommand {
        /// Display-ready message.
        message: String,
        /// Submitted values can be changed.
        user_actionable: bool,
    },
    /// Playlist changed externally.
    StalePlaylist {
        /// Display-ready message.
        message: String,
        /// Reopen and retry.
        user_actionable: bool,
    },
    /// Visible window generation is stale.
    StaleGeneration {
        /// Display-ready message.
        message: String,
        /// Re-read section structure and retry.
        user_actionable: bool,
    },
    /// Conflict needs a source choice.
    NeedsChoice {
        /// Display-ready message.
        message: String,
        /// Choosing a source recovers.
        user_actionable: bool,
    },
    /// Audio/PLS conflict is outside this command surface.
    UnsupportedConflict {
        /// Display-ready message.
        message: String,
        /// User can resolve the files externally.
        user_actionable: bool,
    },
}

impl std::fmt::Display for MusicError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { message, .. }
            | Self::InvalidPlaylist { message, .. }
            | Self::NotFound { message, .. }
            | Self::InvalidCommand { message, .. }
            | Self::StalePlaylist { message, .. }
            | Self::StaleGeneration { message, .. }
            | Self::NeedsChoice { message, .. }
            | Self::UnsupportedConflict { message, .. } => formatter.write_str(message),
        }
    }
}

impl std::error::Error for MusicError {}

impl From<StoreError> for MusicError {
    fn from(error: StoreError) -> Self {
        let message = error.to_string();
        match error {
            StoreError::Io(_) => Self::Io {
                message,
                user_actionable: true,
            },
            StoreError::InvalidPlaylist { .. } => Self::InvalidPlaylist {
                message,
                user_actionable: true,
            },
            StoreError::NotFound => Self::NotFound {
                message,
                user_actionable: true,
            },
            StoreError::InvalidCommand(_) => Self::InvalidCommand {
                message,
                user_actionable: true,
            },
            StoreError::StalePlaylist { .. } => Self::StalePlaylist {
                message,
                user_actionable: true,
            },
            StoreError::StaleGeneration { .. } => Self::StaleGeneration {
                message,
                user_actionable: true,
            },
            StoreError::NeedsChoice => Self::NeedsChoice {
                message,
                user_actionable: true,
            },
            StoreError::UnsupportedConflict => Self::UnsupportedConflict {
                message,
                user_actionable: true,
            },
        }
    }
}

/// Shared conflict-copy filename predicate.
#[uniffi::export]
pub fn is_conflict_name(name: String) -> bool {
    core_is_conflict_name(&name)
}

/// Real-filesystem music session.
#[derive(uniffi::Object)]
pub struct MusicSession {
    vfs: StdVfs,
    device: String,
    store: Mutex<Store>,
}

#[uniffi::export]
impl MusicSession {
    /// Open a selected folder and build its local-byte projection.
    #[uniffi::constructor]
    pub fn open(root: String, device: String) -> Result<Self, MusicError> {
        if !music_core::valid_device(&device) {
            return Err(MusicError::InvalidCommand {
                message: "Device id contains unsupported characters".into(),
                user_actionable: true,
            });
        }
        let vfs = StdVfs::new(TEMP_PREFIX);
        let store = Store::open(&vfs, &root)?;
        Ok(Self {
            vfs,
            device,
            store: Mutex::new(store),
        })
    }

    /// Re-walk the selected folder, preserving metadata for unchanged ids.
    pub fn reload(&self) -> Result<(), MusicError> {
        self.lock()?.reload(&self.vfs)?;
        Ok(())
    }

    /// Empty-folder, no-match, or content.
    pub fn library_content_state(&self) -> Result<LibraryContentState, MusicError> {
        Ok(match self.lock()?.library_content_state() {
            music_core::LibraryContentState::EmptyFolder => LibraryContentState::EmptyFolder,
            music_core::LibraryContentState::NoMatches => LibraryContentState::NoMatches,
            music_core::LibraryContentState::Content => LibraryContentState::Content,
        })
    }

    /// Apply search/sort intent and return the new generation.
    pub fn set_library_view(&self, command: SetLibraryViewCommand) -> Result<u64, MusicError> {
        let mut store = self.lock()?;
        Ok(set_library_view(
            &mut store,
            CoreSetViewCommand {
                query: command.query,
                sort: to_core_sort(command.sort),
            },
        ))
    }

    /// Current view generation.
    pub fn library_generation(&self) -> Result<u64, MusicError> {
        Ok(self.lock()?.library_generation())
    }

    /// Cheap whole-screen structure as section `text-row`s.
    pub fn library_section_rows(&self) -> Result<Vec<TextRow>, MusicError> {
        Ok(self
            .lock()?
            .library_section_rows()
            .into_iter()
            .map(to_text)
            .collect())
    }

    /// Display-ready media items for one visible window.
    pub fn library_track_rows(
        &self,
        section_id: String,
        offset: u64,
        limit: u64,
        generation: u64,
    ) -> Result<Vec<MediaItem>, MusicError> {
        let offset = usize::try_from(offset).map_err(|_| invalid_window())?;
        let limit = usize::try_from(limit).map_err(|_| invalid_window())?;
        Ok(self
            .lock()?
            .library_track_rows(&section_id, offset, limit, generation)?
            .into_iter()
            .map(to_media)
            .collect())
    }

    /// Windowed host work for metadata extraction.
    pub fn metadata_requests(
        &self,
        offset: u64,
        limit: u64,
    ) -> Result<Vec<MetadataRequest>, MusicError> {
        let offset = usize::try_from(offset).map_err(|_| invalid_window())?;
        let limit = usize::try_from(limit).map_err(|_| invalid_window())?;
        Ok(self
            .lock()?
            .metadata_requests(offset, limit)
            .into_iter()
            .map(|request| MetadataRequest {
                id: request.id,
                path: request.path,
            })
            .collect())
    }

    /// Apply one platform metadata result and return the new generation.
    pub fn apply_metadata(&self, result: MetadataResult) -> Result<u64, MusicError> {
        Ok(self.lock()?.apply_metadata(MetadataUpdate {
            id: result.id,
            title: result.title,
            artist: result.artist,
            album: result.album,
            duration_ms: result.duration_ms,
            has_artwork: result.has_artwork,
            has_lyrics: result.has_lyrics,
        })?)
    }

    /// One minimal media playback source.
    pub fn media_source(&self, track_id: String) -> Result<MediaSource, MusicError> {
        let source = self.lock()?.media_source(&track_id)?;
        Ok(MediaSource {
            id: source.id,
            path: source.path,
        })
    }

    /// Display-ready playlist list.
    pub fn playlist_rows(&self) -> Result<Vec<TextRow>, MusicError> {
        let store = self.lock()?;
        Ok(core_playlist_rows(&store)
            .into_iter()
            .map(to_text)
            .collect())
    }

    /// Current token for typed playlist edits.
    pub fn playlist_content_token(&self, playlist_id: String) -> Result<String, MusicError> {
        Ok(self
            .lock()?
            .playlist(&playlist_id)
            .ok_or(StoreError::NotFound)?
            .content_token
            .clone())
    }

    /// Ordered playlist entry rows, including missing/unsupported values.
    pub fn playlist_entry_rows(&self, playlist_id: String) -> Result<Vec<TextRow>, MusicError> {
        let store = self.lock()?;
        Ok(core_playlist_entry_rows(&store, &playlist_id)?
            .into_iter()
            .map(to_text)
            .collect())
    }

    /// Display-ready detail actions and enablement.
    pub fn playlist_action_rows(&self, playlist_id: String) -> Result<Vec<ActionRow>, MusicError> {
        let store = self.lock()?;
        Ok(core_playlist_action_rows(&store, &playlist_id)?
            .into_iter()
            .map(to_action)
            .collect())
    }

    /// Create an empty canonical `.m3u`; returns its opaque id.
    pub fn create_playlist(&self, command: CreatePlaylistCommand) -> Result<String, MusicError> {
        let mut store = self.lock()?;
        Ok(create_playlist_logged(
            &self.vfs,
            &mut store,
            &self.device,
            CoreCreateCommand { name: command.name },
        )?)
    }

    /// Add tracks; returns the replacement content token.
    pub fn add_tracks(&self, command: AddTracksCommand) -> Result<String, MusicError> {
        let mut store = self.lock()?;
        Ok(add_tracks_logged(
            &self.vfs,
            &mut store,
            &self.device,
            CoreAddTracksCommand {
                playlist_id: command.playlist_id,
                content_token: command.content_token,
                track_ids: command.track_ids,
            },
        )?)
    }

    /// Remove selected entries; returns the replacement content token.
    pub fn remove_playlist_entries(
        &self,
        command: RemovePlaylistEntriesCommand,
    ) -> Result<String, MusicError> {
        let mut store = self.lock()?;
        Ok(remove_entries_logged(
            &self.vfs,
            &mut store,
            &self.device,
            CoreRemoveCommand {
                playlist_id: command.playlist_id,
                content_token: command.content_token,
                entry_ids: command.entry_ids,
            },
        )?)
    }

    /// Move one entry; returns the replacement content token.
    pub fn move_playlist_entry(
        &self,
        command: MovePlaylistEntryCommand,
    ) -> Result<String, MusicError> {
        let mut store = self.lock()?;
        Ok(move_entry_logged(
            &self.vfs,
            &mut store,
            &self.device,
            CoreMoveCommand {
                playlist_id: command.playlist_id,
                content_token: command.content_token,
                entry_id: command.entry_id,
                before_entry_id: command.before_entry_id,
            },
        )?)
    }

    /// Delete one confirmation-gated playlist.
    pub fn delete_playlist(&self, command: DeletePlaylistCommand) -> Result<(), MusicError> {
        let mut store = self.lock()?;
        delete_playlist_logged(
            &self.vfs,
            &mut store,
            &self.device,
            CoreDeleteCommand {
                playlist_id: command.playlist_id,
                content_token: command.content_token,
            },
        )?;
        Ok(())
    }

    /// Non-fatal scan issues as status rows.
    pub fn scan_issue_rows(&self) -> Result<Vec<StatusRow>, MusicError> {
        Ok(self
            .lock()?
            .scan_issue_rows()
            .into_iter()
            .map(to_status)
            .collect())
    }

    /// Every unresolved music conflict, including manual-only audio/PLS.
    pub fn conflict_rows(&self) -> Result<Vec<ConflictRow>, MusicError> {
        let store = self.lock()?;
        Ok(core_conflict_rows(&self.vfs, &store)?
            .into_iter()
            .map(to_conflict)
            .collect())
    }

    /// Whole-document choices for one ordered M3U conflict.
    pub fn conflict_choice_rows(&self, group_id: String) -> Result<Vec<TextRow>, MusicError> {
        let store = self.lock()?;
        Ok(core_conflict_choice_rows(&self.vfs, &store, &group_id)?
            .into_iter()
            .map(to_text)
            .collect())
    }

    /// Explicitly apply one R8-R11 conflict decision.
    pub fn resolve_conflict(&self, command: ResolveConflictCommand) -> Result<(), MusicError> {
        let mut store = self.lock()?;
        resolve_conflict_logged(
            &self.vfs,
            &mut store,
            &self.device,
            CoreResolveCommand {
                group_id: command.group_id,
                selected_source: command.selected_source,
            },
        )?;
        Ok(())
    }
}

impl MusicSession {
    fn lock(&self) -> Result<MutexGuard<'_, Store>, MusicError> {
        self.store.lock().map_err(|_| MusicError::Io {
            message: "Music session state is unavailable".into(),
            user_actionable: false,
        })
    }
}

fn invalid_window() -> MusicError {
    MusicError::InvalidCommand {
        message: "Visible row window is too large".into(),
        user_actionable: true,
    }
}

fn to_core_sort(sort: SortOption) -> CoreSortOption {
    match sort {
        SortOption::Title => CoreSortOption::Title,
        SortOption::Artist => CoreSortOption::Artist,
        SortOption::Album => CoreSortOption::Album,
        SortOption::Duration => CoreSortOption::Duration,
    }
}

fn to_text(row: music_core::TextRow) -> TextRow {
    TextRow {
        id: row.id,
        title: row.title,
        subtitle: row.subtitle,
        trailing: row.trailing,
    }
}

fn to_media(row: music_core::MediaItem) -> MediaItem {
    MediaItem {
        id: row.id,
        thumbnail_ref: row.thumbnail_ref,
        label: row.label,
        badge: row.badge,
    }
}

fn to_action(row: music_core::ActionRow) -> ActionRow {
    ActionRow {
        id: row.id,
        label: row.label,
        role: match row.role {
            CoreActionRole::Normal => MusicActionRole::Normal,
            CoreActionRole::Destructive => MusicActionRole::Destructive,
        },
        enabled: row.enabled,
    }
}

fn to_status(row: music_core::StatusRow) -> StatusRow {
    StatusRow {
        id: row.id,
        message: row.message,
        severity: match row.severity {
            CoreStatusSeverity::Info => MusicStatusSeverity::Info,
            CoreStatusSeverity::Warning => MusicStatusSeverity::Warning,
            CoreStatusSeverity::Error => MusicStatusSeverity::Error,
        },
    }
}

fn to_conflict(row: music_core::ConflictRow) -> ConflictRow {
    ConflictRow {
        id: row.id,
        title: row.title,
        subtitle: row.subtitle,
        trailing: row.trailing,
        disposition: match row.disposition {
            CoreConflictDisposition::Auto => ConflictDisposition::Auto,
            CoreConflictDisposition::Choice => ConflictDisposition::Choice,
            CoreConflictDisposition::DeletedVersusModified => {
                ConflictDisposition::DeletedVersusModified
            }
            CoreConflictDisposition::ManualOnly => ConflictDisposition::ManualOnly,
        },
    }
}
