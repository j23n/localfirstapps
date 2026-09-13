//! Headless contacts core (ADR 0001 R9, Phase 3.1).
//!
//! Parse and write vCards, index a folder through [`localcore_vfs::Vfs`],
//! and resolve Syncthing `.vcf` groups (ADR 0005 R8–R11). No GTK. No
//! domain `Contact` record on the UniFFI wire — that surface is
//! `contacts-ffi`.
//!
//! vCards on disk are the authority. This crate does not write
//! UserDefaults, does not dual-write a log, and does not enqueue work.
//! A later consumer that enqueues MUST NFC (or [`localcore_id`]) the
//! queue primary key first.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod card;
pub mod merge;
pub mod store;
pub mod vcard;

pub use card::{Birthday, Card, Labeled, LabeledAddress, Layout, PostalAddress};
pub use localcore_conflict::{is_conflict_name, ConflictCopy, ConflictGroup};
pub use merge::{apply_merge, plan_merge, FieldConflict, MergeKind, MergePlan};
pub use store::{join_root, Store, StoreError, TEMP_PREFIX};
pub use vcard::{parse, parse_multiple, suggested_file_name, write};
