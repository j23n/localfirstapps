//! Library indexes and the memory engine.
//!
//! Two shapes, because the two halves are genuinely different:
//!
//! * [`LibraryIndex`] is an **object** — it owns the photo table and every
//!   index over it, and the app queries it many times between rebuilds. A
//!   request/response function would have to be handed 20,000 photos per
//!   query.
//! * [`generate_memories`] / [`compute_scheduled_memories`] are **free
//!   functions** — pure over an inputs snapshot, exactly as
//!   `MemoryEngine.generate` was. The one piece of state a generation needs is
//!   a cancel flag, and that lives in [`MemoryGenerator`], which the caller
//!   creates per run.
//!
//! # Payload discipline
//!
//! Photos cross **into** the core once per rebuild, as [`ScanPhoto`] — the
//! record the scanner already marshals `PhotoFile` through, so there is one photo
//! wire format, not two. Photos cross **out** as ids: every list this module
//! returns (`sorted_photo_ids`, `search`, `photo_ids_for_tag`,
//! `MemoryRecord::photo_ids`) is a list of `StableId` strings, because the app
//! is already holding the `PhotoFile` those ids name and shipping the struct
//! back would double the traffic for nothing.
//!
//! # Threading
//!
//! Everything here is synchronous and does no locking beyond the index's own
//! `RwLock`. The requirement that these calls run **off the main thread**
//! (docs/architecture.md) is the app's to keep, and
//! `CoreLibraryIndex` / `CoreMemories` on the Swift side are where it is kept.
//! Nothing in this module blocks, so a caller that gets it wrong is slow rather
//! than deadlocked — which is precisely why the app also carries a generation
//! guard rather than relying on this file.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use gallery_index::{LibraryIndex as CoreIndex, TagSuggestion};
use gallery_memories::{
    cluster_key, compute_scheduled, generate_cancellable, Contact, GenerationInputs, LeafFolder,
    Memory, MemoryType, PersonLink, UtcOffset, SCHEDULED_MEMORY_HORIZON_DAYS,
};
use gallery_model::{AppleDate, CivilDateTime, HierarchicalTag, PhotoFile, StableId};

use crate::scanner::{photo_from_record, ScanPhoto};
use crate::view::{
    checked_window, GalleryMediaItem, GalleryTextRow, ViewAction, ViewContentState, ViewError,
    ViewSection, ViewSlotKind, ViewStructure,
};

// ---------------------------------------------------------------------------
// Shared wire records
// ---------------------------------------------------------------------------

/// `TagSuggestion.swift` — one tag bucket, flattened for the UI.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TagSuggestionRecord {
    /// `full_path` lowercased and NFC-folded. The bucket key, and the Swift
    /// `TagSuggestion.id`.
    pub id: String,
    /// Leaf segment.
    pub display_name: String,
    /// Canonical-cased hierarchical path.
    pub full_path: String,
    /// First segment; `None` for a flat tag.
    pub namespace: Option<String>,
    /// Photos credited to the bucket.
    pub count: u32,
    /// Most recent `dateTaken` in the bucket, reference-date seconds. Set for
    /// **people** suggestions only — the general list always leaves it `None`,
    /// which is what `tag_index.json` pins.
    pub latest_photo_date: Option<f64>,
}

impl TagSuggestionRecord {
    fn of(s: &TagSuggestion) -> Self {
        TagSuggestionRecord {
            id: s.id.clone(),
            display_name: s.display_name.clone(),
            full_path: s.full_path.clone(),
            namespace: s.namespace.clone(),
            count: s.count as u32,
            latest_photo_date: s.latest_photo_date,
        }
    }
}

/// What one [`LibraryIndex::build`] produced.
///
/// The sorted order and the aggregated tag lists come back together because
/// the app needs all three after every rebuild and a second boundary crossing
/// to fetch them would be pure overhead. It is also what lets the whole rebuild
/// be one `await` on the Swift side, which is what makes the generation guard
/// around it checkable.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LibraryIndexSummary {
    /// Photo ids, date descending with the `url.path` tiebreak. **This order is
    /// the grid.**
    pub sorted_photo_ids: Vec<String>,
    /// One suggestion per tag bucket, `(count desc, id asc)`.
    pub tags: Vec<TagSuggestionRecord>,
    /// The `People/…` subset, each carrying its most recent photo date.
    pub people: Vec<TagSuggestionRecord>,
    /// Time spent inside the core, for the `Built:` log line the performance
    /// gates are read from.
    pub build_millis: u64,
}

// ---------------------------------------------------------------------------
// LibraryIndex
// ---------------------------------------------------------------------------

/// The photo table and every index over it: the sorted order, the search
/// corpus, and the tag buckets.
///
/// One instance per Store, rebuilt wholesale from `allPhotos` after every
/// `apply(_:)` — the same contract `SearchIndex.build(allPhotos:)` and
/// `TagIndex.build(allPhotos:)` had. Partial updates were never part of it.
#[derive(uniffi::Object)]
pub struct LibraryIndex {
    /// `RwLock` rather than `Mutex`: rebuilds are rare and exclusive, queries
    /// are frequent and shared, and a query must never be able to observe a
    /// half-built index.
    inner: RwLock<Indexed>,
}

struct Indexed {
    index: CoreIndex,
    /// The aggregated tag list, cached from the build.
    ///
    /// `search` needs it: an exact tag-path query switches from substring
    /// matching to tag filtering, and the *virtual* prefix tags
    /// (`places/italy/lazio`, which no photo carries) only exist in this list.
    /// Swift passed the Store's copy in on every call, and a caller that
    /// forgot silently degraded every tag query to a substring match. Holding
    /// it here removes the way to get it wrong.
    tags: Vec<TagSuggestion>,
    people: Vec<TagSuggestion>,
    /// Generation of every id list derived from this table.
    generation: u64,
    /// The current Photos-screen projection. Structure returns these ids;
    /// content windows index the same vector after checking `generation`.
    visible_photo_ids: Vec<StableId>,
    visible_sections: Vec<PhotoViewSection>,
    /// Search/filter intent that produced `visible_photo_ids`.
    visible_key: String,
    /// Platform-resolved UTC offset for each photo at capture time. Parallel
    /// to the index photo table and populated once with the library build so a
    /// scheduled-memory run does not marshal 20,000 offsets again.
    photo_time_zone_offsets: Vec<i32>,
}

struct PhotoViewSection {
    id: String,
    title: String,
    item_ids: Vec<StableId>,
}

impl Default for LibraryIndex {
    fn default() -> Self {
        LibraryIndex {
            inner: RwLock::new(Indexed {
                index: CoreIndex::build(Vec::new()),
                tags: Vec::new(),
                people: Vec::new(),
                generation: 0,
                visible_photo_ids: Vec::new(),
                visible_sections: Vec::new(),
                visible_key: String::new(),
                photo_time_zone_offsets: Vec::new(),
            }),
        }
    }
}

#[uniffi::export]
impl LibraryIndex {
    /// An empty index. The app holds one for the process lifetime and rebuilds
    /// it; a fresh object per rebuild would drop the previous photo table only
    /// after the new one was built, doubling peak memory on a 20k library.
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(LibraryIndex::default())
    }

    /// Rebuild every index from `photos` and return the results the app needs
    /// straight away.
    ///
    /// Takes the photos by value: they become the index's photo table, so
    /// nothing is copied after the boundary crossing itself.
    pub fn build(&self, photos: Vec<ScanPhoto>) -> LibraryIndexSummary {
        self.rebuild(photos, Vec::new())
    }

    /// Rebuild and retain the platform's per-photo UTC offsets.
    ///
    /// The offsets are supplied on the same once-per-library crossing as the
    /// photos. Scheduled memories then reuse both tables instead of rebuilding
    /// and re-marshalling a second 20,000-photo generation snapshot.
    pub fn build_with_time_zone_offsets(
        &self,
        photos: Vec<ScanPhoto>,
        photo_time_zone_offsets: Vec<i32>,
    ) -> LibraryIndexSummary {
        self.rebuild(photos, photo_time_zone_offsets)
    }

    /// Compute the widget's scheduled horizon over the library table already
    /// retained by this index.
    ///
    /// Only the small platform context crosses per run. Photo records and
    /// capture-time offsets crossed once, at build.
    pub fn compute_scheduled(
        &self,
        context: ScheduledMemoryContext,
        horizon_days: i64,
        hidden_memory_ids: Vec<String>,
    ) -> Vec<ScheduledMemoryRecord> {
        let guard = read(&self.inner);
        let hidden: HashSet<String> = hidden_memory_ids.into_iter().collect();
        let inputs = scheduled_inputs(&guard, context);
        compute_scheduled(&inputs, horizon_days, &hidden)
            .iter()
            .map(scheduled_record)
            .collect()
    }

    /// Number of photos the scheduled-horizon reuse path currently owns.
    pub fn scheduled_photo_count(&self) -> u32 {
        read(&self.inner).index.photos().len() as u32
    }

    /// Rebuild every index from `photos` and return the results the app needs
    /// straight away.
    fn rebuild(
        &self,
        photos: Vec<ScanPhoto>,
        photo_time_zone_offsets: Vec<i32>,
    ) -> LibraryIndexSummary {
        let started = std::time::Instant::now();
        let photos: Vec<PhotoFile> = photos.into_iter().map(photo_from_record).collect();
        let index = CoreIndex::build(photos);
        let (tags, people) = index.tag_suggestions();
        let summary = LibraryIndexSummary {
            sorted_photo_ids: index.sorted_photo_ids().iter().map(ids).collect(),
            tags: tags.iter().map(TagSuggestionRecord::of).collect(),
            people: people.iter().map(TagSuggestionRecord::of).collect(),
            build_millis: started.elapsed().as_millis() as u64,
        };
        let visible_photo_ids = index.sorted_photo_ids();
        let visible_sections =
            photo_view_sections(&index, &visible_photo_ids, &photo_time_zone_offsets);
        let mut guard = write(&self.inner);
        guard.generation = guard.generation.saturating_add(1);
        guard.visible_photo_ids = visible_photo_ids;
        guard.visible_sections = visible_sections;
        guard.visible_key.clear();
        guard.index = index;
        guard.tags = tags;
        guard.people = people;
        guard.photo_time_zone_offsets = photo_time_zone_offsets;
        summary
    }

    /// Apply Photos-screen search/tag intent and return cheap structure.
    ///
    /// The whole ordered id list is structure by ADR 0003 R4. The records
    /// behind those ids are fetched only through [`Self::photo_window`].
    pub fn set_photo_view(&self, query: String, required_tag_paths: Vec<String>) -> ViewStructure {
        let key = format!("{query}\u{0}{}", required_tag_paths.join("\u{0}"));
        let mut guard = write(&self.inner);
        if guard.visible_key != key {
            let required: Vec<TagSuggestion> = required_tag_paths
                .iter()
                .map(|path| suggestion_for_path(path))
                .collect();
            let visible_photo_ids: Vec<StableId> = guard
                .index
                .search(&query, &required, &guard.tags)
                .into_iter()
                .map(|photo| photo.id)
                .collect();
            let visible_sections = photo_view_sections(
                &guard.index,
                &visible_photo_ids,
                &guard.photo_time_zone_offsets,
            );
            guard.visible_photo_ids = visible_photo_ids;
            guard.visible_sections = visible_sections;
            guard.visible_key = key;
            guard.generation = guard.generation.saturating_add(1);
        }
        photo_structure(&guard)
    }

    /// Apply an id-only drill-in intent (folder, memory, or saved selection).
    ///
    /// The shell supplies structure ids, never photo records. Unknown/removed
    /// ids are dropped and the caller's order is preserved.
    pub fn set_photo_ids_view(
        &self,
        view_id: String,
        photo_ids: Vec<String>,
        query: String,
        required_tag_paths: Vec<String>,
    ) -> ViewStructure {
        let key = format!(
            "ids\u{0}{view_id}\u{0}{query}\u{0}{}\u{0}{}",
            required_tag_paths.join("\u{0}"),
            photo_ids.join("\u{0}"),
        );
        let mut guard = write(&self.inner);
        if guard.visible_key != key {
            let matches: Option<HashSet<StableId>> =
                (!query.is_empty() || !required_tag_paths.is_empty()).then(|| {
                    let required: Vec<TagSuggestion> = required_tag_paths
                        .iter()
                        .map(|path| suggestion_for_path(path))
                        .collect();
                    guard
                        .index
                        .search(&query, &required, &guard.tags)
                        .into_iter()
                        .map(|photo| photo.id)
                        .collect()
                });
            let visible_photo_ids: Vec<StableId> = photo_ids
                .into_iter()
                .map(|id| parse_id(&id))
                .filter(|id| {
                    guard.index.photo(*id).is_some()
                        && matches
                            .as_ref()
                            .is_none_or(|matching| matching.contains(id))
                })
                .collect();
            let visible_sections = photo_view_sections(
                &guard.index,
                &visible_photo_ids,
                &guard.photo_time_zone_offsets,
            );
            guard.visible_photo_ids = visible_photo_ids;
            guard.visible_sections = visible_sections;
            guard.visible_key = key;
            guard.generation = guard.generation.saturating_add(1);
        }
        photo_structure(&guard)
    }

    /// Current Photos-screen structure without changing search intent.
    pub fn photo_structure(&self) -> ViewStructure {
        photo_structure(&read(&self.inner))
    }

    /// Display-ready photos for one visible window.
    pub fn photo_window(
        &self,
        section_id: String,
        offset: u64,
        limit: u64,
        generation: u64,
    ) -> Result<Vec<GalleryMediaItem>, ViewError> {
        let guard = read(&self.inner);
        // Generation wins over section lookup: every section id belongs to
        // the requested structure, so an absent old id is stale, not unknown.
        checked_window(generation, guard.generation, 0, 0, 0)?;
        let item_ids = if section_id == "photos" {
            guard.visible_photo_ids.as_slice()
        } else if let Some(section) = guard
            .visible_sections
            .iter()
            .find(|section| section.id == section_id)
        {
            section.item_ids.as_slice()
        } else {
            return Err(ViewError::SectionNotFound {
                section_id,
                message: "That Gallery photo section no longer exists.".into(),
                user_actionable: true,
            });
        };
        let range = checked_window(generation, guard.generation, offset, limit, item_ids.len())?;
        let mut rows = Vec::with_capacity(range.len());
        for id in &item_ids[range] {
            if let Some(photo) = guard.index.photo(*id) {
                rows.push(photo_media_item(photo));
            }
        }
        Ok(rows)
    }

    /// Tag-picker structure. The list is ids only; labels and counts are
    /// returned by [`Self::tag_window`].
    pub fn tag_structure(&self) -> ViewStructure {
        let guard = read(&self.inner);
        let ids = guard.tags.iter().map(|tag| tag.id.clone()).collect();
        ViewStructure {
            state: content_state(guard.tags.len()),
            sections: vec![ViewSection {
                id: "tags".into(),
                title: "Tags".into(),
                slot_kind: ViewSlotKind::TextRow,
                item_ids: ids,
            }],
            actions: Vec::new(),
            generation: guard.generation,
        }
    }

    /// Display-ready tag rows for one visible window.
    pub fn tag_window(
        &self,
        section_id: String,
        offset: u64,
        limit: u64,
        generation: u64,
    ) -> Result<Vec<GalleryTextRow>, ViewError> {
        if section_id != "tags" {
            return Err(ViewError::SectionNotFound {
                section_id,
                message: "That Gallery tag section no longer exists.".into(),
                user_actionable: true,
            });
        }
        let guard = read(&self.inner);
        let range = checked_window(
            generation,
            guard.generation,
            offset,
            limit,
            guard.tags.len(),
        )?;
        Ok(guard.tags[range]
            .iter()
            .map(|tag| GalleryTextRow {
                id: tag.id.clone(),
                title: tag.display_name.clone(),
                subtitle: Some(tag.full_path.replace('/', " › ")),
                trailing: Some(photo_count_label(tag.count)),
            })
            .collect())
    }

    /// Generation currently guarding every library window.
    pub fn view_generation(&self) -> u64 {
        read(&self.inner).generation
    }

    /// The date-descending photo order. Backs `store.sortedPhotos`.
    pub fn sorted_photo_ids(&self) -> Vec<String> {
        read(&self.inner)
            .index
            .sorted_photo_ids()
            .iter()
            .map(ids)
            .collect()
    }

    /// `TagIndex.photos(forTag:)` — the photos credited to `full_path`,
    /// including the `Places/…` prefix expansion, in `allPhotos` order.
    pub fn photo_ids_for_tag(&self, full_path: String) -> Vec<String> {
        read(&self.inner)
            .index
            .photos_for_tag(&full_path)
            .into_iter()
            .map(|p| p.id.to_string())
            .collect()
    }

    /// `SearchIndex.search(query:requiredTags:allTags:)`, in sorted order.
    ///
    /// Required tags arrive as plain paths rather than as whole suggestions:
    /// the only two fields the Swift read off a `TagSuggestion` here were
    /// `fullPath` and `namespace`, and the second is the first's leading
    /// segment. Re-deriving it removes a record from the wire and a way for the
    /// two to disagree.
    pub fn search(&self, query: String, required_tag_paths: Vec<String>) -> Vec<String> {
        let guard = read(&self.inner);
        let required: Vec<TagSuggestion> = required_tag_paths
            .iter()
            .map(|p| suggestion_for_path(p))
            .collect();
        guard
            .index
            .search(&query, &required, &guard.tags)
            .into_iter()
            .map(|p| p.id.to_string())
            .collect()
    }

    /// The aggregated tag list and the `People/…` subset — the same pair
    /// [`Self::build`] returned, for a caller that has lost it.
    pub fn tag_suggestions(&self) -> LibraryTagSuggestions {
        let guard = read(&self.inner);
        LibraryTagSuggestions {
            tags: guard.tags.iter().map(TagSuggestionRecord::of).collect(),
            people: guard.people.iter().map(TagSuggestionRecord::of).collect(),
        }
    }

    /// How many photos the index currently holds. Cheap; used by the app's
    /// "did the rebuild I am waiting on actually land" assertions and by tests.
    pub fn photo_count(&self) -> u32 {
        read(&self.inner).index.photos().len() as u32
    }
}

fn photo_view_sections(
    index: &CoreIndex,
    item_ids: &[StableId],
    time_zone_offsets: &[i32],
) -> Vec<PhotoViewSection> {
    let offsets: HashMap<StableId, i32> = index
        .photos()
        .iter()
        .enumerate()
        .map(|(position, photo)| {
            (
                photo.id,
                time_zone_offsets.get(position).copied().unwrap_or(0),
            )
        })
        .collect();
    let mut sections: Vec<PhotoViewSection> = Vec::new();
    let mut current_key = String::new();
    for (position, id) in item_ids.iter().copied().enumerate() {
        let (key, title) = index
            .photo(id)
            .and_then(|photo| photo.date_taken)
            .map(|date| {
                let local_unix =
                    date.unix_secs_f64() + f64::from(offsets.get(&id).copied().unwrap_or(0));
                let civil = CivilDateTime::from_unix_secs_f64(local_unix);
                (
                    format!("{:04}-{:02}", civil.year, civil.month),
                    format!("{} {}", month_name(civil.month), civil.year),
                )
            })
            .unwrap_or_else(|| ("unknown".into(), "Unknown Date".into()));
        if key != current_key {
            current_key = key.clone();
            sections.push(PhotoViewSection {
                id: format!("photos:{position}:{key}"),
                title,
                item_ids: Vec::new(),
            });
        }
        sections
            .last_mut()
            .expect("a section was just created")
            .item_ids
            .push(id);
    }
    sections
}

fn month_name(month: u32) -> &'static str {
    const MONTHS: [&str; 12] = [
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
    month
        .checked_sub(1)
        .and_then(|index| MONTHS.get(index as usize))
        .copied()
        .unwrap_or("Unknown")
}

fn photo_structure(indexed: &Indexed) -> ViewStructure {
    ViewStructure {
        state: content_state(indexed.visible_photo_ids.len()),
        sections: indexed
            .visible_sections
            .iter()
            .map(|section| ViewSection {
                id: section.id.clone(),
                title: section.title.clone(),
                slot_kind: ViewSlotKind::MediaItem,
                item_ids: section.item_ids.iter().map(ToString::to_string).collect(),
            })
            .collect(),
        actions: vec![
            ViewAction {
                id: "search".into(),
                enabled: true,
                disabled_reason: None,
            },
            ViewAction {
                id: "select".into(),
                enabled: !indexed.visible_photo_ids.is_empty(),
                disabled_reason: indexed
                    .visible_photo_ids
                    .is_empty()
                    .then(|| "There are no photos to select.".into()),
            },
        ],
        generation: indexed.generation,
    }
}

fn content_state(count: usize) -> ViewContentState {
    if count == 0 {
        ViewContentState::Empty
    } else {
        ViewContentState::Content
    }
}

fn photo_media_item(photo: &PhotoFile) -> GalleryMediaItem {
    let badge = if photo.is_video {
        Some("Video".into())
    } else if photo.live_photo_video_url.is_some() {
        Some("Live Photo".into())
    } else {
        None
    };
    GalleryMediaItem {
        id: photo.id.to_string(),
        thumbnail_ref: photo.url.path().to_string(),
        label: photo.date_taken.map(AppleDate::to_utc_string),
        accessibility_label: Some(photo.filename.clone()),
        badge,
    }
}

fn photo_count_label(count: usize) -> String {
    if count == 1 {
        "1 photo".into()
    } else {
        format!("{count} photos")
    }
}

/// [`LibraryIndex::tag_suggestions`]' pair.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LibraryTagSuggestions {
    pub tags: Vec<TagSuggestionRecord>,
    pub people: Vec<TagSuggestionRecord>,
}

/// A `TagSuggestion` carrying only what `search` reads off one.
fn suggestion_for_path(full_path: &str) -> TagSuggestion {
    let tag = HierarchicalTag::new(full_path);
    TagSuggestion {
        id: tag.full_path.clone(),
        display_name: tag.display_name,
        full_path: tag.full_path,
        namespace: tag.namespace,
        count: 0,
        latest_photo_date: None,
    }
}

fn ids(id: &StableId) -> String {
    id.to_string()
}

/// Take a lock, ignoring poisoning — the data behind it is an index that is
/// either the old one or the new one, never half of each, and refusing to
/// answer for the rest of the process is the worse failure.
fn read(lock: &RwLock<Indexed>) -> std::sync::RwLockReadGuard<'_, Indexed> {
    lock.read().unwrap_or_else(|p| p.into_inner())
}

fn write(lock: &RwLock<Indexed>) -> std::sync::RwLockWriteGuard<'_, Indexed> {
    lock.write().unwrap_or_else(|p| p.into_inner())
}

// ---------------------------------------------------------------------------
// Memory records
// ---------------------------------------------------------------------------

/// `MemoryType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MemoryKind {
    OnThisDay,
    YearsAgo,
    /// Never produced. The Swift enum keeps the case so old caches decode; so
    /// does this one, for the same reason.
    PersonOverTime,
    FolderEvent,
    PhotoDensity,
    Trip,
    Birthday,
}

impl MemoryKind {
    fn of(kind: MemoryType) -> Self {
        match kind {
            MemoryType::OnThisDay => MemoryKind::OnThisDay,
            MemoryType::YearsAgo => MemoryKind::YearsAgo,
            MemoryType::PersonOverTime => MemoryKind::PersonOverTime,
            MemoryType::FolderEvent => MemoryKind::FolderEvent,
            MemoryType::PhotoDensity => MemoryKind::PhotoDensity,
            MemoryType::Trip => MemoryKind::Trip,
            MemoryType::Birthday => MemoryKind::Birthday,
        }
    }
}

/// One generated memory.
///
/// `date_range` is two optional fields rather than one optional pair because
/// UniFFI has no tuple: both are `Some` or both are `None`, and
/// [`MemoryRecord::of`] is the only thing that constructs them.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MemoryRecord {
    pub id: String,
    pub kind: MemoryKind,
    pub title: String,
    pub subtitle: Option<String>,
    /// Ordered — this is the slideshow order and part of the contract.
    pub photo_ids: Vec<String>,
    pub cover_photo_id: String,
    /// Reference-date seconds.
    pub date_range_start: Option<f64>,
    pub date_range_end: Option<f64>,
    /// The ladder score **before** the daily jitter. The jitter is never
    /// stored; its only observable effect is the order of the returned list.
    pub score: f64,
    pub years_ago: Option<i32>,
    pub person_name: Option<String>,
}

impl MemoryRecord {
    fn of(m: &Memory) -> Self {
        MemoryRecord {
            id: m.id.clone(),
            kind: MemoryKind::of(m.kind),
            title: m.title.clone(),
            subtitle: m.subtitle.clone(),
            photo_ids: m.photo_ids.iter().map(ids).collect(),
            cover_photo_id: m.cover_photo_id.to_string(),
            date_range_start: m.date_range.map(|(a, _)| a.0),
            date_range_end: m.date_range.map(|(_, b)| b.0),
            score: m.score,
            years_ago: m.years_ago,
            person_name: m.person_name.clone(),
        }
    }
}

/// A memory pre-published for a future day, with the window it is valid in.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ScheduledMemoryRecord {
    pub memory: MemoryRecord,
    /// Local midnight of the day it is about, reference-date seconds.
    pub valid_from: f64,
    /// Local midnight of the following day.
    pub valid_to: f64,
}

// ---------------------------------------------------------------------------
// Memory inputs
// ---------------------------------------------------------------------------

/// A leaf `PhotoFolder`, by reference into the photo list rather than by value.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MemoryLeafFolder {
    /// `PhotoFolder.id`. The memory id is `"folder-<this>"`.
    pub id: String,
    pub name: String,
    /// This folder's own photos, in listing order.
    pub photo_ids: Vec<String>,
}

/// `ContactInfo`, reduced to the fields the engine reads. `birthday.year` is
/// routinely absent in address-book data and is never consulted.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MemoryContact {
    pub id: String,
    pub given_name: String,
    pub family_name: String,
    pub birthday_month: Option<u32>,
    pub birthday_day: Option<u32>,
}

/// One entry of `personContactLinks`: an explicit decision about a `People/…`
/// tag that overrides the name-based auto-match.
///
/// `contact_id == None` is `PersonLink.disabled` — "this tag is not a person in
/// the address book", which suppresses the memory entirely.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MemoryPersonLink {
    pub person_path: String,
    pub contact_id: Option<String>,
}

/// A `[String: Date]` entry — seen memories, surfaced clusters.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MemoryDateEntry {
    pub key: String,
    /// Reference-date seconds.
    pub date: f64,
}

/// Small platform context for the scheduled-memory horizon.
///
/// Photo content is deliberately absent: [`LibraryIndex::compute_scheduled`]
/// reuses the index's retained table and capture-time UTC offsets. Contacts,
/// clock and horizon offsets are the minimum platform-owned values the pure
/// core cannot discover itself.
///
/// R6 role: host-port DTO.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ScheduledMemoryContext {
    pub leaf_folders: Vec<MemoryLeafFolder>,
    pub contacts: Vec<MemoryContact>,
    pub person_contact_links: Vec<MemoryPersonLink>,
    pub birthdays_enabled: bool,
    pub me_person_path: String,
    pub hidden_people: Vec<String>,
    /// "Now", reference-date seconds.
    pub now: f64,
    pub time_zone_offset_seconds: i32,
    pub horizon_offset_seconds: Vec<i32>,
    pub seed: String,
    pub seen_memory_ids: Vec<MemoryDateEntry>,
    pub surfaced_clusters: Vec<MemoryDateEntry>,
}

/// `MemoryCoordinator.GenerationInputs` plus the clock, zone, seed and the
/// seen/cool-down state — everything the engine reads.
///
#[derive(Debug, Clone, uniffi::Record)]
pub struct MemoryGenerationInputs {
    pub photos: Vec<ScanPhoto>,
    /// `Calendar.current.timeZone.secondsFromGMT(for: photo.dateTaken)`, one
    /// per entry of `photos`, in the same order.
    ///
    /// **Empty is legal** and means "use `time_zone_offset_seconds` for every
    /// photo" — the behaviour before this field existed. Supplying them matters
    /// for a zone with DST: a single offset resolved at `now` buckets a summer
    /// photo on a different day depending on the season the run happens in,
    /// which moves `density-*` and `trip-*` ids — and therefore cluster keys —
    /// twice a year, so the cool-down and seen penalties stop matching the
    /// history the user's own taps wrote.
    pub photo_time_zone_offsets: Vec<i32>,
    pub leaf_folders: Vec<MemoryLeafFolder>,
    pub contacts: Vec<MemoryContact>,
    pub person_contact_links: Vec<MemoryPersonLink>,
    pub birthdays_enabled: bool,
    /// The user's own `People/…` tag, dropped from trip titles. Empty = unset.
    pub me_person_path: String,
    pub hidden_people: Vec<String>,
    /// "Now", reference-date seconds.
    pub now: f64,
    /// The UTC offset in seconds **at `now`**:
    /// `Calendar.current.timeZone.secondsFromGMT(for: now)`.
    ///
    /// `Calendar.current.timeZone`, not `TimeZone.current`, and the difference
    /// is load-bearing: `TimeZone.current` is cached and does not track an
    /// `NSTimeZone.default` override, so it answers GMT in exactly the
    /// situation the non-UTC conformance scenario creates, and answers a stale
    /// zone on a device whose zone changed while the app was running.
    /// `Calendar.current.timeZone` is what the deleted engine read.
    ///
    /// Not an IANA zone: the core has no tz database. Today, the horizon and
    /// the penalty windows are computed in this offset; each *photo* is bucketed
    /// in its own (see [`Self::photo_time_zone_offsets`]).
    pub time_zone_offset_seconds: i32,
    /// `Calendar.current.timeZone.secondsFromGMT(for: <that day's local noon>)`
    /// for each day of the pre-publish horizon, indexed by days from today —
    /// entry 0 is today.
    ///
    /// **Empty is legal** and means "use `time_zone_offset_seconds` for every
    /// day", the behaviour before this field existed. Supplying it matters for
    /// a zone with DST: seven days can straddle a transition, and a
    /// pre-published item's validity window is compared against the wall clock.
    /// Noon rather than midnight because midnight is the instant a transition
    /// can land on. Only `compute_scheduled_memories` reads it; a table two
    /// entries longer than the horizon covers the last window's close.
    pub horizon_offset_seconds: Vec<i32>,
    /// Drives the daily jitter: the day key for a normal run, a time-based
    /// value for force-regenerate.
    pub seed: String,
    /// Memory id → when the user last opened it. −30 within ~6 months.
    pub seen_memory_ids: Vec<MemoryDateEntry>,
    /// Cluster key → when the cluster last surfaced. −25 within 3 days.
    pub surfaced_clusters: Vec<MemoryDateEntry>,
}

impl MemoryGenerationInputs {
    fn into_engine_inputs(self) -> GenerationInputs {
        let contacts: Vec<Contact> = self
            .contacts
            .into_iter()
            .map(|c| Contact {
                id: c.id,
                given_name: c.given_name,
                family_name: c.family_name,
                birthday_month: c.birthday_month,
                birthday_day: c.birthday_day,
            })
            .collect();
        let photos: Vec<PhotoFile> = self.photos.into_iter().map(photo_from_record).collect();
        GenerationInputs {
            photos,
            photo_time_zone_offsets: self.photo_time_zone_offsets,
            leaf_folders: self
                .leaf_folders
                .into_iter()
                .map(|f| LeafFolder {
                    // Parse, and fall back to deriving: the caller's spelling
                    // is the one its own photo ids were keyed by.
                    id: parse_id(&f.id),
                    name: f.name,
                    photo_ids: f.photo_ids.iter().map(|s| parse_id(s)).collect(),
                })
                .collect(),
            person_contact_links: self
                .person_contact_links
                .into_iter()
                .map(|l| {
                    let link = match l.contact_id {
                        Some(id) => PersonLink::Manual(id),
                        None => PersonLink::Disabled,
                    };
                    (l.person_path, link)
                })
                .collect(),
            // `contactsByLowerName` is not on the wire at all: the engine
            // derives it from `contacts` the way `ContactLinker.index` does
            // (full name, folded, first write wins), and sending both would let
            // the two disagree across the boundary.
            contacts,
            birthdays_enabled: self.birthdays_enabled,
            me_person_path: self.me_person_path,
            hidden_people: self.hidden_people.into_iter().collect(),
            now: AppleDate(self.now),
            time_zone: UtcOffset(self.time_zone_offset_seconds),
            horizon_time_zone_offsets: self.horizon_offset_seconds,
            seed: self.seed,
            seen_memory_ids: date_map(self.seen_memory_ids),
            surfaced_clusters: date_map(self.surfaced_clusters),
        }
    }
}

fn scheduled_inputs(indexed: &Indexed, context: ScheduledMemoryContext) -> GenerationInputs {
    let mut offsets = Vec::with_capacity(indexed.index.photos().len());
    let photos: Vec<PhotoFile> = indexed
        .index
        .photos()
        .iter()
        .enumerate()
        .map(|(position, photo)| {
            offsets.push(
                indexed
                    .photo_time_zone_offsets
                    .get(position)
                    .copied()
                    .unwrap_or(context.time_zone_offset_seconds),
            );
            photo.clone()
        })
        .collect();
    let ladder_photo_count = photos.len();
    GenerationInputs {
        photos,
        ladder_photo_count,
        photo_time_zone_offsets: offsets,
        horizon_time_zone_offsets: context.horizon_offset_seconds,
        leaf_folders: context
            .leaf_folders
            .into_iter()
            .map(|folder| LeafFolder {
                id: parse_id(&folder.id),
                name: folder.name,
                photo_ids: folder
                    .photo_ids
                    .into_iter()
                    .map(|id| parse_id(&id))
                    .collect(),
            })
            .collect(),
        contacts: context
            .contacts
            .into_iter()
            .map(|contact| Contact {
                id: contact.id,
                given_name: contact.given_name,
                family_name: contact.family_name,
                birthday_month: contact.birthday_month,
                birthday_day: contact.birthday_day,
            })
            .collect(),
        person_contact_links: context
            .person_contact_links
            .into_iter()
            .map(|link| {
                let value = match link.contact_id {
                    Some(id) => PersonLink::Manual(id),
                    None => PersonLink::Disabled,
                };
                (link.person_path, value)
            })
            .collect(),
        birthdays_enabled: context.birthdays_enabled,
        me_person_path: context.me_person_path,
        hidden_people: context.hidden_people.into_iter().collect(),
        now: AppleDate(context.now),
        time_zone: UtcOffset(context.time_zone_offset_seconds),
        seed: context.seed,
        seen_memory_ids: date_map(context.seen_memory_ids),
        surfaced_clusters: date_map(context.surfaced_clusters),
    }
}

/// Splice the two per-photo offset tables into one parallel to the combined
/// photo table.
///
/// Either side may be empty — that is the documented "no calendar available"
/// input, and both empty means no table at all (the pre-per-photo-offset
/// behaviour). A short or over-long table is padded/truncated with the `now`
/// offset rather than trusted, because the alternative is either a panic inside
/// a background task or a silent slide of every later photo onto the wrong
/// offset. Padding with `now` degrades exactly to what a caller that sent no
/// table would have got.
fn concat_offsets(
    ladder: Vec<i32>,
    extra: Vec<i32>,
    ladder_count: usize,
    total: usize,
    now_offset: i32,
) -> Vec<i32> {
    if ladder.is_empty() && extra.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(total);
    out.extend(ladder.into_iter().take(ladder_count));
    out.resize(ladder_count, now_offset);
    out.extend(extra);
    out.resize(total, now_offset);
    out
}

fn date_map(entries: Vec<MemoryDateEntry>) -> HashMap<String, AppleDate> {
    entries
        .into_iter()
        .map(|e| (e.key, AppleDate(e.date)))
        .collect()
}

/// A caller-supplied id, parsed. A malformed one becomes the nil id rather than
/// a panic: it will simply match no photo, which is the same outcome the Swift
/// `UUID(uuidString:)` optional produced.
fn parse_id(raw: &str) -> StableId {
    uuid::Uuid::parse_str(raw)
        .map(StableId)
        .unwrap_or_else(|_| StableId(uuid::Uuid::nil()))
}

// ---------------------------------------------------------------------------
// Generation
// ---------------------------------------------------------------------------

/// One cancellable generation.
///
/// The Swift pipeline ran the engine on a detached task and forwarded the
/// caller's cancellation into it with `withTaskCancellationHandler`, because
/// `Task.detached` alone swallows it and the `Task.isCancelled` checks between
/// ladder stages would never trip. This object is that forwarding path across
/// the boundary: `onCancel` calls [`Self::cancel`], the engine sees it at its
/// next stage boundary, and the run returns an empty list — exactly what the
/// Swift returned from a cancelled generation.
///
/// **One generator per run.** `cancel` is sticky by design: a generator that
/// reset its flag on `generate` would lose a cancellation that landed in the
/// window between the two, which is precisely the race an expiring background
/// task creates.
#[derive(uniffi::Object)]
pub struct MemoryGenerator {
    cancelled: AtomicBool,
}

impl Default for MemoryGenerator {
    fn default() -> Self {
        MemoryGenerator {
            cancelled: AtomicBool::new(false),
        }
    }
}

#[uniffi::export]
impl MemoryGenerator {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(MemoryGenerator::default())
    }

    /// Run the ladder. Returns the selected top-10, or an empty list if the run
    /// was cancelled.
    pub fn generate(&self, inputs: MemoryGenerationInputs) -> Vec<MemoryRecord> {
        let inputs = inputs.into_engine_inputs();
        let memories = generate_cancellable(&inputs, &|| self.cancelled.load(Ordering::Acquire));
        memories.iter().map(MemoryRecord::of).collect()
    }

    /// Ask the in-flight run to stop at its next stage boundary. Sticky.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// `MemoryEngine.generate`, uncancellable — for callers that do not have a
/// cancellation to forward (tests, and the conformance harness).
#[uniffi::export]
pub fn generate_memories(inputs: MemoryGenerationInputs) -> Vec<MemoryRecord> {
    gallery_memories::generate(&inputs.into_engine_inputs())
        .iter()
        .map(MemoryRecord::of)
        .collect()
}

/// `GalleryStore.computeScheduledMemories` — the widget's pre-published
/// horizon, offsets `1..=horizon_days` from local midnight.
///
/// `hidden_memory_ids` is `MemoryCoordinator.hiddenMemories`, which stays in
/// Swift; it is passed separately because it is coordinator state rather than
/// engine input, and `generate` does not read it at all.
#[uniffi::export]
pub fn compute_scheduled_memories(
    inputs: MemoryGenerationInputs,
    horizon_days: i64,
    hidden_memory_ids: Vec<String>,
) -> Vec<ScheduledMemoryRecord> {
    let hidden: HashSet<String> = hidden_memory_ids.into_iter().collect();
    compute_scheduled(&inputs.into_engine_inputs(), horizon_days, &hidden)
        .iter()
        .map(scheduled_record)
        .collect()
}

fn scheduled_record(s: &gallery_memories::ScheduledMemory) -> ScheduledMemoryRecord {
    ScheduledMemoryRecord {
        memory: MemoryRecord::of(&s.memory),
        valid_from: s.valid_from.0,
        valid_to: s.valid_to.0,
    }
}

/// How far ahead calendar-tied memories are pre-published.
#[uniffi::export]
pub fn scheduled_memory_horizon_days() -> i64 {
    SCHEDULED_MEMORY_HORIZON_DAYS
}

/// Cluster identity for the selection stage: a trip parent and its sub-trips
/// collapse to one key, every other memory id is its own cluster. The
/// coordinator keys its cool-down map by this.
#[uniffi::export]
pub fn memory_cluster_key(memory_id: String) -> String {
    cluster_key(&memory_id)
}

/// The localized country name for an ISO 3166-1 alpha-2 code, or `None` when
/// the code is unknown. `Locale.current.localizedString(forRegionCode:)`'s
/// replacement — see `gallery_memories::locale` for the (en_US) table and why
/// it is a table.
#[uniffi::export]
pub fn memory_country_name(code: String) -> Option<String> {
    gallery_memories::locale::country_name(&code).map(str::to_string)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::ScanTag;

    fn photo(path: &str, date: Option<f64>, tags: &[&str]) -> ScanPhoto {
        ScanPhoto {
            id: StableId::for_photo(path).to_string(),
            path: path.to_string(),
            filename: path
                .rsplit('/')
                .next()
                .unwrap()
                .rsplit_once('.')
                .map(|(a, _)| a.to_string())
                .unwrap_or_default(),
            file_size: 1,
            date_taken: date,
            date_from_metadata: true,
            is_video: false,
            live_photo_video_path: None,
            hierarchical_tags: tags
                .iter()
                .map(|t| {
                    let tag = HierarchicalTag::new(t);
                    ScanTag {
                        full_path: tag.full_path,
                        namespace: tag.namespace,
                        display_name: tag.display_name,
                    }
                })
                .collect(),
            country_code: None,
            enriched_file_date: None,
            file_modification_date: None,
            gps_latitude: None,
            gps_longitude: None,
            face_regions: Vec::new(),
        }
    }

    fn empty_inputs(now: f64) -> MemoryGenerationInputs {
        MemoryGenerationInputs {
            photos: Vec::new(),
            photo_time_zone_offsets: Vec::new(),
            leaf_folders: Vec::new(),
            contacts: Vec::new(),
            person_contact_links: Vec::new(),
            birthdays_enabled: true,
            me_person_path: String::new(),
            hidden_people: Vec::new(),
            now,
            time_zone_offset_seconds: 0,
            horizon_offset_seconds: Vec::new(),
            seed: "seed".to_string(),
            seen_memory_ids: Vec::new(),
            surfaced_clusters: Vec::new(),
        }
    }

    /// 2019-06-11 12:00 UTC + i minutes, in reference-date seconds.
    fn on_this_day_library(count: usize) -> Vec<ScanPhoto> {
        let base = 581_947_200.0; // 2019-06-11T12:00:00Z
        (0..count)
            .map(|i| {
                photo(
                    &format!("/lib/otd-{i}.jpg"),
                    Some(base + (i as f64) * 120.0),
                    &[],
                )
            })
            .collect()
    }

    #[test]
    fn build_returns_the_sorted_order_and_the_tag_lists() {
        let index = LibraryIndex::default();
        let summary = index.build(vec![
            photo("/lib/b.jpg", Some(200.0), &["Places/Italy/Lazio/Rome"]),
            photo("/lib/a.jpg", Some(300.0), &["People/Alice"]),
            photo("/lib/c.jpg", None, &[]),
        ]);
        assert_eq!(summary.sorted_photo_ids.len(), 3);
        assert_eq!(
            summary.sorted_photo_ids[0],
            StableId::for_photo("/lib/a.jpg").to_string(),
            "the newest photo leads the grid"
        );
        assert_eq!(
            summary.sorted_photo_ids[2],
            StableId::for_photo("/lib/c.jpg").to_string(),
            "the undated photo closes it"
        );
        // The virtual prefix buckets are in the aggregated list, which is what
        // makes `places/italy` a queryable tag rather than a substring.
        let paths: Vec<&str> = summary.tags.iter().map(|t| t.id.as_str()).collect();
        assert!(paths.contains(&"places/italy"));
        assert!(paths.contains(&"places/italy/lazio"));
        assert_eq!(summary.people.len(), 1);
        assert_eq!(summary.people[0].full_path, "People/Alice");
        assert_eq!(index.photo_count(), 3);
    }

    #[test]
    fn photo_windows_refuse_stale_generations() {
        let index = LibraryIndex::default();
        index.build(vec![
            photo("/lib/a.jpg", Some(300.0), &[]),
            photo("/lib/b.jpg", Some(200.0), &[]),
        ]);
        let first = index.photo_structure();
        assert_eq!(first.sections[0].item_ids.len(), 2);
        index.build(vec![photo("/lib/new.jpg", Some(400.0), &[])]);

        assert!(matches!(
            index.photo_window("photos".into(), 0, 20, first.generation),
            Err(ViewError::StaleGeneration {
                requested,
                current,
                ..
            }) if requested == first.generation && current > requested
        ));
    }

    #[test]
    fn photo_and_tag_windows_are_bounded_before_rows_are_allocated() {
        let index = LibraryIndex::default();
        index.build(
            (0..20_000)
                .map(|i| {
                    photo(
                        &format!("/lib/{i:05}.jpg"),
                        Some(i as f64),
                        &["Scenes/Beach"],
                    )
                })
                .collect(),
        );
        let structure = index.photo_structure();
        assert_eq!(structure.sections[0].item_ids.len(), 20_000);
        let rows = index
            .photo_window("photos".into(), 19_900, 100, structure.generation)
            .unwrap();
        assert_eq!(rows.len(), 100);
        assert!(matches!(
            index.photo_window(
                "photos".into(),
                0,
                crate::view::MAX_VIEW_WINDOW as u64 + 1,
                structure.generation
            ),
            Err(ViewError::WindowTooLarge { .. })
        ));

        let tags = index.tag_structure();
        let tag_rows = index
            .tag_window("tags".into(), 0, 10, tags.generation)
            .unwrap();
        assert_eq!(tag_rows.len(), 1, "one indexed leaf tag");
        assert!(tag_rows.iter().all(|row| row.trailing.is_some()));
    }

    #[test]
    fn photo_structure_groups_ids_without_putting_content_in_structure() {
        let index = LibraryIndex::default();
        index.build_with_time_zone_offsets(
            vec![
                photo("/lib/january.jpg", Some(0.0), &[]),
                photo("/lib/february.jpg", Some(32.0 * 86_400.0), &[]),
                photo("/lib/unknown.jpg", None, &[]),
            ],
            vec![0; 3],
        );
        let structure = index.photo_structure();
        assert_eq!(
            structure
                .sections
                .iter()
                .map(|section| section.title.as_str())
                .collect::<Vec<_>>(),
            ["February 2001", "January 2001", "Unknown Date"]
        );
        assert!(structure
            .sections
            .iter()
            .all(|section| section.item_ids.len() == 1));
        let february = index
            .photo_window(structure.sections[0].id.clone(), 0, 1, structure.generation)
            .unwrap();
        assert_eq!(february[0].accessibility_label.as_deref(), Some("february"));
    }

    #[test]
    fn changing_filter_intent_invalidates_an_old_window() {
        let index = LibraryIndex::default();
        index.build(vec![
            photo("/lib/rome.jpg", Some(300.0), &["Places/Italy/Rome"]),
            photo("/lib/paris.jpg", Some(200.0), &["Places/France/Paris"]),
        ]);
        let all = index.set_photo_view(String::new(), Vec::new());
        let rome = index.set_photo_view("rome".into(), Vec::new());
        assert_eq!(rome.sections[0].item_ids.len(), 1);
        assert!(matches!(
            index.photo_window("photos".into(), 0, 20, all.generation),
            Err(ViewError::StaleGeneration { .. })
        ));
        assert_eq!(
            index
                .photo_window("photos".into(), 0, 20, rome.generation)
                .unwrap()[0]
                .accessibility_label
                .as_deref(),
            Some("rome")
        );

        let paris_id = all.sections[0].item_ids[1].clone();
        let drill_in = index.set_photo_ids_view(
            "memory-1".into(),
            vec!["not-a-uuid".into(), paris_id.clone()],
            String::new(),
            Vec::new(),
        );
        assert_eq!(drill_in.sections[0].item_ids, vec![paris_id]);
        assert!(matches!(
            index.photo_window("photos".into(), 0, 20, rome.generation),
            Err(ViewError::StaleGeneration { .. })
        ));
        assert_eq!(
            index
                .photo_window("photos".into(), 0, 20, drill_in.generation)
                .unwrap()[0]
                .accessibility_label
                .as_deref(),
            Some("paris")
        );
    }

    /// The failure this API shape exists to make impossible: search must take
    /// the tag branch for a virtual prefix path, which needs the aggregated
    /// list. The Swift signature let a caller omit it.
    #[test]
    fn a_virtual_prefix_tag_query_filters_by_tag_not_by_substring() {
        let index = LibraryIndex::default();
        index.build(vec![
            photo("/lib/rome.jpg", Some(300.0), &["Places/Italy/Lazio/Rome"]),
            photo("/lib/paris.jpg", Some(200.0), &["Places/France/Paris"]),
            // Carries the words but not the tag: a substring fallback would
            // sweep it in.
            photo("/lib/places italy lazio.jpg", Some(100.0), &[]),
        ]);
        let hits = index.search("places/italy/lazio".to_string(), Vec::new());
        assert_eq!(hits, vec![StableId::for_photo("/lib/rome.jpg").to_string()]);
    }

    #[test]
    fn required_tags_and_together_and_places_expands_by_prefix() {
        let index = LibraryIndex::default();
        index.build(vec![
            photo(
                "/lib/beach.jpg",
                Some(300.0),
                &["Places/Italy/Lazio/Rome", "Scenes/Beach"],
            ),
            photo("/lib/rome2.jpg", Some(200.0), &["Places/Italy/Lazio/Rome"]),
            photo("/lib/paris.jpg", Some(100.0), &["Places/France/Paris"]),
        ]);
        let both = index.search(
            String::new(),
            vec!["Places/Italy".to_string(), "Scenes/Beach".to_string()],
        );
        assert_eq!(
            both,
            vec![StableId::for_photo("/lib/beach.jpg").to_string()]
        );
        let italy = index.search(String::new(), vec!["Places/Italy".to_string()]);
        assert_eq!(italy.len(), 2, "prefix expansion reaches the nested leaf");
    }

    #[test]
    fn photo_ids_for_tag_includes_the_prefix_expansion() {
        let index = LibraryIndex::default();
        index.build(vec![
            photo("/lib/rome.jpg", Some(300.0), &["Places/Italy/Lazio/Rome"]),
            photo(
                "/lib/milan.jpg",
                Some(200.0),
                &["Places/Italy/Lombardy/Milan"],
            ),
        ]);
        assert_eq!(index.photo_ids_for_tag("Places/Italy".to_string()).len(), 2);
        assert_eq!(
            index
                .photo_ids_for_tag("Places/Italy/Lazio".to_string())
                .len(),
            1
        );
        assert!(index
            .photo_ids_for_tag("Places/France".to_string())
            .is_empty());
    }

    /// A rebuild replaces the table wholesale — the contract `build(allPhotos:)`
    /// always had, and the reason the app never needs a "remove photo" call.
    #[test]
    fn rebuilding_replaces_the_previous_table() {
        let index = LibraryIndex::default();
        index.build(on_this_day_library(3));
        assert_eq!(index.photo_count(), 3);
        index.build(vec![photo("/lib/only.jpg", Some(1.0), &[])]);
        assert_eq!(index.photo_count(), 1);
        assert_eq!(index.sorted_photo_ids().len(), 1);
    }

    #[test]
    fn generate_produces_the_calendar_memory_for_the_day() {
        let mut inputs = empty_inputs(739_800_000.0); // 2024-06-11T12:00:00Z
        inputs.photos = on_this_day_library(12);
        let memories = generate_memories(inputs);
        assert_eq!(memories.len(), 2, "onThisDay + yearsAgo-5");
        assert_eq!(memories[0].id, "onThisDay-2024-06-11");
        assert_eq!(memories[0].kind, MemoryKind::OnThisDay);
        assert_eq!(memories[0].photo_ids.len(), 12);
        assert!(memories[0].photo_ids.contains(&memories[0].cover_photo_id));
        assert_eq!(
            memories[0].subtitle.as_deref(),
            Some("Jun 11, 2019 · 12 photos")
        );
    }

    /// The forwarding path itself: a generator cancelled before it runs returns
    /// nothing, which is what the Swift `Task.isCancelled` checks produced.
    #[test]
    fn a_cancelled_generator_returns_nothing() {
        let mut inputs = empty_inputs(739_800_000.0);
        inputs.photos = on_this_day_library(12);
        let generator = MemoryGenerator::default();
        generator.cancel();
        assert!(generator.is_cancelled());
        assert!(generator.generate(inputs).is_empty());
    }

    #[test]
    fn an_uncancelled_generator_matches_the_free_function() {
        let mut inputs = empty_inputs(739_800_000.0);
        inputs.photos = on_this_day_library(12);
        let generator = MemoryGenerator::default();
        assert_eq!(
            generator.generate(inputs.clone()),
            generate_memories(inputs)
        );
    }

    #[test]
    fn scheduled_memories_cover_the_horizon_and_skip_today() {
        let mut inputs = empty_inputs(739_540_800.0); // 2024-06-08T12:00:00Z
        inputs.photos = on_this_day_library(12);
        let scheduled = compute_scheduled_memories(inputs, 7, Vec::new());
        assert!(!scheduled.is_empty());
        for item in &scheduled {
            assert!(item.valid_from < item.valid_to);
            assert!(
                item.valid_from >= 739_584_000.0,
                "day 0 (2024-06-08) must not be pre-published"
            );
        }
        // 2024-06-11 is offset +3 and is the only populated day.
        assert!(scheduled
            .iter()
            .any(|s| s.memory.id == "onThisDay-2024-06-11"));
    }

    #[test]
    fn scheduled_horizon_reuses_the_library_generation() {
        let photos = on_this_day_library(12);
        let index = LibraryIndex::default();
        index.build_with_time_zone_offsets(photos.clone(), vec![0; photos.len()]);
        let context = ScheduledMemoryContext {
            leaf_folders: Vec::new(),
            contacts: Vec::new(),
            person_contact_links: Vec::new(),
            birthdays_enabled: true,
            me_person_path: String::new(),
            hidden_people: Vec::new(),
            now: 739_540_800.0,
            time_zone_offset_seconds: 0,
            horizon_offset_seconds: Vec::new(),
            seed: String::new(),
            seen_memory_ids: Vec::new(),
            surfaced_clusters: Vec::new(),
        };
        let reused = index.compute_scheduled(context, 7, Vec::new());

        let mut legacy = empty_inputs(739_540_800.0);
        legacy.photos = photos;
        legacy.photo_time_zone_offsets = vec![0; 12];
        assert_eq!(reused, compute_scheduled_memories(legacy, 7, Vec::new()));
        assert_eq!(index.scheduled_photo_count(), 12);
    }

    #[test]
    fn a_hidden_memory_is_not_pre_published() {
        let mut inputs = empty_inputs(739_540_800.0);
        inputs.photos = on_this_day_library(12);
        let all = compute_scheduled_memories(inputs.clone(), 7, Vec::new());
        let hidden =
            compute_scheduled_memories(inputs, 7, vec!["onThisDay-2024-06-11".to_string()]);
        assert_eq!(hidden.len(), all.len() - 1);
        assert!(!hidden.iter().any(|s| s.memory.id == "onThisDay-2024-06-11"));
    }

    /// the local-day memory-id rule's exit criterion across the boundary: what the widget
    /// pre-publishes for a day is what the rail generates when that day
    /// arrives, in a zone ahead of GMT. Tokyo's horizon used to name the
    /// previous GMT day and the deep link resolved to nothing.
    #[test]
    fn a_tokyo_horizon_pre_publishes_the_ids_the_live_run_produces() {
        let jst = 9 * 3600;
        let mut inputs = empty_inputs(739_508_400.0); // 2024-06-08T03:00:00Z, 12:00 JST
        inputs.photos = on_this_day_library(12); // 2019-06-11T12:00Z, 21:00 JST
        inputs.time_zone_offset_seconds = jst;
        let scheduled = compute_scheduled_memories(inputs.clone(), 7, Vec::new());
        assert!(scheduled
            .iter()
            .any(|s| s.memory.id == "onThisDay-2024-06-11"));

        // Noon JST on the day it is about.
        let mut live = inputs;
        live.now = 739_767_600.0; // 2024-06-11T03:00:00Z
        assert!(generate_memories(live)
            .iter()
            .any(|m| m.id == "onThisDay-2024-06-11"));
    }

    /// The per-day offset table is the horizon's half of the DST story: a
    /// window opens at the midnight of **its own** day, not at the run's. The
    /// offsets are synthetic — a zone that loses an hour three days out — since
    /// the point is that the table is read at all.
    #[test]
    fn the_horizon_offset_table_moves_a_window_and_an_empty_one_does_not() {
        let plus_two = 2 * 3600;
        let plus_one = 3600;
        let mut inputs = empty_inputs(739_533_600.0); // 2024-06-08T10:00:00Z, 12:00 local
        inputs.photos = on_this_day_library(12);
        inputs.time_zone_offset_seconds = plus_two;
        let single = compute_scheduled_memories(inputs.clone(), 7, Vec::new());

        inputs.horizon_offset_seconds = vec![plus_two; 3]
            .into_iter()
            .chain(std::iter::repeat_n(plus_one, 6))
            .collect();
        let per_day = compute_scheduled_memories(inputs, 7, Vec::new());

        let opens = |items: &[ScheduledMemoryRecord]| {
            items
                .iter()
                .find(|s| s.memory.id == "onThisDay-2024-06-11")
                .map(|s| s.valid_from)
        };
        assert_eq!(opens(&single), Some(739_749_600.0)); // 2024-06-10T22:00:00Z, +2
        assert_eq!(opens(&per_day), Some(739_753_200.0)); // 2024-06-10T23:00:00Z, +1
    }

    #[test]
    fn the_time_zone_offset_moves_the_day() {
        // 2024-06-11T15:30Z is 2024-06-12 00:30 in Tokyo, so "today" is the
        // 12th there and the 11th in UTC.
        let mut utc = empty_inputs(739_812_600.0);
        utc.photos = on_this_day_library(12);
        let mut tokyo = utc.clone();
        tokyo.time_zone_offset_seconds = 9 * 3600;
        assert!(generate_memories(utc)
            .iter()
            .any(|m| m.id == "onThisDay-2024-06-11"));
        assert!(
            !generate_memories(tokyo)
                .iter()
                .any(|m| m.id == "onThisDay-2024-06-11"),
            "the June-11 photos are not on Tokyo's June 12"
        );
    }

    #[test]
    fn cluster_keys_collapse_subtrips_onto_their_parent() {
        assert_eq!(
            memory_cluster_key("subtrip-2023-5-1-italy".to_string()),
            "trip-2023-5-1"
        );
        assert_eq!(
            memory_cluster_key("onThisDay-2024-06-11".to_string()),
            "onThisDay-2024-06-11"
        );
    }

    #[test]
    fn the_horizon_constant_is_the_one_the_engine_uses() {
        assert_eq!(scheduled_memory_horizon_days(), 7);
    }

    #[test]
    fn country_names_resolve_and_unknown_codes_do_not() {
        assert_eq!(
            memory_country_name("AR".to_string()).as_deref(),
            Some("Argentina")
        );
        assert_eq!(memory_country_name("ZZ".to_string()), None);
    }

    /// A person link with no contact id is `.disabled`, and it suppresses the
    /// birthday memory the name auto-match would otherwise produce.
    #[test]
    fn a_disabled_person_link_suppresses_the_birthday() {
        let mut inputs = empty_inputs(739_800_000.0); // 2024-06-11
        inputs.photos = (0..3)
            .map(|i| {
                photo(
                    &format!("/lib/alice-{i}.jpg"),
                    Some(633_866_400.0 + i as f64 * 86_400.0),
                    &["People/Alice Anderson"],
                )
            })
            .collect();
        inputs.contacts = vec![MemoryContact {
            id: "c-alice".to_string(),
            given_name: "Alice".to_string(),
            family_name: "Anderson".to_string(),
            birthday_month: Some(6),
            birthday_day: Some(11),
        }];
        assert!(generate_memories(inputs.clone())
            .iter()
            .any(|m| m.kind == MemoryKind::Birthday));

        inputs.person_contact_links = vec![MemoryPersonLink {
            person_path: "People/Alice Anderson".to_string(),
            contact_id: None,
        }];
        assert!(!generate_memories(inputs)
            .iter()
            .any(|m| m.kind == MemoryKind::Birthday));
    }
}
