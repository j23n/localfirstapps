//! User actions both shells call. Tier-1 vCards are authoritative; a Tier-2
//! log failure never turns a committed file mutation into a reported failure.

use localcore_vfs::Vfs;

use crate::card::Card;
use crate::folder_log::{append_deleted, append_group_resolved, append_saved};
use crate::merge::{apply_merge, plan_merge, MergeKind};
use crate::store::{Store, StoreError};

/// Write a card and best-effort append `contact_saved`.
pub fn save_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    card: Card,
) -> Result<Card, StoreError> {
    let saved = store.save(vfs, card)?;
    let _ = append_saved(vfs, &store.root, device, &saved.local_id);
    Ok(saved)
}

/// Delete a card and best-effort append `contact_deleted`.
pub fn delete_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    id: &str,
) -> Result<(), StoreError> {
    store.delete(vfs, id)?;
    let _ = append_deleted(vfs, &store.root, device, id);
    Ok(())
}

/// Apply a Syncthing-group merge, re-walk, then best-effort log it.
pub fn resolve_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    group_id: &str,
    choice_ids: &[String],
) -> Result<(), StoreError> {
    let group = store
        .conflict_group(group_id)
        .cloned()
        .ok_or(StoreError::NotFound)?;
    let plan = plan_merge(vfs, &store.root, &group)?;
    if plan.kind == MergeKind::Choice && choice_ids.is_empty() {
        return Err(StoreError::NeedsChoice);
    }
    let choices: Vec<(String, String)> = choice_ids
        .iter()
        .filter_map(|raw| {
            raw.split_once('|')
                .map(|(field, source)| (field.to_owned(), source.to_owned()))
        })
        .collect();
    apply_merge(vfs, &store.root, &plan, &choices)?;
    let kind = match plan.kind {
        MergeKind::Auto => "auto",
        MergeKind::Choice => "choice",
        MergeKind::DeletedVersusModified => "keep_copy",
    };
    *store = Store::open(vfs, &store.root)?;
    let _ = append_group_resolved(vfs, &store.root, device, group_id, kind);
    Ok(())
}
