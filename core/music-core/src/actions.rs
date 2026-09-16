//! Typed user commands. No command carries a serialized playlist or Track.

use std::collections::BTreeSet;

use localcore_vfs::Vfs;

use crate::folder_log::{append_changed, append_created, append_deleted, append_group_resolved};
use crate::merge::{apply_merge, plan_merge, ConflictDisposition};
use crate::model::{Playlist, PlaylistEntry, PlaylistFormat};
use crate::playlist::{empty_playlist, hydrate_entries, parse_playlist, write_playlist};
use crate::projection::SortOption;
use crate::{Store, StoreError};

/// Core-owned library search/sort intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetLibraryViewCommand {
    /// Search text. Canonical-equivalent spellings match.
    pub query: String,
    /// Deterministic order and section policy.
    pub sort: SortOption,
}

/// Create an empty canonical `.m3u`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePlaylistCommand {
    /// User-facing name; path separators are neutralized by the core.
    pub name: String,
}

/// Add projected tracks without replacing existing or unknown entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddTracksCommand {
    /// Opaque playlist id.
    pub playlist_id: String,
    /// Token from [`crate::playlist_rows`] / playlist detail.
    pub content_token: String,
    /// Opaque track ids, in requested order. Duplicates remain meaningful.
    pub track_ids: Vec<String>,
}

/// Remove selected entries while preserving every other known or unknown line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovePlaylistEntriesCommand {
    /// Opaque playlist id.
    pub playlist_id: String,
    /// Authoritative content token.
    pub content_token: String,
    /// Per-document entry ids.
    pub entry_ids: Vec<String>,
}

/// Move one entry before another, or to the end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovePlaylistEntryCommand {
    /// Opaque playlist id.
    pub playlist_id: String,
    /// Authoritative content token.
    pub content_token: String,
    /// Entry to move.
    pub entry_id: String,
    /// Destination entry; absent means the end.
    pub before_entry_id: Option<String>,
}

/// Delete one playlist after confirmation in the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletePlaylistCommand {
    /// Opaque playlist id.
    pub playlist_id: String,
    /// Authoritative content token.
    pub content_token: String,
}

/// Apply one explicitly reviewed Syncthing group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveConflictCommand {
    /// Folder-relative conflict group id.
    pub group_id: String,
    /// Whole-document source for `Choice`; absent for deterministic plans.
    pub selected_source: Option<String>,
}

/// Apply core-owned search and sort policy.
pub fn set_library_view(store: &mut Store, command: SetLibraryViewCommand) -> u64 {
    store.set_library_view(command.query, command.sort)
}

/// Create, atomically write, project, and best-effort log a playlist.
pub fn create_playlist_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    command: CreatePlaylistCommand,
) -> Result<String, StoreError> {
    let base = sanitize_name(&command.name)?;
    let mut suffix = 1usize;
    let path = loop {
        let name = if suffix == 1 {
            format!("{base}.m3u")
        } else {
            format!("{base} {suffix}.m3u")
        };
        let candidate = crate::path::join(&store.root, &name);
        if !vfs.try_exists(&candidate)? {
            break candidate;
        }
        suffix += 1;
    };
    let mut playlist = empty_playlist(path, PlaylistFormat::M3u);
    write_playlist(vfs, &mut playlist)?;
    let id = playlist.id.clone();
    store.replace_playlist(playlist);
    let _ = append_created(vfs, &store.root, device, &id);
    Ok(id)
}

/// Add tracks while retaining all pre-existing entries and foreign lines.
pub fn add_tracks_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    command: AddTracksCommand,
) -> Result<String, StoreError> {
    let mut playlist =
        authoritative_playlist(vfs, store, &command.playlist_id, &command.content_token)?;
    let playlist_dir = crate::path::parent(&playlist.path);
    let tracks = command
        .track_ids
        .iter()
        .map(|id| store.track(id).cloned().ok_or(StoreError::NotFound))
        .collect::<Result<Vec<_>, _>>()?;
    for track in tracks {
        playlist.entries.push(PlaylistEntry {
            id: String::new(),
            raw_path: crate::path::relative_path(&track.path, &playlist_dir),
            resolved_path: Some(track.path),
            track_id: Some(track.id),
            directives: Vec::new(),
            pls_fields: Vec::new(),
        });
    }
    save_changed(vfs, store, device, playlist, "add_tracks")
}

/// Remove only the selected entry ids.
pub fn remove_entries_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    command: RemovePlaylistEntriesCommand,
) -> Result<String, StoreError> {
    let mut playlist =
        authoritative_playlist(vfs, store, &command.playlist_id, &command.content_token)?;
    let selected: BTreeSet<&str> = command.entry_ids.iter().map(String::as_str).collect();
    if selected.is_empty()
        || selected
            .iter()
            .any(|id| !playlist.entries.iter().any(|entry| entry.id == *id))
    {
        return Err(StoreError::NotFound);
    }
    playlist
        .entries
        .retain(|entry| !selected.contains(entry.id.as_str()));
    save_changed(vfs, store, device, playlist, "remove_entries")
}

/// Move one entry without sending the whole ordered list over FFI.
pub fn move_entry_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    command: MovePlaylistEntryCommand,
) -> Result<String, StoreError> {
    let mut playlist =
        authoritative_playlist(vfs, store, &command.playlist_id, &command.content_token)?;
    if command.before_entry_id.as_deref() == Some(command.entry_id.as_str()) {
        return Ok(playlist.content_token);
    }
    let from = playlist
        .entries
        .iter()
        .position(|entry| entry.id == command.entry_id)
        .ok_or(StoreError::NotFound)?;
    let moved = playlist.entries.remove(from);
    let destination = match command.before_entry_id {
        Some(before) => playlist
            .entries
            .iter()
            .position(|entry| entry.id == before)
            .ok_or(StoreError::NotFound)?,
        None => playlist.entries.len(),
    };
    playlist.entries.insert(destination, moved);
    save_changed(vfs, store, device, playlist, "move_entry")
}

/// Delete only after a typed command carrying the current content token.
pub fn delete_playlist_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    command: DeletePlaylistCommand,
) -> Result<(), StoreError> {
    let playlist =
        authoritative_playlist(vfs, store, &command.playlist_id, &command.content_token)?;
    vfs.remove(&playlist.path)?;
    store.remove_playlist(&command.playlist_id);
    let _ = append_deleted(vfs, &store.root, device, &command.playlist_id);
    Ok(())
}

/// Resolve one M3U/M3U8 conflict after explicit user confirmation.
pub fn resolve_conflict_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    command: ResolveConflictCommand,
) -> Result<(), StoreError> {
    let group = store
        .conflict_group(&command.group_id)
        .cloned()
        .ok_or(StoreError::NotFound)?;
    let plan = plan_merge(vfs, &store.root, &group)?;
    if plan.disposition == ConflictDisposition::Choice && command.selected_source.is_none() {
        return Err(StoreError::NeedsChoice);
    }
    apply_merge(vfs, &plan, command.selected_source.as_deref())?;
    let kind = match plan.disposition {
        ConflictDisposition::Auto => "auto",
        ConflictDisposition::Choice => "choice",
        ConflictDisposition::DeletedVersusModified => "keep_modified",
        ConflictDisposition::ManualOnly => "manual",
    };
    store.reload(vfs)?;
    let _ = append_group_resolved(vfs, &store.root, device, &command.group_id, kind);
    Ok(())
}

fn authoritative_playlist(
    vfs: &dyn Vfs,
    store: &Store,
    playlist_id: &str,
    expected_token: &str,
) -> Result<Playlist, StoreError> {
    let projected = store.playlist(playlist_id).ok_or(StoreError::NotFound)?;
    if !vfs.try_exists(&projected.path)? {
        return Err(StoreError::NotFound);
    }
    let bytes = vfs.read(&projected.path)?;
    let mut authoritative = parse_playlist(&projected.path, &bytes)?;
    if authoritative.content_token != expected_token {
        return Err(StoreError::StalePlaylist {
            message: "This playlist changed on disk. Reopen it before editing".into(),
        });
    }
    hydrate_entries(&mut authoritative, store.tracks());
    Ok(authoritative)
}

fn save_changed(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    mut playlist: Playlist,
    operation: &str,
) -> Result<String, StoreError> {
    write_playlist(vfs, &mut playlist)?;
    let token = playlist.content_token.clone();
    let id = playlist.id.clone();
    store.replace_playlist(playlist);
    let _ = append_changed(vfs, &store.root, device, &id, operation);
    Ok(token)
}

fn sanitize_name(name: &str) -> Result<String, StoreError> {
    let mut output = name
        .trim()
        .chars()
        .map(|character| {
            if matches!(character, '/' | '\\' | ':') || character.is_control() {
                '-'
            } else {
                character
            }
        })
        .collect::<String>();
    while output.contains("..") {
        output = output.replace("..", "");
    }
    output = output.trim().trim_matches('-').trim().to_owned();
    if output.is_empty() {
        return Err(StoreError::InvalidCommand(
            "Playlist name must not be empty".into(),
        ));
    }
    if output.chars().count() > 120 {
        return Err(StoreError::InvalidCommand(
            "Playlist name must be 120 characters or fewer".into(),
        ));
    }
    Ok(output)
}
