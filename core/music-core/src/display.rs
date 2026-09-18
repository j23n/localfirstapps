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

/// Kind of a global music search hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchKind {
    /// One track.
    Track,
    /// An album location.
    Album,
    /// An artist location.
    Artist,
    /// A playlist.
    Playlist,
}

/// One global search result. Shells pick an icon from [`SearchKind::symbol`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// Track id, `album:{name}`, `artist:{name}`, or playlist id.
    pub id: String,
    /// Result kind.
    pub kind: SearchKind,
    /// Primary label.
    pub title: String,
    /// Secondary label.
    pub subtitle: Option<String>,
}

impl SearchKind {
    /// Symbolic icon for this kind (HIG: symbolic style in lists).
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Track => "audio-x-generic-symbolic",
            Self::Album => "media-optical-symbolic",
            Self::Artist => "system-users-symbolic",
            Self::Playlist => "view-list-symbolic",
        }
    }

    /// Field kind shown on the second line (`Track`, `Album`, …).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Track => "Track",
            Self::Album => "Album",
            Self::Artist => "Artist",
            Self::Playlist => "Playlist",
        }
    }
}

/// Album locations for the Albums browse view. Grouped by album title.
#[must_use]
pub fn album_rows(store: &Store) -> Vec<TextRow> {
    let mut groups = std::collections::BTreeMap::<String, AlbumGroup>::new();
    for track in store.tracks() {
        let key = track.album.to_lowercase();
        let group = groups.entry(key).or_insert_with(|| AlbumGroup {
            title: track.album.clone(),
            artists: std::collections::BTreeSet::new(),
            count: 0,
        });
        group.artists.insert(track.artist.clone());
        group.count += 1;
    }
    groups
        .into_values()
        .map(|group| {
            let subtitle = if group.artists.len() == 1 {
                group.artists.into_iter().next()
            } else {
                Some("Various Artists".into())
            };
            TextRow {
                id: format!("album:{}", group.title),
                title: group.title,
                subtitle,
                trailing: Some(crate::projection::count_label(group.count, "track")),
            }
        })
        .collect()
}

/// Track whose embedded art stands in for an album location.
#[must_use]
pub fn album_art_track_id(store: &Store, album_id: &str) -> Option<String> {
    album_track_items(store, album_id)
        .into_iter()
        .find(|item| item.thumbnail_ref.starts_with("artwork:"))
        .map(|item| item.id)
}

/// Tracks in one album, already display-ready.
#[must_use]
pub fn album_track_items(store: &Store, album_id: &str) -> Vec<MediaItem> {
    let Some(album) = album_id.strip_prefix("album:") else {
        return Vec::new();
    };
    let mut items: Vec<MediaItem> = store
        .tracks()
        .iter()
        .filter(|track| track.album == album)
        .map(crate::projection::media_item)
        .collect();
    items.sort_by(|left, right| {
        left.label
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .cmp(&right.label.as_deref().unwrap_or_default().to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    items
}

/// Artist locations for the Artists browse view.
#[must_use]
pub fn artist_rows(store: &Store) -> Vec<TextRow> {
    let mut groups = std::collections::BTreeMap::<String, ArtistGroup>::new();
    for track in store.tracks() {
        let key = track.artist.to_lowercase();
        let group = groups.entry(key).or_insert_with(|| ArtistGroup {
            title: track.artist.clone(),
            albums: std::collections::BTreeSet::new(),
            count: 0,
        });
        group.albums.insert(track.album.clone());
        group.count += 1;
    }
    groups
        .into_values()
        .map(|group| TextRow {
            id: format!("artist:{}", group.title),
            title: group.title,
            subtitle: Some(crate::projection::count_label(group.albums.len(), "album")),
            trailing: Some(crate::projection::count_label(group.count, "track")),
        })
        .collect()
}

/// Track whose embedded art stands in for an artist location.
#[must_use]
pub fn artist_art_track_id(store: &Store, artist_id: &str) -> Option<String> {
    artist_track_items(store, artist_id)
        .into_iter()
        .find(|item| item.thumbnail_ref.starts_with("artwork:"))
        .map(|item| item.id)
}

/// Tracks by one artist, already display-ready.
#[must_use]
pub fn artist_track_items(store: &Store, artist_id: &str) -> Vec<MediaItem> {
    let Some(artist) = artist_id.strip_prefix("artist:") else {
        return Vec::new();
    };
    let mut items: Vec<MediaItem> = store
        .tracks()
        .iter()
        .filter(|track| track.artist == artist)
        .map(crate::projection::media_item)
        .collect();
    items.sort_by(|left, right| {
        left.label
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .cmp(&right.label.as_deref().unwrap_or_default().to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    items
}

/// First artwork-bearing track in a playlist, else the first playable track.
#[must_use]
pub fn playlist_art_track_id(store: &Store, playlist_id: &str) -> Option<String> {
    let playlist = store.playlist(playlist_id)?;
    let mut first = None;
    for entry in &playlist.entries {
        let Some(track_id) = &entry.track_id else {
            continue;
        };
        if first.is_none() {
            first = Some(track_id.clone());
        }
        if store.track(track_id).is_some_and(|track| track.has_artwork) {
            return Some(track_id.clone());
        }
    }
    first
}

/// Global search across tracks, albums, artists, and playlists.
#[must_use]
pub fn search_hits(store: &Store, query: &str) -> Vec<SearchHit> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for track in store.tracks() {
        if corpus_contains(track, &needle) {
            hits.push(SearchHit {
                id: track.id.clone(),
                kind: SearchKind::Track,
                title: track.title.clone(),
                subtitle: Some(format!("{} · {}", track.artist, track.album)),
            });
        }
    }
    let mut albums = std::collections::BTreeSet::new();
    let mut artists = std::collections::BTreeSet::new();
    for track in store.tracks() {
        if track.album.to_lowercase().contains(&needle) {
            albums.insert(track.album.clone());
        }
        if track.artist.to_lowercase().contains(&needle) {
            artists.insert(track.artist.clone());
        }
    }
    for album in albums {
        let count = store
            .tracks()
            .iter()
            .filter(|track| track.album == album)
            .count();
        hits.push(SearchHit {
            id: format!("album:{album}"),
            kind: SearchKind::Album,
            title: album,
            subtitle: Some(crate::projection::count_label(count, "track")),
        });
    }
    for artist in artists {
        let count = store
            .tracks()
            .iter()
            .filter(|track| track.artist == artist)
            .count();
        hits.push(SearchHit {
            id: format!("artist:{artist}"),
            kind: SearchKind::Artist,
            title: artist,
            subtitle: Some(crate::projection::count_label(count, "track")),
        });
    }
    for playlist in store.playlists() {
        if playlist.name.to_lowercase().contains(&needle) {
            hits.push(SearchHit {
                id: playlist.id.clone(),
                kind: SearchKind::Playlist,
                title: playlist.name.clone(),
                subtitle: Some(crate::projection::count_label(
                    playlist.entries.len(),
                    "track",
                )),
            });
        }
    }
    hits
}

fn corpus_contains(track: &crate::model::Track, needle: &str) -> bool {
    track.title.to_lowercase().contains(needle)
        || track.artist.to_lowercase().contains(needle)
        || track.album.to_lowercase().contains(needle)
}

struct AlbumGroup {
    title: String,
    artists: std::collections::BTreeSet<String>,
    count: usize,
}

struct ArtistGroup {
    title: String,
    albums: std::collections::BTreeSet<String>,
    count: usize,
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
