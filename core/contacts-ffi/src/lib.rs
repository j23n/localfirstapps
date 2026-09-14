//! R6-clean UniFFI surface for [`contacts_core`].
//!
//! Only ids, formatted strings, booleans, and ADR 0004 display records
//! cross. No `Contact` / `Card` record.

uniffi::setup_scaffolding!("ContactsCore");

use std::sync::Mutex;

use contacts_core::StdVfs;
use contacts_core::{
    choice_rows as core_choice_rows, conflict_rows as core_conflict_rows, delete_logged,
    field_rows as core_field_rows, is_conflict_name as core_is_conflict_name,
    list_rows as core_list_rows, parse_multiple, resolve_logged, save_logged, valid_device, write,
    FieldRow as CoreFieldRow, Store, StoreError, TextRow as CoreTextRow, TEMP_PREFIX,
};

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
    device: String,
    store: Mutex<Store>,
}

#[uniffi::export]
impl ContactsSession {
    /// Walk `root` and load surviving `.vcf` files.
    ///
    /// `device` is this device's log partition (ADR 0005 R4/R5). The
    /// identifier stays off the synced folder.
    #[uniffi::constructor]
    pub fn open(root: String, device: String) -> Result<Self, ContactsError> {
        if !valid_device(&device) {
            return Err(ContactsError::Io {
                message: format!("invalid device {device:?}"),
            });
        }
        let vfs = StdVfs::new(TEMP_PREFIX);
        let store = Store::open(&vfs, &root)?;
        Ok(Self {
            vfs,
            device,
            store: Mutex::new(store),
        })
    }

    /// `text-row` list, sorted by title.
    pub fn list_rows(&self) -> Result<Vec<TextRow>, ContactsError> {
        let store = self.store.lock().expect("session lock");
        Ok(core_list_rows(&store, "")
            .into_iter()
            .map(to_text)
            .collect())
    }

    /// `field-row`s for one card.
    pub fn field_rows(&self, id: String) -> Result<Vec<FieldRow>, ContactsError> {
        let store = self.store.lock().expect("session lock");
        let card = store.get(&id).ok_or(ContactsError::NotFound)?;
        Ok(core_field_rows(card).into_iter().map(to_field).collect())
    }

    /// Canonical vCard text for one card. The shell may parse it; `Card` does not cross.
    pub fn vcard_text(&self, id: String) -> Result<String, ContactsError> {
        let store = self.store.lock().expect("session lock");
        let card = store.get(&id).ok_or(ContactsError::NotFound)?;
        Ok(write(card))
    }

    /// Basename of the `.vcf` this id lives in.
    pub fn file_name(&self, id: String) -> Result<String, ContactsError> {
        let store = self.store.lock().expect("session lock");
        let card = store.get(&id).ok_or(ContactsError::NotFound)?;
        Ok(card.file_name.clone())
    }

    /// Parse `text` and write through [`Store::save`]. Returns the last id.
    ///
    /// `file_name` is the basename to update. Empty means assign (or
    /// reuse the name already indexed for this id).
    pub fn save_vcard(&self, text: String, file_name: String) -> Result<String, ContactsError> {
        let mut store = self.store.lock().expect("session lock");
        let cards = parse_multiple(text.as_bytes(), &file_name, false);
        if cards.is_empty() {
            return Err(ContactsError::Io {
                message: "not a vCard".into(),
            });
        }
        let mut last = String::new();
        for card in cards {
            last = save_logged(&self.vfs, &mut store, &self.device, card)?.local_id;
        }
        Ok(last)
    }

    /// Delete one card (and its file when it was the last sibling).
    pub fn delete(&self, id: String) -> Result<(), ContactsError> {
        let mut store = self.store.lock().expect("session lock");
        delete_logged(&self.vfs, &mut store, &self.device, &id)?;
        Ok(())
    }

    /// Re-walk the folder.
    pub fn reload(&self) -> Result<(), ContactsError> {
        let mut store = self.store.lock().expect("session lock");
        *store = Store::open(&self.vfs, &store.root)?;
        Ok(())
    }

    /// Conflict groups as `text-row`s. Trailing is `needs choice` or `auto`.
    pub fn conflict_rows(&self) -> Result<Vec<TextRow>, ContactsError> {
        let store = self.store.lock().expect("session lock");
        Ok(core_conflict_rows(&self.vfs, &store)?
            .into_iter()
            .map(to_text)
            .collect())
    }

    /// `text-row`s for one group's field choices. `id` is `field|source`.
    pub fn conflict_choice_rows(
        &self,
        canonical_name: String,
    ) -> Result<Vec<TextRow>, ContactsError> {
        let store = self.store.lock().expect("session lock");
        Ok(core_choice_rows(&self.vfs, &store, &canonical_name)?
            .into_iter()
            .map(to_text)
            .collect())
    }

    /// Apply a merge. `choice_ids` are `field|source` from [`Self::conflict_choice_rows`].
    /// Empty is enough for Auto / DeletedVersusModified. Choice without a pick fails.
    pub fn resolve_group(
        &self,
        canonical_name: String,
        choice_ids: Vec<String>,
    ) -> Result<(), ContactsError> {
        let mut store = self.store.lock().expect("session lock");
        resolve_logged(
            &self.vfs,
            &mut store,
            &self.device,
            &canonical_name,
            &choice_ids,
        )?;
        Ok(())
    }
}

fn to_text(row: CoreTextRow) -> TextRow {
    TextRow {
        id: row.id,
        title: row.title,
        subtitle: row.subtitle,
        trailing: row.trailing,
    }
}

fn to_field(row: CoreFieldRow) -> FieldRow {
    FieldRow {
        id: row.id,
        label: row.label,
        value: row.value,
        editable: row.editable,
    }
}
