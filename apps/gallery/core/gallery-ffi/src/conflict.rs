//! Syncthing conflict FFI (ADR 0005 R8–R11).
//!
//! [`gallery_meta::merge_sidecar_conflicts`] is the only merge. This module
//! walks groups, lists `.xmp` and image-file groups, and commits XMP with
//! VFS `write_atomic` plus `remove` on an explicit
//! [`ConflictSession::resolve_group`]. Image-file groups are keep-one:
//! [`ConflictSession::keep_image_copy`] uses VFS `rename` / `remove` only
//! and never rewrites image bytes. [`MergeKind::Choice`] is never emitted.

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

/// Which files a conflict group is about. Shells branch on this before
/// [`MergeKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ConflictKind {
    /// `.xmp` sidecar. [`ConflictSession::resolve_group`] merges.
    Xmp,
    /// Image (or other non-`.xmp`) file. [`ConflictSession::keep_image_copy`]
    /// keeps one copy. No merge.
    Image,
}

/// Semantic conflict disposition. Shells use this for control flow.
///
/// [`MergeKind::Choice`] is part of the Contacts-shaped enum and is never
/// emitted. XMP is [`MergeKind::Auto`] or [`MergeKind::DeletedVersusModified`].
/// Image groups are [`MergeKind::KeepOne`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MergeKind {
    /// All sidecar fields merge deterministically.
    Auto,
    /// At least one field requires a user choice. Never produced here.
    Choice,
    /// The surviving sidecar disappeared while a copy retains the data.
    DeletedVersusModified,
    /// User must pick one file. Image groups only. Not an XMP field choice.
    KeepOne,
}

/// Display-ready conflict group plus typed disposition.
///
/// R6 role: command DTO.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ConflictRow {
    /// Folder-relative group key handed back to preview / resolve / keep.
    pub id: String,
    /// Same as [`Self::id`].
    pub title: String,
    /// Human-readable number of copies.
    pub subtitle: String,
    /// Human-readable disposition.
    pub trailing: String,
    /// Typed disposition for shell control flow.
    pub disposition: MergeKind,
    /// XMP merge vs image keep-one. The sheet branches on this.
    pub kind: ConflictKind,
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
    /// Auto / choice / deleted-versus-modified / keep-one.
    pub kind: MergeKind,
    /// Copy basenames that will be deleted on confirm.
    pub discarded: Vec<String>,
    /// Merged sidecar as the user will see it if they confirm.
    pub merged_fields: Vec<GalleryFieldRow>,
    /// Fields that differ. Empty when [`MergeKind::Auto`] — always empty here.
    pub fields: Vec<ConflictFieldPreview>,
}

/// Keep-one candidates for an image-file group. Does not write.
///
/// R6 role: command DTO.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ImageConflictPreview {
    /// Group id the shell hands back to [`ConflictSession::keep_image_copy`].
    pub id: String,
    /// Surviving file name (`photo.heic`).
    pub title: String,
    /// Candidate names: surviving first when it exists, then conflict copies.
    pub copies: Vec<String>,
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
    /// No group with that id, or the keep-one name is not in the group.
    NotFound,
    /// The id names an image-file group; only `.xmp` groups merge.
    NotAnXmpGroup {
        /// Display-ready recovery message.
        message: String,
        /// The user can pick a different group.
        user_actionable: bool,
    },
    /// The id names an `.xmp` group; only image groups keep-one.
    NotAnImageGroup {
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
            | Self::NotAnImageGroup { message, .. }
            | Self::Sidecar { message, .. } => formatter.write_str(message),
            Self::NotFound => formatter.write_str("not found"),
        }
    }
}

impl std::error::Error for ConflictError {}

/// Coarse-grained handle on Syncthing groups under one library root.
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

    /// `.xmp` and image-file groups. Unparseable XMP groups are omitted.
    /// [`MergeKind::Choice`] is never returned.
    pub fn conflict_rows(&self) -> Result<Vec<ConflictRow>, ConflictError> {
        let vfs = StdVfs::new();
        let mut rows = Vec::new();
        for group in gallery_scan::conflict_groups(&vfs, &self.root) {
            if is_xmp_group(&group) {
                match plan_group(&vfs, &self.root, &group) {
                    Ok(plan) => rows.push(ConflictRow {
                        id: plan.id.clone(),
                        title: plan.id,
                        subtitle: copies_label(group.copies.len()),
                        trailing: merge_trailing(plan.kind).into(),
                        disposition: plan.kind,
                        kind: ConflictKind::Xmp,
                    }),
                    Err(ConflictError::Sidecar { .. }) => {}
                    Err(error) => return Err(error),
                }
            } else {
                let id = relative_id(&self.root, &group);
                rows.push(ConflictRow {
                    id: id.clone(),
                    title: id,
                    subtitle: copies_label(group.copies.len()),
                    trailing: merge_trailing(MergeKind::KeepOne).into(),
                    disposition: MergeKind::KeepOne,
                    kind: ConflictKind::Image,
                });
            }
        }
        Ok(rows)
    }

    /// Confirmation DTO. Does not write or delete. Image groups are
    /// [`ConflictError::NotAnXmpGroup`].
    pub fn conflict_preview(&self, group_id: String) -> Result<ConflictPreview, ConflictError> {
        let vfs = StdVfs::new();
        let group = find_xmp_group(&vfs, &self.root, &group_id)?;
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
        let group = find_xmp_group(&vfs, &self.root, &group_id)?;
        let plan = plan_group(&vfs, &self.root, &group)?;
        apply_sidecar_conflict(
            &vfs,
            &plan.surviving_path,
            &plan.copy_paths,
            &plan.merge.bytes,
        )
        .map_err(from_meta)
    }

    /// Keep-one candidates. Does not write or delete. XMP groups are
    /// [`ConflictError::NotAnImageGroup`].
    pub fn image_preview(&self, group_id: String) -> Result<ImageConflictPreview, ConflictError> {
        let vfs = StdVfs::new();
        let group = find_image_group(&vfs, &self.root, &group_id)?;
        Ok(ImageConflictPreview {
            id: relative_id(&self.root, &group),
            title: group.canonical_name.clone(),
            copies: image_copy_names(&vfs, &self.root, &group)?,
        })
    }

    /// Keep one image file and delete the others. VFS `rename` / `remove`
    /// only. Does not read or rewrite image bytes.
    ///
    /// `surviving` is a basename, folder-relative path, or absolute path of
    /// the surviving file or one conflict copy. XMP groups are
    /// [`ConflictError::NotAnImageGroup`].
    pub fn keep_image_copy(
        &self,
        group_id: String,
        surviving: String,
    ) -> Result<(), ConflictError> {
        let vfs = StdVfs::new();
        let group = find_image_group(&vfs, &self.root, &group_id)?;
        let members = image_members(&vfs, &self.root, &group)?;
        let chosen =
            resolve_member(&members, &surviving, &self.root).ok_or(ConflictError::NotFound)?;
        let dest = group.surviving_path(&self.root);
        if chosen.path != dest {
            if vfs.try_exists(&dest).map_err(io_err)? {
                vfs.remove(&dest).map_err(io_err)?;
            }
            vfs.rename(&chosen.path, &dest).map_err(io_err)?;
        }
        for copy in &group.copies {
            if copy.path == chosen.path {
                continue;
            }
            match vfs.remove(&copy.path) {
                Ok(()) => {}
                Err(VfsError::NotFound { .. }) => {}
                Err(error) => return Err(io_err(error)),
            }
        }
        Ok(())
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

struct ImageMember {
    name: String,
    path: String,
}

fn find_any_group(
    vfs: &dyn Vfs,
    root: &str,
    group_id: &str,
) -> Result<ConflictGroup, ConflictError> {
    for group in gallery_scan::conflict_groups(vfs, root) {
        if relative_id(root, &group) == group_id {
            return Ok(group);
        }
    }
    Err(ConflictError::NotFound)
}

fn find_xmp_group(
    vfs: &dyn Vfs,
    root: &str,
    group_id: &str,
) -> Result<ConflictGroup, ConflictError> {
    let group = find_any_group(vfs, root, group_id)?;
    if is_xmp_group(&group) {
        return Ok(group);
    }
    Err(ConflictError::NotAnXmpGroup {
        message: format!("{group_id} is an image conflict group; only .xmp groups can be merged"),
        user_actionable: false,
    })
}

fn find_image_group(
    vfs: &dyn Vfs,
    root: &str,
    group_id: &str,
) -> Result<ConflictGroup, ConflictError> {
    let group = find_any_group(vfs, root, group_id)?;
    if is_xmp_group(&group) {
        return Err(ConflictError::NotAnImageGroup {
            message: format!("{group_id} is an .xmp conflict group; keep-one is for image files"),
            user_actionable: false,
        });
    }
    Ok(group)
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

fn image_members(
    vfs: &dyn Vfs,
    root: &str,
    group: &ConflictGroup,
) -> Result<Vec<ImageMember>, ConflictError> {
    let dest = group.surviving_path(root);
    let mut members = Vec::new();
    if vfs.try_exists(&dest).map_err(io_err)? {
        members.push(ImageMember {
            name: group.canonical_name.clone(),
            path: dest,
        });
    }
    for copy in &group.copies {
        members.push(ImageMember {
            name: copy.name.clone(),
            path: copy.path.clone(),
        });
    }
    if members.is_empty() {
        return Err(ConflictError::NotFound);
    }
    Ok(members)
}

fn image_copy_names(
    vfs: &dyn Vfs,
    root: &str,
    group: &ConflictGroup,
) -> Result<Vec<String>, ConflictError> {
    Ok(image_members(vfs, root, group)?
        .into_iter()
        .map(|member| member.name)
        .collect())
}

fn resolve_member<'a>(
    members: &'a [ImageMember],
    needle: &str,
    root: &str,
) -> Option<&'a ImageMember> {
    let needle = needle.trim();
    if needle.is_empty() {
        return None;
    }
    members.iter().find(|member| {
        member.path == needle
            || member.name == needle
            || relative_to_root(root, &member.path) == needle
            || basename(&member.path) == needle
    })
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
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
        MergeKind::KeepOne => "keep one",
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
