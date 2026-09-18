//! `SearchIndex.swift`, ported: the date-sorted photo list, the per-photo
//! search corpus, and the two-branch query.

use gallery_model::{CivilDateTime, PhotoFile};

use crate::text;

/// `Date.distantPast`, in Apple-reference-date seconds — what
/// `SearchIndex.sortPhotos` substitutes for a missing `dateTaken`.
///
/// A sentinel rather than `f64::NEG_INFINITY` so a photo that somehow *is*
/// dated to the distant past ties with the undated tail, exactly as in Swift,
/// instead of sorting above it.
pub const DISTANT_PAST: f64 = -63_114_076_800.0;

/// The per-photo corpus terms, before they are joined: the filename, then
/// `displayName` and `fullPath` for each tag, each lowercased.
///
/// Exposed because the fixture records this list verbatim — the field order
/// and the count (`1 + 2 × tags`) are both part of the contract.
pub fn corpus_terms(photo: &PhotoFile) -> Vec<String> {
    let mut terms = Vec::with_capacity(1 + photo.hierarchical_tags.len() * 2);
    terms.push(text::lowercased(&photo.filename));
    for tag in &photo.hierarchical_tags {
        terms.push(text::lowercased(&tag.display_name));
        terms.push(text::lowercased(&tag.full_path));
    }
    terms
}

/// The corpus entry a query is substring-matched against: the terms joined with
/// `\n`, in the canonical form `text::match_key` defines.
///
/// The newline join is load-bearing: it is why no substring can ever span two
/// terms, and why `"italy beach"` matches nothing while `"beach italy"` matches
/// a filename that literally contains it.
pub fn corpus_entry(photo: &PhotoFile) -> String {
    text::nfc(&corpus_terms(photo).join("\n"))
}

/// The sort key: `(dateTaken ?? .distantPast, url.path)`.
///
/// The path half is the tiebreak that stops a rescan from reshuffling the grid
/// — `sorted` is not documented stable and the input order comes off a
/// filesystem enumeration. It is NFC-normalised because Swift's `String <`
/// compares canonically-equivalent spellings as equal, so a decomposed and a
/// precomposed filename must not sort to two different places.
pub(crate) struct SortKey {
    pub date: f64,
    pub path: String,
}

impl SortKey {
    pub fn new(photo: &PhotoFile) -> Self {
        SortKey {
            date: photo.date_taken.map_or(DISTANT_PAST, |d| d.0),
            path: text::nfc(photo.url.path()),
        }
    }

    /// Date descending, then path ascending.
    pub fn cmp(&self, other: &SortKey) -> std::cmp::Ordering {
        other
            .date
            .partial_cmp(&self.date)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| self.path.cmp(&other.path))
    }
}

/// Kind of a Photos search hit. Shells pick an icon from [`Self::symbol`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SearchKind {
    /// `Places/*` or `Landmarks/*`.
    Location,
    /// `People/*`.
    People,
    /// `Scenes/*`.
    Scene,
    /// Capture year or month.
    Date,
    /// `Objects/*`.
    Objects,
}

/// One live-search match. Same shape as contacts: title, kind label, symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// Tag path or `date:YYYY` / `date:YYYY-MM`.
    pub id: String,
    /// Result kind.
    pub kind: SearchKind,
    /// Primary label (place, person, month…).
    pub title: String,
    /// Secondary label (already-formatted count).
    pub subtitle: Option<String>,
}

impl SearchKind {
    /// Symbolic icon for this kind (HIG: symbolic style in lists).
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Location => "mark-location-symbolic",
            Self::People => "system-users-symbolic",
            Self::Scene => "image-x-generic-symbolic",
            Self::Date => "x-office-calendar-symbolic",
            Self::Objects => "applications-engineering-symbolic",
        }
    }

    /// Field kind shown on the second line (`Location`, `People`, …).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Location => "Location",
            Self::People => "People",
            Self::Scene => "Scene",
            Self::Date => "Date",
            Self::Objects => "Objects",
        }
    }

    /// Map a hierarchical-tag namespace onto a search kind.
    #[must_use]
    pub fn from_namespace(namespace: Option<&str>) -> Option<Self> {
        match namespace.map(text::lowercased).as_deref() {
            Some("places") | Some("landmarks") => Some(Self::Location),
            Some("people") => Some(Self::People),
            Some("scenes") => Some(Self::Scene),
            Some("objects") => Some(Self::Objects),
            Some("date") => Some(Self::Date),
            _ => None,
        }
    }
}

/// English month names used by date search and date hits. Keep these
/// identical on Swift so a tapped suggestion stays a valid query.
pub const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Month name for `1..=12`. Empty for an out-of-range month.
#[must_use]
pub fn month_name(month: u32) -> &'static str {
    MONTH_NAMES
        .get(month.saturating_sub(1) as usize)
        .copied()
        .unwrap_or("")
}

/// A query that names a capture date rather than a corpus substring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateQuery {
    /// `2023`.
    Year(i32),
    /// `june`.
    Month(u32),
    /// `2023-06` or `june 2023`.
    YearMonth {
        /// Calendar year.
        year: i32,
        /// `1..=12`.
        month: u32,
    },
}

/// Parse a date-shaped query. Short or ambiguous strings stay `None` so
/// ordinary tag/filename search is unchanged.
#[must_use]
pub fn parse_date_query(query: &str) -> Option<DateQuery> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    if q.len() == 4 && q.bytes().all(|b| b.is_ascii_digit()) {
        return q.parse().ok().map(DateQuery::Year);
    }
    if q.len() == 7 && q.as_bytes().get(4) == Some(&b'-') {
        let year = q[..4].parse().ok()?;
        let month = q[5..].parse().ok()?;
        if (1..=12).contains(&month) {
            return Some(DateQuery::YearMonth { year, month });
        }
    }
    let folded = text::match_key(q);
    if let Some(month) = month_from_name(&folded) {
        return Some(DateQuery::Month(month));
    }
    let (left, right) = folded.split_once(' ')?;
    match (month_from_name(left), right.parse::<i32>().ok()) {
        (Some(month), Some(year)) => Some(DateQuery::YearMonth { year, month }),
        _ => match (left.parse::<i32>().ok(), month_from_name(right)) {
            (Some(year), Some(month)) => Some(DateQuery::YearMonth { year, month }),
            _ => None,
        },
    }
}

fn month_from_name(name: &str) -> Option<u32> {
    MONTH_NAMES
        .iter()
        .enumerate()
        .find(|(_, month)| text::match_key(month) == name)
        .map(|(index, _)| index as u32 + 1)
}

/// Whether `photo`'s capture date satisfies `query`. UTC civil time; the
/// Photos grid may group by local offset, but search stays timezone-free.
#[must_use]
pub fn photo_matches_date(photo: &PhotoFile, query: DateQuery) -> bool {
    let Some(date) = photo.date_taken else {
        return false;
    };
    let civil = CivilDateTime::from_unix_secs_f64(date.unix_secs_f64());
    match query {
        DateQuery::Year(year) => civil.year == year,
        DateQuery::Month(month) => civil.month == month,
        DateQuery::YearMonth { year, month } => civil.year == year && civil.month == month,
    }
}

/// Human-readable photo count, same grammar as the collections rows.
#[must_use]
pub fn photo_count_label(count: usize) -> String {
    if count == 1 {
        "1 photo".into()
    } else {
        format!("{count} photos")
    }
}
