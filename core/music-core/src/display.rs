//! Display-ready rows shared by Swift and future GTK bindings.

use localcore_vfs::Vfs;

use crate::merge::{plan_merge, ConflictDisposition};
use crate::path::{file_stem, relative_to};
use crate::projection::{count_label, format_duration};
use crate::{Store, StoreError};

/// ADR 0004 `text-row`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRow {
    /// Opaque key handed back to commands.
    pub id: String,
    /// Primary display text.
    pub title: String,
    /// Optional secondary display text.
    pub subtitle: Option<String>,
    /// Optional trailing display text.
    pub trailing: Option<String>,
}

/// ADR 0004 `media-item`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaItem {
    /// Opaque track id.
    pub id: String,
    /// Host-resolved artwork reference or a semantic symbol reference.
    pub thumbnail_ref: String,
    /// Already-selected primary label.
    pub label: Option<String>,
    /// Already-formatted artist, album, and duration.
    pub badge: Option<String>,
}

/// ADR 0004 action role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionRole {
    /// Non-destructive action.
    Normal,
    /// Destructive action requiring confirmation.
    Destructive,
}

/// ADR 0004 `action-row`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRow {
    /// Opaque command key.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Semantic role.
    pub role: ActionRole,
    /// Whether the action is currently available.
    pub enabled: bool,
}

/// ADR 0004 status severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusSeverity {
    /// Informational state.
    Info,
    /// Recoverable or preservation-related warning.
    Warning,
    /// Operation failed.
    Error,
}

/// ADR 0004 `status-row`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusRow {
    /// Opaque path or issue key.
    pub id: String,
    /// Display-ready issue text.
    pub message: String,
    /// Semantic severity.
    pub severity: StatusSeverity,
}

/// A display-ready Syncthing conflict row with typed control-flow state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictRow {
    /// Folder-relative group key.
    pub id: String,
    /// Surviving filename.
    pub title: String,
    /// Human-readable number of copies.
    pub subtitle: String,
    /// Human-readable disposition.
    pub trailing: String,
    /// Typed disposition; shells never parse [`Self::trailing`].
    pub disposition: ConflictDisposition,
}

/// Sorted playlist rows.
#[must_use]
pub fn playlist_rows(store: &Store) -> Vec<TextRow> {
    store
        .playlists()
        .iter()
        .map(|playlist| TextRow {
            id: playlist.id.clone(),
            title: playlist.name.clone(),
            subtitle: Some(relative_to(&store.root, &playlist.path)),
            trailing: Some(count_label(playlist.entries.len(), "track")),
        })
        .collect()
}

/// Core-formatted facts for the Settings Info section.
///
/// Shells display these rows verbatim so item-count grammar cannot drift
/// between GTK and Swift.
#[must_use]
pub fn settings_info_rows(store: &Store) -> Vec<TextRow> {
    vec![
        TextRow {
            id: "tracks".into(),
            title: "Tracks".into(),
            subtitle: None,
            trailing: Some(count_label(store.tracks().len(), "track")),
        },
        TextRow {
            id: "playlists".into(),
            title: "Playlists".into(),
            subtitle: None,
            trailing: Some(count_label(store.playlists().len(), "playlist")),
        },
        TextRow {
            id: "sync-conflicts".into(),
            title: "Sync Conflicts".into(),
            subtitle: None,
            trailing: Some(count_label(store.conflict_groups().len(), "group")),
        },
    ]
}

/// Ordered rows for one playlist. Missing and unsupported entries remain
/// visible rather than silently falling out of the projection.
pub fn playlist_entry_rows(store: &Store, playlist_id: &str) -> Result<Vec<TextRow>, StoreError> {
    let playlist = store.playlist(playlist_id).ok_or(StoreError::NotFound)?;
    Ok(playlist
        .entries
        .iter()
        .map(|entry| {
            if let Some(track_id) = &entry.track_id {
                if let Some(track) = store.track(track_id) {
                    return TextRow {
                        id: entry.id.clone(),
                        title: track.title.clone(),
                        subtitle: Some(track.artist.clone()),
                        trailing: Some(format_duration(track.duration_ms)),
                    };
                }
            }
            match &entry.resolved_path {
                Some(_) => TextRow {
                    id: entry.id.clone(),
                    title: file_stem(&entry.raw_path),
                    subtitle: Some(entry.raw_path.clone()),
                    trailing: Some("File not found".into()),
                },
                None => TextRow {
                    id: entry.id.clone(),
                    title: entry.raw_path.clone(),
                    subtitle: Some("Unsupported playlist entry".into()),
                    trailing: None,
                },
            }
        })
        .collect())
}

/// Typed actions for a playlist detail screen.
pub fn playlist_action_rows(
    store: &Store,
    playlist_id: &str,
) -> Result<Vec<ActionRow>, StoreError> {
    let playlist = store.playlist(playlist_id).ok_or(StoreError::NotFound)?;
    Ok(vec![
        ActionRow {
            id: "add-tracks".into(),
            label: "Add Tracks".into(),
            role: ActionRole::Normal,
            enabled: !store.tracks().is_empty(),
        },
        ActionRow {
            id: "delete-playlist".into(),
            label: "Delete Playlist".into(),
            role: ActionRole::Destructive,
            enabled: true,
        },
        ActionRow {
            id: "play-all".into(),
            label: "Play All".into(),
            role: ActionRole::Normal,
            enabled: playlist
                .entries
                .iter()
                .any(|entry| entry.track_id.is_some()),
        },
    ])
}

/// Every unresolved sync conflict. Audio conflicts are visible but cannot be
/// rewritten by playlist commands.
pub fn conflict_rows(vfs: &dyn Vfs, store: &Store) -> Result<Vec<ConflictRow>, StoreError> {
    store
        .conflict_groups()
        .iter()
        .map(|group| {
            let id = store.conflict_id(group);
            let format = crate::model::PlaylistFormat::from_extension(
                group
                    .canonical_name
                    .rsplit_once('.')
                    .map_or("", |(_, extension)| extension),
            );
            let disposition = match format {
                Some(crate::model::PlaylistFormat::M3u | crate::model::PlaylistFormat::M3u8) => {
                    plan_merge(vfs, &store.root, group)?.disposition
                }
                _ => ConflictDisposition::ManualOnly,
            };
            Ok(ConflictRow {
                id,
                title: group.canonical_name.clone(),
                subtitle: count_label(group.copies.len(), "copy"),
                trailing: disposition_label(disposition).into(),
                disposition,
            })
        })
        .collect()
}

/// Whole-document choices for a conflicting playlist order.
pub fn conflict_choice_rows(
    vfs: &dyn Vfs,
    store: &Store,
    group_id: &str,
) -> Result<Vec<TextRow>, StoreError> {
    let group = store.conflict_group(group_id).ok_or(StoreError::NotFound)?;
    let plan = plan_merge(vfs, &store.root, group)?;
    Ok(plan
        .sources
        .iter()
        .map(|(source, playlist)| TextRow {
            id: source.clone(),
            title: if source == "surviving" {
                "Keep surviving order".into()
            } else {
                format!("Use {source}")
            },
            subtitle: Some(count_label(playlist.entries.len(), "track")),
            trailing: Some("Choose".into()),
        })
        .collect())
}

fn disposition_label(disposition: ConflictDisposition) -> &'static str {
    match disposition {
        ConflictDisposition::Auto => "ready to merge",
        ConflictDisposition::Choice => "needs choice",
        ConflictDisposition::DeletedVersusModified => "keep modified copy",
        ConflictDisposition::ManualOnly => "manual file choice",
    }
}
