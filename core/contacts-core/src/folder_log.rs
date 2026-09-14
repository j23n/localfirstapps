//! Tier-2 folder log (ADR 0005 R4). Through [`Vfs`] only.
//!
//! Layout: `{folder}/.contacts/log/<dev>/YYYY-MM.ndjson`.
//! Types live here, not in `localcore-log::known_type`.

use localcore_log::{append_on, read_all_on, Event};
use localcore_vfs::Vfs;
use serde_json::json;

use crate::store::{join_root, StoreError};

/// Dotted application directory inside the contacts folder.
pub const STATE_DIR: &str = ".contacts";

/// User saved or updated a card.
pub const TYPE_CONTACT_SAVED: &str = "contact_saved";
/// User deleted a card.
pub const TYPE_CONTACT_DELETED: &str = "contact_deleted";
/// User resolved a Syncthing group.
pub const TYPE_GROUP_RESOLVED: &str = "group_resolved";

/// `{folder}/.contacts`.
#[must_use]
pub fn log_root(folder: &str) -> String {
    join_root(folder, STATE_DIR)
}

fn map_err(err: localcore_log::Error) -> StoreError {
    StoreError::Io(err.to_string())
}

fn write(
    vfs: &dyn Vfs,
    folder: &str,
    device: &str,
    event_type: &str,
    body: serde_json::Value,
) -> Result<(), StoreError> {
    let ev = Event::fresh(device, event_type, body);
    append_on(vfs, &log_root(folder), &ev).map_err(map_err)
}

/// Append `contact_saved{id}`.
pub fn append_saved(vfs: &dyn Vfs, folder: &str, device: &str, id: &str) -> Result<(), StoreError> {
    write(vfs, folder, device, TYPE_CONTACT_SAVED, json!({ "id": id }))
}

/// Append `contact_deleted{id}`.
pub fn append_deleted(
    vfs: &dyn Vfs,
    folder: &str,
    device: &str,
    id: &str,
) -> Result<(), StoreError> {
    write(
        vfs,
        folder,
        device,
        TYPE_CONTACT_DELETED,
        json!({ "id": id }),
    )
}

/// Append `group_resolved{canonical, kind}`.
pub fn append_group_resolved(
    vfs: &dyn Vfs,
    folder: &str,
    device: &str,
    canonical: &str,
    kind: &str,
) -> Result<(), StoreError> {
    write(
        vfs,
        folder,
        device,
        TYPE_GROUP_RESOLVED,
        json!({ "canonical": canonical, "kind": kind }),
    )
}

/// Read the folder log. Tests and projections.
pub fn read_ops(vfs: &dyn Vfs, folder: &str) -> Result<Vec<Event>, StoreError> {
    read_all_on(vfs, &log_root(folder)).map_err(map_err)
}
