//! R6-clean UniFFI surface for [`contacts_core`].
//!
//! Only ids, formatted strings, booleans, and ADR 0004 display records
//! cross. No `Contact` / `Card` record.

uniffi::setup_scaffolding!("ContactsCore");

use std::sync::Mutex;

use contacts_core::{
    apply_merge, is_conflict_name as core_is_conflict_name, plan_merge, Card, MergeKind, Store,
    StoreError, TEMP_PREFIX,
};
use localcore_vfs::StdVfs;

/// `text-row` (ADR 0004 R4).
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct TextRow {
    /// Opaque key the shell hands back.
    pub id: String,
    /// Primary label.
    pub title: String,
    /// Secondary label.
    pub subtitle: Option<String>,
    /// Trailing status.
    pub trailing: Option<String>,
}

/// `field-row` (ADR 0004 R4).
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct FieldRow {
    /// Optional row key.
    pub id: Option<String>,
    /// Field name, already chosen.
    pub label: String,
    /// Field value, already formatted.
    pub value: String,
    /// Whether the shell may edit this row.
    pub editable: bool,
}

/// Typed failures (ADR 0003 R8).
#[derive(uniffi::Error, Debug, Clone, PartialEq, Eq)]
pub enum ContactsError {
    /// Folder cannot be read or written.
    Io {
        /// Log text, not a match key.
        message: String,
    },
    /// No card or group with that id.
    NotFound,
    /// Merge needs a field choice; do not apply silently.
    NeedsChoice,
}

impl std::fmt::Display for ContactsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { message } => write!(f, "{message}"),
            Self::NotFound => write!(f, "not found"),
            Self::NeedsChoice => write!(f, "needs choice"),
        }
    }
}

impl std::error::Error for ContactsError {}

impl From<StoreError> for ContactsError {
    fn from(err: StoreError) -> Self {
        match err {
            StoreError::NotFound => Self::NotFound,
            StoreError::NeedsChoice | StoreError::IncompleteChoices => Self::NeedsChoice,
            StoreError::Io(message) => Self::Io { message },
        }
    }
}

/// Syncthing conflict-copy predicate (ADR 0005 R7).
#[uniffi::export]
pub fn is_conflict_name(name: String) -> bool {
    core_is_conflict_name(&name)
}

/// Open a contacts folder on the real filesystem.
#[derive(uniffi::Object)]
pub struct ContactsSession {
    vfs: StdVfs,
    store: Mutex<Store>,
}

#[uniffi::export]
impl ContactsSession {
    /// Walk `root` and load surviving `.vcf` files.
    #[uniffi::constructor]
    pub fn open(root: String) -> Result<Self, ContactsError> {
        let vfs = StdVfs::new(TEMP_PREFIX);
        let store = Store::open(&vfs, &root)?;
        Ok(Self {
            vfs,
            store: Mutex::new(store),
        })
    }

    /// `text-row` list, sorted by title.
    pub fn list_rows(&self) -> Result<Vec<TextRow>, ContactsError> {
        let store = self.store.lock().expect("session lock");
        Ok(store
            .search("")
            .into_iter()
            .map(|c| TextRow {
                id: c.local_id.clone(),
                title: c.display_name(),
                subtitle: subtitle(c),
                trailing: None,
            })
            .collect())
    }

    /// `field-row`s for one card.
    pub fn field_rows(&self, id: String) -> Result<Vec<FieldRow>, ContactsError> {
        let store = self.store.lock().expect("session lock");
        let card = store.get(&id).ok_or(ContactsError::NotFound)?;
        Ok(field_rows(card))
    }

    /// Conflict groups as `text-row`s. Trailing is `needs choice` or `auto`.
    pub fn conflict_rows(&self) -> Result<Vec<TextRow>, ContactsError> {
        let store = self.store.lock().expect("session lock");
        let mut rows = Vec::new();
        for group in store.conflict_groups() {
            let plan = plan_merge(&self.vfs, &store.root, group)?;
            let trailing = match plan.kind {
                MergeKind::Auto => "auto",
                MergeKind::Choice => "needs choice",
                MergeKind::DeletedVersusModified => "keep copy",
            };
            rows.push(TextRow {
                id: group.canonical_name.clone(),
                title: group.canonical_name.clone(),
                subtitle: Some(format!("{} copies", group.copies.len())),
                trailing: Some(trailing.into()),
            });
        }
        Ok(rows)
    }

    /// Apply an automatic (or R11) merge. Fails on [`ContactsError::NeedsChoice`].
    pub fn resolve_group(&self, canonical_name: String) -> Result<(), ContactsError> {
        let mut store = self.store.lock().expect("session lock");
        let group = store
            .conflict_groups()
            .iter()
            .find(|g| g.canonical_name == canonical_name)
            .cloned()
            .ok_or(ContactsError::NotFound)?;
        let plan = plan_merge(&self.vfs, &store.root, &group)?;
        if plan.kind == MergeKind::Choice {
            return Err(ContactsError::NeedsChoice);
        }
        let card = apply_merge(&self.vfs, &store.root, &plan, &[])?;
        *store = Store::open(&self.vfs, &store.root)?;
        let _ = card;
        Ok(())
    }
}

fn subtitle(card: &Card) -> Option<String> {
    if !card.organization.is_empty() {
        Some(card.organization.clone())
    } else {
        card.emails.first().map(|e| e.value.clone())
    }
}

fn field_rows(card: &Card) -> Vec<FieldRow> {
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
