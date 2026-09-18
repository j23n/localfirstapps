//! User actions both shells call. Tier-1 vCards are authoritative; a Tier-2
//! log failure never turns a committed file mutation into a reported failure.

use localcore_vfs::Vfs;

use crate::card::Card;
use crate::draft::{
    apply_edit_draft, content_token, edit_draft_from_card, ContactEditDraft, SaveContactCommand,
};
use crate::folder_log::{append_deleted, append_group_resolved, append_saved};
use crate::merge::{apply_merge, plan_merge, MergeKind};
use crate::store::{Store, StoreError};
use crate::vcard::write;

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

/// Reload an authoritative card and build its typed edit draft.
pub fn load_edit_draft(
    vfs: &dyn Vfs,
    store: &mut Store,
    id: &str,
) -> Result<ContactEditDraft, StoreError> {
    let root = store.root.clone();
    *store = Store::open(vfs, &root)?;
    let card = store.get(id).ok_or(StoreError::NotFound)?;
    Ok(edit_draft_from_card(card))
}

/// Canonical vCard text for an explicit export operation.
pub fn export_vcard_text(store: &Store, id: &str) -> Result<String, StoreError> {
    let card = store.get(id).ok_or(StoreError::NotFound)?;
    Ok(write(card))
}

/// Apply a typed edit after reloading and token-checking the authoritative Card.
///
/// Only fields present on [`ContactEditDraft`] are applied. File placement,
/// identity, unknown properties, and untouched PHOTO metadata come from disk.
/// The Tier-1 vCard write completes before the best-effort folder log append.
pub fn save_contact_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    command: SaveContactCommand,
) -> Result<ContactEditDraft, StoreError> {
    let _span = localcore_trace::span_always("contacts", "save_contact_logged");
    let root = store.root.clone();
    *store = Store::open(vfs, &root)?;
    save_draft_on_store(vfs, store, device, command.draft)
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

/// Delete several contacts after first validating every id.
pub fn bulk_delete_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    ids: &[String],
) -> Result<usize, StoreError> {
    let ids = normalized_ids(ids);
    reload_and_require_ids(vfs, store, &ids)?;
    for id in &ids {
        delete_logged(vfs, store, device, id)?;
    }
    Ok(ids.len())
}

/// Assign one tag to several contacts through the typed draft save path.
pub fn assign_tag_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    tag: &str,
    ids: &[String],
) -> Result<usize, StoreError> {
    let tag = normalized_tag(tag)?;
    let ids = normalized_ids(ids);
    reload_and_require_ids(vfs, store, &ids)?;
    let mut changed = 0;
    for id in ids {
        let mut draft = draft_from_store(store, &id)?;
        if draft.categories.contains(&tag) {
            continue;
        }
        draft.categories.push(tag.clone());
        save_draft_on_store(vfs, store, device, draft)?;
        changed += 1;
    }
    Ok(changed)
}

/// Rename a tag everywhere through the typed draft save path.
pub fn rename_tag_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    old_name: &str,
    new_name: &str,
) -> Result<usize, StoreError> {
    let old_name = normalized_tag(old_name)?;
    let new_name = normalized_tag(new_name)?;
    if old_name == new_name {
        return Ok(0);
    }
    let root = store.root.clone();
    *store = Store::open(vfs, &root)?;
    let ids: Vec<String> = store
        .cards()
        .iter()
        .filter(|card| card.categories.contains(&old_name))
        .map(|card| card.local_id.clone())
        .collect();
    let mut changed = 0;
    for id in ids {
        let mut draft = draft_from_store(store, &id)?;
        for category in &mut draft.categories {
            if category == &old_name {
                *category = new_name.clone();
            }
        }
        dedup_categories(&mut draft.categories);
        save_draft_on_store(vfs, store, device, draft)?;
        changed += 1;
    }
    Ok(changed)
}

/// Remove a tag everywhere through the typed draft save path.
pub fn remove_tag_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    tag: &str,
) -> Result<usize, StoreError> {
    let tag = normalized_tag(tag)?;
    let root = store.root.clone();
    *store = Store::open(vfs, &root)?;
    let ids: Vec<String> = store
        .cards()
        .iter()
        .filter(|card| card.categories.contains(&tag))
        .map(|card| card.local_id.clone())
        .collect();
    let mut changed = 0;
    for id in ids {
        let mut draft = draft_from_store(store, &id)?;
        draft.categories.retain(|category| category != &tag);
        save_draft_on_store(vfs, store, device, draft)?;
        changed += 1;
    }
    Ok(changed)
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

fn draft_from_store(store: &Store, id: &str) -> Result<ContactEditDraft, StoreError> {
    let card = store.get(id).ok_or(StoreError::NotFound)?;
    Ok(edit_draft_from_card(card))
}

/// Token-check and save against the current in-memory store. No re-walk.
fn save_draft_on_store(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    draft: ContactEditDraft,
) -> Result<ContactEditDraft, StoreError> {
    validate_draft(&draft)?;
    let mut card = if let Some(id) = draft.id.as_deref() {
        let current = store.get(id).cloned().ok_or(StoreError::NotFound)?;
        let actual = content_token(&current);
        let expected = draft
            .content_token
            .as_deref()
            .ok_or_else(|| {
                StoreError::InvalidCommand(
                    "An existing contact save requires a content token".into(),
                )
            })?
            .to_owned();
        if expected != actual {
            return Err(StoreError::StaleEdit { expected, actual });
        }
        current
    } else {
        if draft.content_token.is_some() {
            return Err(StoreError::InvalidCommand(
                "A new contact must not carry a content token".into(),
            ));
        }
        Card::new("")
    };
    apply_edit_draft(&mut card, &draft);
    let saved = save_logged(vfs, store, device, card)?;
    Ok(edit_draft_from_card(&saved))
}

fn validate_draft(draft: &ContactEditDraft) -> Result<(), StoreError> {
    if draft.id.as_ref().is_some_and(String::is_empty) {
        return Err(StoreError::InvalidCommand(
            "A contact id must not be empty".into(),
        ));
    }
    if let Some(birthday) = &draft.birthday {
        if !(1..=12).contains(&birthday.month) || !(1..=31).contains(&birthday.day) {
            return Err(StoreError::InvalidCommand(
                "Birthday month or day is out of range".into(),
            ));
        }
    }
    Ok(())
}

fn normalized_tag(tag: &str) -> Result<String, StoreError> {
    let tag = tag.trim();
    if tag.is_empty() {
        return Err(StoreError::InvalidCommand("A tag must not be empty".into()));
    }
    Ok(tag.to_owned())
}

fn normalized_ids(ids: &[String]) -> Vec<String> {
    let mut ids = ids.to_vec();
    ids.sort();
    ids.dedup();
    ids
}

fn reload_and_require_ids(
    vfs: &dyn Vfs,
    store: &mut Store,
    ids: &[String],
) -> Result<(), StoreError> {
    let root = store.root.clone();
    *store = Store::open(vfs, &root)?;
    if ids.iter().any(|id| store.get(id).is_none()) {
        return Err(StoreError::NotFound);
    }
    Ok(())
}

fn dedup_categories(categories: &mut Vec<String>) {
    let mut seen = std::collections::BTreeSet::new();
    categories.retain(|category| seen.insert(category.clone()));
}
