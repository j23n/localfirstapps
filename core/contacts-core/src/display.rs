//! Display records both shells bind (ADR 0003 R5 / R6, ADR 0004 R4).
//!
//! These types are not UniFFI. `contacts-ffi` copies them onto the wire.
//! A GTK shell links this crate and binds them directly.

use localcore_vfs::Vfs;

use crate::card::Card;
use crate::merge::{plan_merge, MergeKind};
use crate::store::{Store, StoreError};

/// `text-row`. Ready to show; the shell does not format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRow {
    /// Opaque key the shell hands back (`X-LOCALCONTACTS-ID` or a group name).
    pub id: String,
    /// Primary label.
    pub title: String,
    /// Secondary label.
    pub subtitle: Option<String>,
    /// Trailing status.
    pub trailing: Option<String>,
}

/// `field-row`. Ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldRow {
    /// Optional row key (`fn`, `org`, `tel:0`, …).
    pub id: Option<String>,
    /// Field name, already chosen.
    pub label: String,
    /// Field value, already formatted.
    pub value: String,
    /// Whether a form may edit this row.
    pub editable: bool,
}

/// Sorted contact-list rows for `query` (core search).
#[must_use]
pub fn list_rows(store: &Store, query: &str) -> Vec<TextRow> {
    store
        .search(query)
        .into_iter()
        .map(|card| TextRow {
            id: card.local_id.clone(),
            title: card.display_name(),
            subtitle: subtitle(card),
            trailing: None,
        })
        .collect()
}

/// Detail `field-row`s for one card. Same copy on both shells.
#[must_use]
pub fn field_rows(card: &Card) -> Vec<FieldRow> {
    let mut rows = vec![FieldRow {
        id: Some("fn".into()),
        label: "Name".into(),
        value: card.display_name(),
        editable: true,
    }];
    if !card.organization.is_empty() {
        rows.push(FieldRow {
            id: Some("org".into()),
            label: "Organization".into(),
            value: card.organization.clone(),
            editable: true,
        });
    }
    for (i, phone) in card.phones.iter().enumerate() {
        rows.push(FieldRow {
            id: Some(format!("tel:{i}")),
            label: phone.label.clone(),
            value: phone.value.clone(),
            editable: true,
        });
    }
    for (i, email) in card.emails.iter().enumerate() {
        rows.push(FieldRow {
            id: Some(format!("email:{i}")),
            label: email.label.clone(),
            value: email.value.clone(),
            editable: true,
        });
    }
    if !card.note.is_empty() {
        rows.push(FieldRow {
            id: Some("note".into()),
            label: "Note".into(),
            value: card.note.clone(),
            editable: true,
        });
    }
    rows
}

/// Trailing string on a conflict row.
#[must_use]
pub fn merge_trailing(kind: MergeKind) -> &'static str {
    match kind {
        MergeKind::Auto => "auto",
        MergeKind::Choice => "needs choice",
        MergeKind::DeletedVersusModified => "keep copy",
    }
}

/// Syncthing groups as `text-row`s. Trailing is [`merge_trailing`].
pub fn conflict_rows(vfs: &dyn Vfs, store: &Store) -> Result<Vec<TextRow>, StoreError> {
    let mut rows = Vec::new();
    for group in store.conflict_groups() {
        let plan = plan_merge(vfs, &store.root, group)?;
        rows.push(TextRow {
            id: group.canonical_name.clone(),
            title: group.canonical_name.clone(),
            subtitle: Some(format!("{} copies", group.copies.len())),
            trailing: Some(merge_trailing(plan.kind).into()),
        });
    }
    Ok(rows)
}

/// Field-choice sides for one group. `id` is `field|source`.
pub fn choice_rows(
    vfs: &dyn Vfs,
    store: &Store,
    canonical: &str,
) -> Result<Vec<TextRow>, StoreError> {
    let group = store
        .conflict_groups()
        .iter()
        .find(|g| g.canonical_name == canonical)
        .cloned()
        .ok_or(StoreError::NotFound)?;
    let plan = plan_merge(vfs, &store.root, &group)?;
    let mut rows = Vec::new();
    for conflict in plan.conflicts {
        for (source, value) in conflict.sides {
            rows.push(TextRow {
                id: format!("{}|{source}", conflict.field),
                title: conflict.field.clone(),
                subtitle: Some(source),
                trailing: Some(value),
            });
        }
    }
    Ok(rows)
}

fn subtitle(card: &Card) -> Option<String> {
    if !card.organization.is_empty() {
        Some(card.organization.clone())
    } else {
        card.emails.first().map(|e| e.value.clone())
    }
}
