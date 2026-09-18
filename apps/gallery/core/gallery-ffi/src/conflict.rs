//! XMP Syncthing conflict FFI (ADR 0005 R8–R11).
//!
//! [`gallery_meta::merge_sidecar_conflicts`] is the only merge. This module
//! walks groups, filters to `.xmp`, and commits with VFS `write_atomic` plus
//! `remove` on an explicit [`ConflictSession::resolve_group`]. Image-file
//! groups have no merge. [`MergeKind::Choice`] is never emitted.

use std::sync::Arc;

use gallery_meta::{
    apply_sidecar_conflict, merge_sidecar_conflicts, read_view, MetaError, SidecarVersion,
};
use gallery_scan::ConflictGroup;
use gallery_vfs::{FileTime, StdVfs, Vfs, VfsError};

/// Syncthing conflict-copy predicate (ADR 0005 R7).
#[uniffi::export]
pub fn is_conflict_name(name: String) -> bool {
    gallery_scan::is_conflict_name(&name)
}

/// Semantic conflict disposition. Shells use this for control flow.
///
/// [`MergeKind::Choice`] is part of the Contacts-shaped enum and is never
/// emitted for XMP under the current sidecar rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MergeKind {
    /// All sidecar fields merge deterministically.
    Auto,
    /// At least one field requires a user choice. Never produced for `.xmp`.
    Choice,
    /// The surviving sidecar disappeared while a copy retains the data.
    DeletedVersusModified,
}

/// Display-ready conflict group plus typed disposition.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ConflictRow {
    /// Folder-relative group key handed back to preview / resolve.
    pub id: String,
    /// Same as [`Self::id`].
    pub title: String,
    /// Human-readable number of copies.
    pub subtitle: String,
    /// Human-readable disposition.
    pub trailing: String,
    /// Typed disposition for shell control flow.
    pub disposition: MergeKind,
}

/// `field-row` (ADR 0004 R4). Sidecar preview fields are never editable.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct GalleryFieldRow {
    /// Optional row key (`tags`, `subject`, …).
    pub id: Option<String>,
    /// Field name, already chosen.
    pub label: String,
    /// Field value, already formatted.
    pub value: String,
    /// Always `false` on this surface.
    pub editable: bool,
}

/// R6 role: command DTO.
///
/// One side of a conflicting field. XMP never emits field choices.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ConflictSide {
    /// Copy basename or surviving path.
    pub source: String,
    /// Formatted field value on that copy.
    pub value: String,
}

/// R6 role: command DTO.
///
/// One field that differs across Syncthing copies. Always empty for XMP.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ConflictFieldPreview {
    /// Stable field key.
    pub field: String,
    /// Visible sides the shell would offer as a choice.
    pub sides: Vec<ConflictSide>,
}

/// R6 role: command DTO.
///
/// Field-level preview for an explicit Syncthing-group review.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ConflictPreview {
    /// Group id the shell hands back to [`ConflictSession::resolve_group`].
    pub id: String,
    /// Surviving file name.
    pub title: String,
    /// Auto / choice / deleted-versus-modified.
    pub kind: MergeKind,
    /// Copy basenames that will be deleted on confirm.
    pub discarded: Vec<String>,
    /// Merged sidecar as the user will see it if they confirm.
    pub merged_fields: Vec<GalleryFieldRow>,
    /// Fields that differ. Empty when [`MergeKind::Auto`] — always empty here.
    pub fields: Vec<ConflictFieldPreview>,
}

/// Typed failures (ADR 0003 R8).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum ConflictError {
    /// Folder cannot be read or written.
    Io {
        /// Log text, not a match key.
        message: String,
        /// Whether the shell should offer a retry.
        user_actionable: bool,
    },
    /// No group with that id.
    NotFound,
    /// The id names an image-file group; only `.xmp` groups merge.
    NotAnXmpGroup {
        /// Display-ready recovery message.
        message: String,
        /// The user can pick a different group.
        user_actionable: bool,
    },
    /// A sidecar could not be parsed or merged.
    Sidecar {
        /// Parser or merge message; for logs and recovery copy.
        message: String,
        /// The user can fix the sidecar bytes.
        user_actionable: bool,
    },
}

impl std::fmt::Display for ConflictError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { message, .. }
            | Self::NotAnXmpGroup { message, .. }
            | Self::Sidecar { message, .. } => formatter.write_str(message),
            Self::NotFound => formatter.write_str("not found"),
        }
    }
}

impl std::error::Error for ConflictError {}

/// Coarse-grained handle on Syncthing `.xmp` groups under one library root.
///
/// The session stores the root only. Every call re-walks and re-merges; it
/// does not cache a plan, attach to [`crate::ScannerSession`], or rebuild
/// [`crate::LibraryIndex`].
#[derive(uniffi::Object)]
pub struct ConflictSession {
    root: String,
}

#[uniffi::export]
impl ConflictSession {
    /// Store `root`. The path is not opened until a method runs.
    #[uniffi::constructor]
    pub fn open(root: String) -> Arc<Self> {
        Arc::new(Self {
            root: normalize_root(&root).to_owned(),
        })
    }

    /// `.xmp` groups only. Unparseable groups are omitted; image groups never
    /// appear. [`MergeKind::Choice`] is never returned.
    pub fn conflict_rows(&self) -> Result<Vec<ConflictRow>, ConflictError> {
        let vfs = StdVfs::new();
        let mut rows = Vec::new();
        for group in xmp_groups(&vfs, &self.root) {
            match plan_group(&vfs, &self.root, &group) {
                Ok(plan) => rows.push(ConflictRow {
                    id: plan.id.clone(),
                    title: plan.id,
                    subtitle: copies_label(group.copies.len()),
                    trailing: merge_trailing(plan.kind).into(),
                    disposition: plan.kind,
                }),
                Err(ConflictError::Sidecar { .. }) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(rows)
    }

    /// Confirmation DTO. Does not write or delete.
    pub fn conflict_preview(&self, group_id: String) -> Result<ConflictPreview, ConflictError> {
        let vfs = StdVfs::new();
        let group = find_group(&vfs, &self.root, &group_id)?;
        let plan = plan_group(&vfs, &self.root, &group)?;
        Ok(ConflictPreview {
            id: plan.id,
            title: group.canonical_name.clone(),
            kind: plan.kind,
            discarded: group.copies.iter().map(|copy| copy.name.clone()).collect(),
            merged_fields: merged_fields(&plan.merge.bytes)?,
            fields: Vec::new(),
        })
    }

    /// Re-read, merge, write the surviving sidecar, then delete copies.
    ///
    /// Does not touch the paired image or any `photo.heic` conflict group.
    pub fn resolve_group(&self, group_id: String) -> Result<(), ConflictError> {
        let vfs = StdVfs::new();
        let group = find_group(&vfs, &self.root, &group_id)?;
        let plan = plan_group(&vfs, &self.root, &group)?;
        apply_sidecar_conflict(
            &vfs,
            &plan.surviving_path,
            &plan.copy_paths,
            &plan.merge.bytes,
        )
        .map_err(from_meta)
    }
}

struct Planned {
    id: String,
    kind: MergeKind,
    surviving_path: String,
    copy_paths: Vec<String>,
    merge: gallery_meta::SidecarConflictMerge,
}

struct Loaded {
    path: String,
    modified_ns: i128,
    bytes: Vec<u8>,
}

fn xmp_groups(vfs: &dyn Vfs, root: &str) -> Vec<ConflictGroup> {
    gallery_scan::conflict_groups(vfs, root)
        .into_iter()
        .filter(is_xmp_group)
        .collect()
}

fn find_group(vfs: &dyn Vfs, root: &str, group_id: &str) -> Result<ConflictGroup, ConflictError> {
    let mut matched_image = None;
    for group in gallery_scan::conflict_groups(vfs, root) {
        if relative_id(root, &group) != group_id {
            continue;
        }
        if is_xmp_group(&group) {
            return Ok(group);
        }
        matched_image = Some(group);
    }
    if matched_image.is_some() {
        return Err(ConflictError::NotAnXmpGroup {
            message: format!(
                "{group_id} is an image conflict group; only .xmp groups can be merged"
            ),
            user_actionable: false,
        });
    }
    Err(ConflictError::NotFound)
}

fn plan_group(vfs: &dyn Vfs, root: &str, group: &ConflictGroup) -> Result<Planned, ConflictError> {
    let id = relative_id(root, group);
    let surviving_path = group.surviving_path(root);
    let surviving_exists = vfs.try_exists(&surviving_path).map_err(io_err)?;
    let mut loaded = Vec::new();
    if surviving_exists {
        loaded.push(read_sidecar(vfs, &surviving_path)?);
    }
    let mut copy_paths = Vec::new();
    for copy in &group.copies {
        copy_paths.push(copy.path.clone());
        loaded.push(read_sidecar(vfs, &copy.path)?);
    }
    if loaded.is_empty() {
        return Err(ConflictError::NotFound);
    }
    let versions: Vec<SidecarVersion<'_>> = loaded
        .iter()
        .map(|item| SidecarVersion {
            path: &item.path,
            modified_ns: item.modified_ns,
            bytes: &item.bytes,
        })
        .collect();
    let merge = merge_sidecar_conflicts(&versions).map_err(from_meta)?;
    let kind = if surviving_exists {
        MergeKind::Auto
    } else {
        MergeKind::DeletedVersusModified
    };
    Ok(Planned {
        id,
        kind,
        surviving_path,
        copy_paths,
        merge,
    })
}

fn read_sidecar(vfs: &dyn Vfs, path: &str) -> Result<Loaded, ConflictError> {
    let bytes = match vfs.read(path) {
        Ok(bytes) => bytes,
        Err(VfsError::NotFound { .. }) => {
            return Err(ConflictError::NotFound);
        }
        Err(error) => return Err(io_err(error)),
    };
    let modified_ns = match vfs.stat_entry(path) {
        Ok(entry) => entry.modified.map(file_time_ns).unwrap_or(0),
        Err(VfsError::NotFound { .. }) => 0,
        Err(error) => return Err(io_err(error)),
    };
    Ok(Loaded {
        path: path.to_owned(),
        modified_ns,
        bytes,
    })
}

fn merged_fields(bytes: &[u8]) -> Result<Vec<GalleryFieldRow>, ConflictError> {
    let view = read_view(bytes).map_err(from_meta)?;
    let people: Vec<String> = view.people_tags().into_iter().map(str::to_owned).collect();
    Ok(vec![
        field_row("tags", "Tags", &view.tags_list),
        field_row("subject", "Subject", &view.subject),
        field_row("hierarchical", "Hierarchical", &view.hierarchical_subject),
        field_row("people", "People", &people),
    ])
}

fn field_row(id: &str, label: &str, values: &[String]) -> GalleryFieldRow {
    GalleryFieldRow {
        id: Some(id.to_owned()),
        label: label.to_owned(),
        value: values.join(", "),
        editable: false,
    }
}

fn is_xmp_group(group: &ConflictGroup) -> bool {
    group
        .copies
        .first()
        .map(|copy| copy.extension == "xmp")
        .unwrap_or_else(|| {
            group
                .canonical_name
                .rsplit('.')
                .next()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("xmp"))
        })
}

fn relative_id(root: &str, group: &ConflictGroup) -> String {
    relative_to_root(root, &group.id())
}

fn relative_to_root(root: &str, path: &str) -> String {
    let root = normalize_root(root);
    path.strip_prefix(root)
        .and_then(|rest| rest.strip_prefix(['/', '\\']))
        .unwrap_or(path)
        .to_owned()
}

fn normalize_root(root: &str) -> &str {
    root.trim_end_matches(['/', '\\'])
}

fn copies_label(count: usize) -> String {
    if count == 1 {
        "1 copy".into()
    } else {
        format!("{count} copies")
    }
}

fn merge_trailing(kind: MergeKind) -> &'static str {
    match kind {
        MergeKind::Auto => "auto",
        MergeKind::Choice => "needs choice",
        MergeKind::DeletedVersusModified => "keep copy",
    }
}

fn file_time_ns(time: FileTime) -> i128 {
    i128::from(time.secs) * 1_000_000_000 + i128::from(time.subsec_nanos)
}

fn io_err(error: VfsError) -> ConflictError {
    ConflictError::Io {
        message: error.to_string(),
        user_actionable: false,
    }
}

fn from_meta(error: MetaError) -> ConflictError {
    match error {
        MetaError::Vfs(error) => io_err(error),
        other => ConflictError::Sidecar {
            message: other.to_string(),
            user_actionable: true,
        },
    }
}
