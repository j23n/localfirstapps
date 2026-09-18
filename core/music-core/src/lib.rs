//! Headless localmusic foundation.
//!
//! `music-core` owns audio/playlist classification, path identity, metadata
//! projection, search/sort/sections, canonical playlist writes, typed user
//! actions, folder logging, and Syncthing M3U reconciliation. It references no
//! UI toolkit or Apple/Linux media API.

#![forbid(unsafe_code)]

pub mod actions;
pub mod display;
pub mod folder_log;
pub mod merge;
pub mod model;
pub mod path;
pub mod playlist;
mod projection;
pub mod store;

pub use actions::{
    add_tracks_logged, create_playlist_logged, delete_playlist_logged, move_entry_logged,
    remove_entries_logged, resolve_conflict_logged, set_library_view, AddTracksCommand,
    CreatePlaylistCommand, DeletePlaylistCommand, MovePlaylistEntryCommand,
    RemovePlaylistEntriesCommand, ResolveConflictCommand, SetLibraryViewCommand,
};
pub use display::{
    album_art_track_id, album_rows, album_track_items, artist_art_track_id, artist_rows,
    artist_track_items, conflict_choice_rows, conflict_rows, playlist_action_rows,
    playlist_art_track_id, playlist_entry_rows, playlist_rows, search_hits, settings_info_rows,
    ActionRole, ActionRow, ConflictRow, MediaItem, SearchHit, SearchKind, StatusRow,
    StatusSeverity, TextRow,
};
pub use folder_log::{
    append_changed, append_created, append_deleted, append_group_resolved, log_root, read_ops,
    STATE_DIR, TYPE_GROUP_RESOLVED, TYPE_PLAYLIST_CHANGED, TYPE_PLAYLIST_CREATED,
    TYPE_PLAYLIST_DELETED,
};
pub use localcore_conflict::{is_conflict_name, ConflictCopy, ConflictGroup};
pub use localcore_log::valid_device;
pub use localcore_vfs::{ConfinedVfs, MemVfs, StdVfs, Vfs};
pub use merge::{apply_merge, plan_merge, ConflictDisposition, MergePlan};
pub use model::{
    classify_name, FileClass, MetadataUpdate, Playlist, PlaylistEntry, PlaylistFormat, Track,
    AUDIO_EXTENSIONS, PLAYLIST_EXTENSIONS,
};
pub use playlist::{
    canonical_bytes, content_token, empty_playlist, hydrate_entries, parse_playlist,
    refresh_entry_ids, write_playlist,
};
pub use projection::{count_label, format_duration, media_item, LibraryContentState, SortOption};
pub use store::{MediaSource, MetadataRequest, Store, StoreError, TEMP_PREFIX};
