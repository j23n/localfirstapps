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

/// A Syncthing conflict row with typed control-flow state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictRow {
    /// Opaque folder-relative group key.
    pub id: String,
    /// Surviving file path, relative to the contacts folder.
    pub title: String,
    /// Human-readable number of conflict copies.
    pub subtitle: String,
    /// Human-readable disposition.
    pub trailing: String,
    /// Semantic merge disposition. Shells branch on this, never on copy.
    pub disposition: MergeKind,
}

/// Sorted contact-list rows for `query` (core search).
#[must_use]
pub fn list_rows(store: &Store, query: &str) -> Vec<TextRow> {
    list_rows_filtered(store, query, None)
}

/// Sorted contact-list rows for `query`, optionally restricted to one tag.
#[must_use]
pub fn list_rows_filtered(store: &Store, query: &str, tag: Option<&str>) -> Vec<TextRow> {
    store
        .search(query)
        .into_iter()
        .filter(|card| tag.is_none_or(|tag| card.categories.iter().any(|value| value == tag)))
        .map(|card| TextRow {
            id: card.local_id.clone(),
            title: card.display_name(),
            subtitle: subtitle(card),
            trailing: None,
        })
        .collect()
}

/// Sorted tag filter rows with display-ready counts.
#[must_use]
pub fn tag_rows(store: &Store) -> Vec<TextRow> {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for card in store.cards() {
        for tag in &card.categories {
            *counts.entry(tag.clone()).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .map(|(tag, count)| TextRow {
            id: tag.clone(),
            title: tag,
            subtitle: None,
            trailing: Some(if count == 1 {
                "1 contact".into()
            } else {
                format!("{count} contacts")
            }),
        })
        .collect()
}

/// Display-ready detail rows for one id without exposing [`Card`] to a shell.
pub fn detail_rows(store: &Store, id: &str) -> Result<Vec<FieldRow>, StoreError> {
    let card = store.get(id).ok_or(StoreError::NotFound)?;
    Ok(field_rows(card))
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
    if !card.job_title.is_empty() {
        rows.push(FieldRow {
            id: Some("title".into()),
            label: "Job title".into(),
            value: card.job_title.clone(),
            editable: true,
        });
    }
    if !card.nickname.is_empty() {
        rows.push(FieldRow {
            id: Some("nickname".into()),
            label: "Nickname".into(),
            value: card.nickname.clone(),
            editable: true,
        });
    }
    for (i, phone) in card.phones.iter().enumerate() {
        rows.push(FieldRow {
            id: Some(format!("tel:{i}")),
            label: display_label(&phone.label),
            value: phone.value.clone(),
            editable: true,
        });
    }
    for (i, email) in card.emails.iter().enumerate() {
        rows.push(FieldRow {
            id: Some(format!("email:{i}")),
            label: display_label(&email.label),
            value: email.value.clone(),
            editable: true,
        });
    }
    for (i, url) in card.urls.iter().enumerate() {
        rows.push(FieldRow {
            id: Some(format!("url:{i}")),
            label: display_label(&url.label),
            value: url.value.clone(),
            editable: true,
        });
    }
    for (i, address) in card.addresses.iter().enumerate() {
        rows.push(FieldRow {
            id: Some(format!("adr:{i}")),
            label: display_label(&address.label),
            value: address.value.formatted(),
            editable: true,
        });
    }
    if let Some(birthday) = &card.birthday {
        let value = match birthday.year {
            Some(year) => format!("{year:04}-{:02}-{:02}", birthday.month, birthday.day),
            None => format!("--{:02}-{:02}", birthday.month, birthday.day),
        };
        rows.push(FieldRow {
            id: Some("bday".into()),
            label: "Birthday".into(),
            value,
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
    for (i, category) in card.categories.iter().enumerate() {
        rows.push(FieldRow {
            id: Some(format!("category:{i}")),
            label: "Tag".into(),
            value: category.clone(),
            editable: true,
        });
    }
    if let Some(photo) = &card.photo {
        rows.push(FieldRow {
            id: Some("photo".into()),
            label: "Photo".into(),
            value: format!("{} bytes", photo.len()),
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

/// Syncthing groups with a typed disposition and display copy.
pub fn conflict_rows(vfs: &dyn Vfs, store: &Store) -> Result<Vec<ConflictRow>, StoreError> {
    let mut rows = Vec::new();
    for group in store.conflict_groups() {
        let plan = plan_merge(vfs, &store.root, group)?;
        let id = store.conflict_id(group);
        rows.push(ConflictRow {
            title: id.clone(),
            id,
            subtitle: copies_label(group.copies.len()),
            trailing: merge_trailing(plan.kind).into(),
            disposition: plan.kind,
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
        .conflict_group(canonical)
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

fn copies_label(count: usize) -> String {
    if count == 1 {
        "1 copy".into()
    } else {
        format!("{count} copies")
    }
}

fn subtitle(card: &Card) -> Option<String> {
    if !card.organization.is_empty() {
        Some(card.organization.clone())
    } else {
        card.emails.first().map(|e| e.value.clone())
    }
}

fn display_label(label: &str) -> String {
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => "Other".into(),
    }
}
