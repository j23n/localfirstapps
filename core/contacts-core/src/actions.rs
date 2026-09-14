//! User actions both shells call. Log append is part of each action.

use localcore_vfs::Vfs;

use crate::card::Card;
use crate::folder_log::{append_deleted, append_group_resolved, append_saved};
use crate::merge::{apply_merge, plan_merge, MergeKind};
use crate::store::{Store, StoreError};

/// Write a card and append `contact_saved`.
pub fn save_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    card: Card,
) -> Result<Card, StoreError> {
    let saved = store.save(vfs, card)?;
    append_saved(vfs, &store.root, device, &saved.local_id)?;
    Ok(saved)
}

/// Delete a card and append `contact_deleted`.
pub fn delete_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    id: &str,
) -> Result<(), StoreError> {
    store.delete(vfs, id)?;
    append_deleted(vfs, &store.root, device, id)?;
    Ok(())
}

/// Apply a Syncthing-group merge, log it, and re-walk.
pub fn resolve_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    canonical: &str,
    choice_ids: &[String],
) -> Result<(), StoreError> {
    let group = store
        .conflict_groups()
        .iter()
        .find(|g| g.canonical_name == canonical)
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
    append_group_resolved(vfs, &store.root, device, canonical, kind)?;
    *store = Store::open(vfs, &store.root)?;
    Ok(())
}
