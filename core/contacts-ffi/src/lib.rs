//! R6-clean UniFFI surface for [`contacts_core`].
//!
//! Only ids, formatted display records, and explicitly marked command DTOs
//! cross. No `Contact` / `Card` record or serialized vCard mutation payload.

uniffi::setup_scaffolding!("ContactsCore");

use std::sync::{Mutex, MutexGuard};

use contacts_core::{
    assign_tag_logged, bulk_delete_logged, choice_rows as core_choice_rows,
    conflict_preview as core_conflict_preview, conflict_rows as core_conflict_rows, delete_logged,
    detail_rows as core_detail_rows, export_vcard_text as core_export_vcard_text,
    is_conflict_name as core_is_conflict_name, list_rows as core_list_rows,
    list_rows_filtered as core_list_rows_filtered, load_edit_draft, remove_tag_logged,
    rename_tag_logged, resolve_logged, save_contact_logged, search_hits as core_search_hits,
    tag_rows as core_tag_rows, valid_device, BirthdayDraft as CoreBirthdayDraft, ConfinedVfs,
    ConflictPreview as CoreConflictPreview, ConflictRow as CoreConflictRow,
    ContactEditDraft as CoreContactEditDraft, FieldRow as CoreFieldRow,
    LabeledAddressDraft as CoreLabeledAddressDraft, LabeledValueDraft as CoreLabeledValueDraft,
    MergeKind as CoreMergeKind, SaveContactCommand as CoreSaveContactCommand,
    SearchHit as CoreSearchHit, Store, StoreError, TextRow as CoreTextRow, TEMP_PREFIX,
};
use localcore_vfs::VfsError;

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

/// R6 role: command DTO.
///
/// One editable labeled URL, phone, or email.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct LabeledValueDraft {
    /// TYPE label.
    pub label: String,
    /// Unescaped value.
    pub value: String,
}

/// R6 role: command DTO.
///
/// One editable structured postal address.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct LabeledAddressDraft {
    /// TYPE label.
    pub label: String,
    /// Street.
    pub street: String,
    /// City.
    pub city: String,
    /// Region/state.
    pub state: String,
    /// Postal code.
    pub postal_code: String,
    /// Country.
    pub country: String,
}

/// R6 role: command DTO.
///
/// Editable birthday; `year` is absent for `--MM-DD`.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct BirthdayDraft {
    /// Four-digit year, when known.
    pub year: Option<i32>,
    /// Month 1–12.
    pub month: u8,
    /// Day 1–31.
    pub day: u8,
}

/// R6 role: command DTO.
///
/// Full core-owned contact edit form. Storage-only Card fields do not cross.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ContactEditDraft {
    /// Existing id; absent for a new contact.
    pub id: Option<String>,
    /// Deterministic token of the authoritative Card used to open this draft.
    pub content_token: Option<String>,
    /// FN.
    pub full_name: String,
    /// N family.
    pub family_name: String,
    /// N given.
    pub given_name: String,
    /// N middle.
    pub middle_name: String,
    /// N prefix.
    pub name_prefix: String,
    /// N suffix.
    pub name_suffix: String,
    /// ORG.
    pub organization: String,
    /// TITLE.
    pub job_title: String,
    /// NICKNAME.
    pub nickname: String,
    /// URL rows.
    pub urls: Vec<LabeledValueDraft>,
    /// TEL rows.
    pub phones: Vec<LabeledValueDraft>,
    /// EMAIL rows.
    pub emails: Vec<LabeledValueDraft>,
    /// ADR rows.
    pub addresses: Vec<LabeledAddressDraft>,
    /// BDAY.
    pub birthday: Option<BirthdayDraft>,
    /// NOTE.
    pub note: String,
    /// CATEGORIES.
    pub categories: Vec<String>,
    /// Decoded PHOTO bytes.
    pub photo: Option<Vec<u8>>,
}

/// R6 role: command DTO.
///
/// Typed save intent carrying editable fields and their base token.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct SaveContactCommand {
    /// Draft to validate and save.
    pub draft: ContactEditDraft,
}

/// Semantic conflict disposition. Shells use this for control flow.
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeKind {
    /// All fields merge deterministically.
    Auto,
    /// At least one field requires a user choice.
    Choice,
    /// The surviving file disappeared while a copy retains the data.
    DeletedVersusModified,
}

/// One live-search match. Same slot as [`TextRow`]; `subtitle` is the
/// matched field value and `trailing` is the field kind.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// Opaque key the shell hands back.
    pub id: String,
    /// Display name.
    pub title: String,
    /// Matched field value; the shell highlights the query it already holds.
    pub subtitle: String,
    /// Field kind (`Phone`, `Email`, …).
    pub trailing: Option<String>,
    /// Symbolic icon name for the matched field kind.
    pub symbol: String,
}

/// R6 role: command DTO.
///
/// One side of a conflicting field.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ConflictSide {
    /// Copy basename or surviving path.
    pub source: String,
    /// Formatted field value on that copy.
    pub value: String,
}

/// R6 role: command DTO.
///
/// One field that differs across Syncthing copies.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ConflictFieldPreview {
    /// Stable field key (`tel:cell`, `fn`, …).
    pub field: String,
    /// Visible sides the shell offers as a choice.
    pub sides: Vec<ConflictSide>,
}

/// R6 role: command DTO.
///
/// Field-level preview for an explicit Syncthing-group review.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ConflictPreview {
    /// Group id the shell hands back to [`ContactsSession::resolve_group`].
    pub id: String,
    /// Surviving file name.
    pub title: String,
    /// Auto / choice / deleted-versus-modified.
    pub kind: MergeKind,
    /// Copy basenames that will be deleted on confirm.
    pub discarded: Vec<String>,
    /// Merged card as the user will see it if they confirm without edits.
    pub merged_fields: Vec<FieldRow>,
    /// Fields that differ. Empty when [`MergeKind::Auto`].
    pub fields: Vec<ConflictFieldPreview>,
}

/// Display-ready conflict group plus typed disposition.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ConflictRow {
    /// Opaque folder-relative key handed back to merge calls.
    pub id: String,
    /// Surviving file path relative to the contacts folder.
    pub title: String,
    /// Human-readable number of copies.
    pub subtitle: String,
    /// Human-readable disposition.
    pub trailing: String,
    /// Typed disposition for shell control flow.
    pub disposition: MergeKind,
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
    /// Draft is older than the authoritative Card.
    StaleEdit {
        /// Display-ready recovery message.
        message: String,
        /// The shell can recover by reopening the editor.
        user_actionable: bool,
    },
    /// Typed command is inconsistent or invalid.
    InvalidCommand {
        /// Display-ready validation message.
        message: String,
        /// The user can change the submitted values.
        user_actionable: bool,
    },
}

impl std::fmt::Display for ContactsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { message } => write!(f, "{message}"),
            Self::NotFound => write!(f, "not found"),
            Self::NeedsChoice => write!(f, "needs choice"),
            Self::StaleEdit { message, .. } | Self::InvalidCommand { message, .. } => {
                write!(f, "{message}")
            }
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
            stale @ StoreError::StaleEdit { .. } => Self::StaleEdit {
                message: stale.to_string(),
                user_actionable: true,
            },
            StoreError::InvalidCommand(message) => Self::InvalidCommand {
                message,
                user_actionable: true,
            },
        }
    }
}

impl From<VfsError> for ContactsError {
    fn from(err: VfsError) -> Self {
        Self::Io {
            message: err.to_string(),
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
    vfs: ConfinedVfs,
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
            return Err(ContactsError::InvalidCommand {
                message: format!("invalid device {device:?}"),
                user_actionable: true,
            });
        }
        let vfs = ConfinedVfs::new(TEMP_PREFIX, &root)?;
        let store = Store::open(&vfs, &root)?;
        Ok(Self {
            vfs,
            device,
            store: Mutex::new(store),
        })
    }

    /// `text-row` list, sorted by title.
    pub fn list_rows(&self) -> Result<Vec<TextRow>, ContactsError> {
        let store = self.lock()?;
        Ok(core_list_rows(&store, "")
            .into_iter()
            .map(to_text)
            .collect())
    }

    /// Display-ready contact rows for a search query and optional tag filter.
    pub fn filtered_list_rows(
        &self,
        query: String,
        tag: Option<String>,
    ) -> Result<Vec<TextRow>, ContactsError> {
        let store = self.lock()?;
        Ok(core_list_rows_filtered(&store, &query, tag.as_deref())
            .into_iter()
            .map(to_text)
            .collect())
    }

    /// Live-search hits with the first matching field.
    pub fn search_hits(
        &self,
        query: String,
        tag: Option<String>,
    ) -> Result<Vec<SearchHit>, ContactsError> {
        let store = self.lock()?;
        Ok(core_search_hits(&store, &query, tag.as_deref())
            .into_iter()
            .map(to_search)
            .collect())
    }

    /// Display-ready tag filters with contact counts.
    pub fn tag_rows(&self) -> Result<Vec<TextRow>, ContactsError> {
        let store = self.lock()?;
        Ok(core_tag_rows(&store).into_iter().map(to_text).collect())
    }

    /// `field-row`s for one card.
    pub fn field_rows(&self, id: String) -> Result<Vec<FieldRow>, ContactsError> {
        let store = self.lock()?;
        Ok(core_detail_rows(&store, &id)?
            .into_iter()
            .map(to_field)
            .collect())
    }

    /// Empty typed draft for creating a contact.
    pub fn new_contact_draft(&self) -> ContactEditDraft {
        from_core_draft(contacts_core::new_edit_draft())
    }

    /// Reload the authoritative Card by id and return its typed edit draft.
    pub fn contact_edit_draft(&self, id: String) -> Result<ContactEditDraft, ContactsError> {
        let mut store = self.lock()?;
        Ok(from_core_draft(load_edit_draft(
            &self.vfs, &mut store, &id,
        )?))
    }

    /// Validate and save a typed contact edit.
    pub fn save_contact(
        &self,
        command: SaveContactCommand,
    ) -> Result<ContactEditDraft, ContactsError> {
        let mut store = self.lock()?;
        let saved = save_contact_logged(
            &self.vfs,
            &mut store,
            &self.device,
            CoreSaveContactCommand {
                draft: to_core_draft(command.draft),
            },
        )?;
        Ok(from_core_draft(saved))
    }

    /// Canonical vCard text for an explicit export operation only.
    pub fn export_vcard_text(&self, id: String) -> Result<String, ContactsError> {
        let store = self.lock()?;
        Ok(core_export_vcard_text(&store, &id)?)
    }

    /// Basename of the `.vcf` this id lives in.
    pub fn file_name(&self, id: String) -> Result<String, ContactsError> {
        let store = self.lock()?;
        let card = store.get(&id).ok_or(ContactsError::NotFound)?;
        Ok(card.file_name.clone())
    }

    /// Delete one card (and its file when it was the last sibling).
    pub fn delete(&self, id: String) -> Result<(), ContactsError> {
        let mut store = self.lock()?;
        delete_logged(&self.vfs, &mut store, &self.device, &id)?;
        Ok(())
    }

    /// Delete several contacts with one logged delete action per id.
    pub fn delete_many(&self, ids: Vec<String>) -> Result<u64, ContactsError> {
        let mut store = self.lock()?;
        Ok(bulk_delete_logged(&self.vfs, &mut store, &self.device, &ids)? as u64)
    }

    /// Assign a tag to several contacts through typed draft saves.
    pub fn assign_tag(&self, tag: String, ids: Vec<String>) -> Result<u64, ContactsError> {
        let mut store = self.lock()?;
        Ok(assign_tag_logged(&self.vfs, &mut store, &self.device, &tag, &ids)? as u64)
    }

    /// Rename a tag on every contact through typed draft saves.
    pub fn rename_tag(&self, old_name: String, new_name: String) -> Result<u64, ContactsError> {
        let mut store = self.lock()?;
        Ok(rename_tag_logged(&self.vfs, &mut store, &self.device, &old_name, &new_name)? as u64)
    }

    /// Remove a tag from every contact through typed draft saves.
    pub fn remove_tag(&self, tag: String) -> Result<u64, ContactsError> {
        let mut store = self.lock()?;
        Ok(remove_tag_logged(&self.vfs, &mut store, &self.device, &tag)? as u64)
    }

    /// Re-walk the folder.
    pub fn reload(&self) -> Result<(), ContactsError> {
        let mut store = self.lock()?;
        *store = Store::open(&self.vfs, &store.root)?;
        Ok(())
    }

    /// Conflict groups with display copy and a typed merge disposition.
    pub fn conflict_rows(&self) -> Result<Vec<ConflictRow>, ContactsError> {
        let store = self.lock()?;
        Ok(core_conflict_rows(&self.vfs, &store)?
            .into_iter()
            .map(to_conflict)
            .collect())
    }

    /// Field-level preview for one Syncthing group.
    pub fn conflict_preview(
        &self,
        canonical_name: String,
    ) -> Result<ConflictPreview, ContactsError> {
        let store = self.lock()?;
        Ok(to_preview(core_conflict_preview(
            &self.vfs,
            &store,
            &canonical_name,
        )?))
    }

    /// `text-row`s for one group's field choices. `id` is `field|source`.
    pub fn conflict_choice_rows(
        &self,
        canonical_name: String,
    ) -> Result<Vec<TextRow>, ContactsError> {
        let store = self.lock()?;
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
        let mut store = self.lock()?;
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

impl ContactsSession {
    fn lock(&self) -> Result<MutexGuard<'_, Store>, ContactsError> {
        self.store.lock().map_err(|_| ContactsError::Io {
            message: "Contacts session state is unavailable".into(),
        })
    }
}

fn from_core_draft(draft: CoreContactEditDraft) -> ContactEditDraft {
    ContactEditDraft {
        id: draft.id,
        content_token: draft.content_token,
        full_name: draft.full_name,
        family_name: draft.family_name,
        given_name: draft.given_name,
        middle_name: draft.middle_name,
        name_prefix: draft.name_prefix,
        name_suffix: draft.name_suffix,
        organization: draft.organization,
        job_title: draft.job_title,
        nickname: draft.nickname,
        urls: draft.urls.into_iter().map(from_core_labeled).collect(),
        phones: draft.phones.into_iter().map(from_core_labeled).collect(),
        emails: draft.emails.into_iter().map(from_core_labeled).collect(),
        addresses: draft.addresses.into_iter().map(from_core_address).collect(),
        birthday: draft.birthday.map(|birthday| BirthdayDraft {
            year: birthday.year,
            month: birthday.month,
            day: birthday.day,
        }),
        note: draft.note,
        categories: draft.categories,
        photo: draft.photo,
    }
}

fn to_core_draft(draft: ContactEditDraft) -> CoreContactEditDraft {
    CoreContactEditDraft {
        id: draft.id,
        content_token: draft.content_token,
        full_name: draft.full_name,
        family_name: draft.family_name,
        given_name: draft.given_name,
        middle_name: draft.middle_name,
        name_prefix: draft.name_prefix,
        name_suffix: draft.name_suffix,
        organization: draft.organization,
        job_title: draft.job_title,
        nickname: draft.nickname,
        urls: draft.urls.into_iter().map(to_core_labeled).collect(),
        phones: draft.phones.into_iter().map(to_core_labeled).collect(),
        emails: draft.emails.into_iter().map(to_core_labeled).collect(),
        addresses: draft.addresses.into_iter().map(to_core_address).collect(),
        birthday: draft.birthday.map(|birthday| CoreBirthdayDraft {
            year: birthday.year,
            month: birthday.month,
            day: birthday.day,
        }),
        note: draft.note,
        categories: draft.categories,
        photo: draft.photo,
    }
}

fn from_core_labeled(row: CoreLabeledValueDraft) -> LabeledValueDraft {
    LabeledValueDraft {
        label: row.label,
        value: row.value,
    }
}

fn to_core_labeled(row: LabeledValueDraft) -> CoreLabeledValueDraft {
    CoreLabeledValueDraft {
        label: row.label,
        value: row.value,
    }
}

fn from_core_address(row: CoreLabeledAddressDraft) -> LabeledAddressDraft {
    LabeledAddressDraft {
        label: row.label,
        street: row.street,
        city: row.city,
        state: row.state,
        postal_code: row.postal_code,
        country: row.country,
    }
}

fn to_core_address(row: LabeledAddressDraft) -> CoreLabeledAddressDraft {
    CoreLabeledAddressDraft {
        label: row.label,
        street: row.street,
        city: row.city,
        state: row.state,
        postal_code: row.postal_code,
        country: row.country,
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

fn to_search(hit: CoreSearchHit) -> SearchHit {
    SearchHit {
        id: hit.id,
        title: hit.title,
        subtitle: hit.field_value,
        trailing: Some(hit.field_label),
        symbol: hit.symbol.to_string(),
    }
}

fn to_preview(preview: CoreConflictPreview) -> ConflictPreview {
    ConflictPreview {
        id: preview.id,
        title: preview.title,
        kind: match preview.kind {
            CoreMergeKind::Auto => MergeKind::Auto,
            CoreMergeKind::Choice => MergeKind::Choice,
            CoreMergeKind::DeletedVersusModified => MergeKind::DeletedVersusModified,
        },
        discarded: preview.discarded,
        merged_fields: preview.merged_fields.into_iter().map(to_field).collect(),
        fields: preview
            .fields
            .into_iter()
            .map(|field| ConflictFieldPreview {
                field: field.field,
                sides: field
                    .sides
                    .into_iter()
                    .map(|(source, value)| ConflictSide { source, value })
                    .collect(),
            })
            .collect(),
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

fn to_conflict(row: CoreConflictRow) -> ConflictRow {
    ConflictRow {
        id: row.id,
        title: row.title,
        subtitle: row.subtitle,
        trailing: row.trailing,
        disposition: match row.disposition {
            CoreMergeKind::Auto => MergeKind::Auto,
            CoreMergeKind::Choice => MergeKind::Choice,
            CoreMergeKind::DeletedVersusModified => MergeKind::DeletedVersusModified,
        },
    }
}
