//! Core-owned search, deterministic sort, section, and display policy.

use std::cmp::Ordering;

use unicode_normalization::UnicodeNormalization;

use crate::display::{MediaItem, TextRow};
use crate::model::Track;
use crate::StoreError;

/// Library ordering selected by user intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOption {
    /// Title, then stable id.
    Title,
    /// Artist, title, then stable id.
    Artist,
    /// Album, title, then stable id.
    Album,
    /// Duration, title, then stable id.
    Duration,
}

impl SortOption {
    fn token(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Artist => "artist",
            Self::Album => "album",
            Self::Duration => "duration",
        }
    }
}

/// Display state for a synchronously opened folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryContentState {
    /// The selected folder contains no supported local audio.
    EmptyFolder,
    /// Audio exists, but the current search has no matches.
    NoMatches,
    /// At least one row is available.
    Content,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Section {
    pub id: String,
    pub title: String,
    pub track_indices: Vec<usize>,
}

/// Rebuildable in-memory projection. Audio files remain authoritative.
#[derive(Debug, Clone)]
pub(crate) struct LibraryProjection {
    pub generation: u64,
    pub query: String,
    pub sort: SortOption,
    pub sections: Vec<Section>,
    pub total_tracks: usize,
}

impl LibraryProjection {
    pub fn new(tracks: &[Track]) -> Self {
        let mut projection = Self {
            generation: 0,
            query: String::new(),
            sort: SortOption::Title,
            sections: Vec::new(),
            total_tracks: tracks.len(),
        };
        projection.rebuild(tracks);
        projection
    }

    pub fn set_view(&mut self, tracks: &[Track], query: String, sort: SortOption) {
        self.query = query;
        self.sort = sort;
        self.rebuild(tracks);
    }

    pub fn rebuild(&mut self, tracks: &[Track]) {
        self.generation = self.generation.saturating_add(1);
        self.total_tracks = tracks.len();
        let query = search_text(&self.query);
        let mut indices: Vec<usize> = tracks
            .iter()
            .enumerate()
            .filter(|(_, track)| query.is_empty() || corpus(track).contains(&query))
            .map(|(index, _)| index)
            .collect();
        indices.sort_by(|left, right| compare(&tracks[*left], &tracks[*right], self.sort));
        self.sections = make_sections(tracks, &indices, self.sort);
    }

    pub fn content_state(&self) -> LibraryContentState {
        if self.total_tracks == 0 {
            LibraryContentState::EmptyFolder
        } else if self
            .sections
            .iter()
            .all(|section| section.track_indices.is_empty())
        {
            LibraryContentState::NoMatches
        } else {
            LibraryContentState::Content
        }
    }

    pub fn section_rows(&self) -> Vec<TextRow> {
        self.sections
            .iter()
            .map(|section| TextRow {
                id: section.id.clone(),
                title: section.title.clone(),
                subtitle: None,
                trailing: Some(count_label(section.track_indices.len(), "track")),
            })
            .collect()
    }

    pub fn rows(
        &self,
        tracks: &[Track],
        section_id: &str,
        offset: usize,
        limit: usize,
        generation: u64,
    ) -> Result<Vec<MediaItem>, StoreError> {
        if generation != self.generation {
            return Err(StoreError::StaleGeneration {
                requested: generation,
                current: self.generation,
            });
        }
        if limit > 500 {
            return Err(StoreError::InvalidCommand(
                "A visible row window cannot exceed 500 tracks".into(),
            ));
        }
        let section = self
            .sections
            .iter()
            .find(|section| section.id == section_id)
            .ok_or(StoreError::NotFound)?;
        Ok(section
            .track_indices
            .iter()
            .skip(offset)
            .take(limit)
            .map(|index| media_item(&tracks[*index]))
            .collect())
    }
}

fn compare(left: &Track, right: &Track, sort: SortOption) -> Ordering {
    let by_title = || display_key(&left.title).cmp(&display_key(&right.title));
    let primary = match sort {
        SortOption::Title => by_title(),
        SortOption::Artist => display_key(&left.artist)
            .cmp(&display_key(&right.artist))
            .then_with(by_title),
        SortOption::Album => display_key(&left.album)
            .cmp(&display_key(&right.album))
            .then_with(by_title),
        SortOption::Duration => left.duration_ms.cmp(&right.duration_ms).then_with(by_title),
    };
    primary.then_with(|| left.id.cmp(&right.id))
}

fn make_sections(tracks: &[Track], indices: &[usize], sort: SortOption) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    for index in indices {
        let title = match sort {
            SortOption::Title => first_letter(&tracks[*index].title),
            SortOption::Artist => first_letter(&tracks[*index].artist),
            SortOption::Album => first_letter(&tracks[*index].album),
            SortOption::Duration => duration_bucket(tracks[*index].duration_ms).to_owned(),
        };
        if sections.last().is_none_or(|section| section.title != title) {
            sections.push(Section {
                id: format!("{}:{title}", sort.token()),
                title,
                track_indices: Vec::new(),
            });
        }
        sections
            .last_mut()
            .expect("section was inserted")
            .track_indices
            .push(*index);
    }
    sections
}

fn display_key(value: &str) -> String {
    value.trim().to_lowercase().nfc().collect::<String>()
}

fn search_text(value: &str) -> String {
    display_key(value)
}

fn corpus(track: &Track) -> String {
    search_text(&format!("{} {} {}", track.title, track.artist, track.album))
}

fn first_letter(value: &str) -> String {
    let key = value.trim().nfc().collect::<String>();
    let Some(first) = key.chars().next() else {
        return "#".into();
    };
    if first.is_alphabetic() {
        first.to_uppercase().collect()
    } else {
        "#".into()
    }
}

fn duration_bucket(duration_ms: u64) -> &'static str {
    match duration_ms {
        0..=59_999 => "Under 1 min",
        60_000..=179_999 => "1–3 min",
        180_000..=299_999 => "3–5 min",
        300_000..=599_999 => "5–10 min",
        _ => "10+ min",
    }
}

/// Display-ready media item for one track.
#[must_use]
pub fn media_item(track: &Track) -> MediaItem {
    MediaItem {
        id: track.id.clone(),
        thumbnail_ref: if track.has_artwork {
            format!("artwork:{}", track.id)
        } else {
            "symbol:music-note".into()
        },
        label: Some(track.title.clone()),
        badge: Some(format!(
            "{} · {} · {}",
            track.artist,
            track.album,
            format_duration(track.duration_ms)
        )),
    }
}

/// English integral duration formatting shared by both shells.
#[must_use]
pub fn format_duration(duration_ms: u64) -> String {
    let total_seconds = duration_ms / 1_000;
    format!("{}:{:02}", total_seconds / 60, total_seconds % 60)
}

/// English count formatting shared by both shells.
#[must_use]
pub fn count_label(count: usize, singular: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {singular}s")
    }
}
