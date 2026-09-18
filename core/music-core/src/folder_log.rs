//! Music operation log through `localcore-log` and `Vfs`.
//!
//! Layout: `{folder}/.music/log/<device>/YYYY-MM.ndjson`. User playlist
//! gestures are low-rate, so the append-only log needs no compaction. At an
//! intentionally generous 100 playlist edits/day and roughly 300 bytes/event,
//! growth is about 11 MB/year.

use localcore_log::{append_op, read_all_on, Event};
use localcore_vfs::Vfs;
use serde_json::json;

use crate::StoreError;

/// Synced application state directory.
pub const STATE_DIR: &str = ".music";
/// Playlist created.
pub const TYPE_PLAYLIST_CREATED: &str = "playlist_created";
/// Playlist entries changed.
pub const TYPE_PLAYLIST_CHANGED: &str = "playlist_changed";
/// Playlist deleted.
pub const TYPE_PLAYLIST_DELETED: &str = "playlist_deleted";
/// Syncthing group resolved.
pub const TYPE_GROUP_RESOLVED: &str = "group_resolved";

/// `{folder}/.music`.
#[must_use]
pub fn log_root(folder: &str) -> String {
    crate::path::join(folder, STATE_DIR)
}

fn write(
    vfs: &dyn Vfs,
    folder: &str,
    device: &str,
    event_type: &str,
    body: serde_json::Value,
) -> Result<(), StoreError> {
    append_op(vfs, &log_root(folder), device, event_type, body)
        .map(|_| ())
        .map_err(|error| StoreError::Io(error.to_string()))
}

/// Append `playlist_created`.
pub fn append_created(
    vfs: &dyn Vfs,
    folder: &str,
    device: &str,
    playlist_id: &str,
) -> Result<(), StoreError> {
    write(
        vfs,
        folder,
        device,
        TYPE_PLAYLIST_CREATED,
        json!({ "id": playlist_id }),
    )
}

/// Append `playlist_changed`.
pub fn append_changed(
    vfs: &dyn Vfs,
    folder: &str,
    device: &str,
    playlist_id: &str,
    operation: &str,
) -> Result<(), StoreError> {
    write(
        vfs,
        folder,
        device,
        TYPE_PLAYLIST_CHANGED,
        json!({ "id": playlist_id, "operation": operation }),
    )
}

/// Append `playlist_deleted`.
pub fn append_deleted(
    vfs: &dyn Vfs,
    folder: &str,
    device: &str,
    playlist_id: &str,
) -> Result<(), StoreError> {
    write(
        vfs,
        folder,
        device,
        TYPE_PLAYLIST_DELETED,
        json!({ "id": playlist_id }),
    )
}

/// Append `group_resolved`.
pub fn append_group_resolved(
    vfs: &dyn Vfs,
    folder: &str,
    device: &str,
    group_id: &str,
    kind: &str,
) -> Result<(), StoreError> {
    write(
        vfs,
        folder,
        device,
        TYPE_GROUP_RESOLVED,
        json!({ "id": group_id, "canonical": group_id, "kind": kind }),
    )
}

/// Read all music operations in deterministic log order.
pub fn read_ops(vfs: &dyn Vfs, folder: &str) -> Result<Vec<Event>, StoreError> {
    read_all_on(vfs, &log_root(folder)).map_err(|error| StoreError::Io(error.to_string()))
}
