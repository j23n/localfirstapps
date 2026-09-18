//! Headless contacts core (ADR 0001 R9, Phase 3.1).
//!
//! Parse and write vCards, index a folder through [`localcore_vfs::Vfs`],
//! and resolve Syncthing `.vcf` groups (ADR 0005 R8–R11). No GTK. No
//! domain `Contact` record on the UniFFI wire — that surface is
//! `contacts-ffi`.
//!
//! Display rows, the edit draft, and logged save/delete/resolve live
//! here so both shells call the same functions (Milestone C).
//!
//! vCards on disk are the authority. The folder log is operations
//! (`contact_saved`, `contact_deleted`, `group_resolved`) under
//! `.contacts/log/<dev>/`. A consumer that enqueues MUST NFC the
//! queue key first (`localcore-queue` does).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod actions;
pub mod card;
pub mod display;
pub mod draft;
pub mod folder_log;
pub mod merge;
pub mod store;
pub mod vcard;

pub use actions::{
    assign_tag_logged, bulk_delete_logged, delete_logged, export_vcard_text, load_edit_draft,
    remove_tag_logged, rename_tag_logged, resolve_logged, save_contact_logged, save_logged,
};
pub use card::{structured_name, Birthday, Card, Labeled, LabeledAddress, Layout, PostalAddress};
pub use display::{
    choice_rows, conflict_preview, conflict_rows, detail_rows, field_rows, list_rows,
    list_rows_filtered, merge_trailing, search_hits, tag_rows, ConflictFieldPreview,
    ConflictPreview, ConflictRow, FieldMatch, FieldRow, SearchHit, TextRow,
};
pub use draft::{
    apply_edit_draft, content_token, edit_draft_from_card, new_edit_draft, BirthdayDraft,
    ContactEditDraft, LabeledAddressDraft, LabeledValueDraft, SaveContactCommand,
};
pub use folder_log::{
    append_deleted, append_group_resolved, append_saved, log_root, read_ops, STATE_DIR,
    TYPE_CONTACT_DELETED, TYPE_CONTACT_SAVED, TYPE_GROUP_RESOLVED,
};
pub use localcore_conflict::{is_conflict_name, ConflictCopy, ConflictGroup};
pub use localcore_log::valid_device;
pub use localcore_vfs::{ConfinedVfs, MemVfs, StdVfs, Vfs};
pub use merge::{apply_merge, plan_merge, FieldConflict, MergeKind, MergePlan};
pub use store::{join_root, Store, StoreError, TEMP_PREFIX};
pub use vcard::{parse, parse_multiple, suggested_file_name, write};
