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

use gallery_index::{collection_groups, leaf_tags, LibraryIndex as CoreIndex, TagSuggestion};
use gallery_memories::{
    cluster_key, compute_scheduled, generate_cancellable, Contact, GenerationInputs, LeafFolder,
    Memory, MemoryType, PersonLink, UtcOffset, SCHEDULED_MEMORY_HORIZON_DAYS,
};
use gallery_model::{AppleDate, CivilDateTime, HierarchicalTag, PhotoFile, StableId};

use crate::locations::{
    collection_section_id, collection_text_row, folder_text_row, people_text_row,
    photo_count_label, FolderTable,
};
use crate::people::{page_people, rail_people};
use crate::person_log::PersonStateStructure;
use crate::scanner::{
    photo_from_record, photo_to_record, ScanPhoto, ScannedFolderHost, ScannedMediaHost,
};
use crate::view::{
    checked_window, GalleryMediaItem, GalleryTextRow, ViewAction, ViewContentState, ViewError,
    ViewSection, ViewSlotKind, ViewStructure,
};

// ---------------------------------------------------------------------------
// Shared wire records
// ---------------------------------------------------------------------------

/// `TagSuggestion.swift` — one tag bucket, flattened for the UI.
///
/// R6 role: structure DTO.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TagStructureItem {
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

impl TagStructureItem {
    pub(crate) fn of(s: &TagSuggestion) -> Self {
        TagStructureItem {
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
///
/// R6 role: structure DTO.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LibraryBuildStructure {
    /// Photo ids, date descending with the `url.path` tiebreak. **This order is
    /// the grid.**
    pub sorted_photo_ids: Vec<String>,
    /// One suggestion per tag bucket, `(count desc, id asc)`.
    pub tags: Vec<TagStructureItem>,
    /// The `People/…` subset, each carrying its most recent photo date.
    pub people: Vec<TagStructureItem>,
    /// Time spent inside the core, for the `Built:` log line the performance
    /// gates are read from.
    pub build_millis: u64,
}

/// What one [`LibraryIndex::remove_photos`] produced.
///
/// Structure + generation only (ADR 0003). The host already has the photo
/// records; this names which ids left the table and the new window guard.
///
/// R6 role: structure DTO.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RemovePhotosResult {
    /// Canonical ids that were in the table and are now gone.
    pub removed_ids: Vec<String>,
    /// Window guard after the rewrite. Old `photo_window` calls are stale.
    pub generation: u64,
    /// Photos left in the table.
    pub photo_count: u32,
    /// Current Photos-screen projection with the dropped ids filtered out.
    pub visible_photo_ids: Vec<String>,
    /// True when the table is empty after the call.
    pub empty: bool,
}

// ---------------------------------------------------------------------------
// LibraryIndex
// ---------------------------------------------------------------------------

/// The photo table and every index over it: the sorted order, the search
/// corpus, and the tag buckets.
///
/// One instance per Store. `build` replaces the table wholesale after a scan
/// or snapshot load — the same contract `SearchIndex.build(allPhotos:)` and
/// `TagIndex.build(allPhotos:)` had. [`Self::remove_photos`] is the delete
/// path: drop ids, `CoreIndex::build` the remaining table, rewrite folder
/// slices. Sparse in-place patches are still not part of the contract.
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
    /// Hidden-filtered, featured-first page list.
    visible_people: Vec<TagSuggestion>,
    /// Recency-gated rail (cap 20).
    rail_people: Vec<TagSuggestion>,
    person_state: PersonStateStructure,
    /// Apple reference seconds; rail recency uses this.
    person_now: f64,
    /// Generation of every id list derived from this table.
    generation: u64,
    /// The current Photos-screen projection. Structure returns these ids;
    /// content windows index the same vector after checking `generation`.
    visible_photo_ids: Vec<StableId>,
    visible_sections: Vec<PhotoViewSection>,
    /// Search/filter intent that produced `visible_photo_ids`.
    visible_key: String,
    /// Scanner folders plus the photo-id order their slices address.
    folders: FolderTable,
    /// Parent last handed to [`LibraryIndex::folder_structure`]. `None` is the
    /// Folders-tab root listing (children of the scan root).
    visible_folder_parent: Option<String>,
    /// Child folder ids for `visible_folder_parent`.
    visible_folder_ids: Vec<String>,
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
                visible_people: Vec::new(),
                rail_people: Vec::new(),
                person_state: PersonStateStructure::default(),
                person_now: 0.0,
                generation: 0,
                visible_photo_ids: Vec::new(),
                visible_sections: Vec::new(),
                visible_key: String::new(),
                folders: FolderTable::empty(),
                visible_folder_parent: None,
                visible_folder_ids: Vec::new(),
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
    pub fn build(&self, photos: Vec<crate::scanner::ScannedMediaHost>) -> LibraryBuildStructure {
        self.rebuild(photos, Vec::new())
    }

    /// Rebuild and retain the platform's per-photo UTC offsets.
    ///
    /// The offsets are supplied on the same once-per-library crossing as the
    /// photos. Scheduled memories then reuse both tables instead of rebuilding
    /// and re-marshalling a second 20,000-photo generation snapshot.
    pub fn build_with_time_zone_offsets(
        &self,
        photos: Vec<crate::scanner::ScannedMediaHost>,
        photo_time_zone_offsets: Vec<i32>,
    ) -> LibraryBuildStructure {
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
    ) -> Vec<ScheduledMemoryStructure> {
        let hidden: HashSet<String> = hidden_memory_ids.into_iter().collect();
        let inputs = {
            let guard = read(&self.inner);
            scheduled_inputs(&guard, context)
        };
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
        let _span =
            localcore_trace::span_always("catalog", "index.rebuild").extra("photos", photos.len());
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
        refresh_person_lists(&mut guard);
        guard.folders = FolderTable::empty();
        guard.visible_folder_parent = None;
        guard.visible_folder_ids = Vec::new();
        guard.photo_time_zone_offsets = photo_time_zone_offsets;
        summary
    }

    /// Apply Photos-screen search/tag intent and return cheap structure.
    ///
    /// The whole ordered id list is structure by ADR 0003 R4. The records
    /// behind those ids are fetched only through [`Self::photo_window`].
    pub fn set_photo_view(&self, query: String, required_tag_paths: Vec<String>) -> ViewStructure {
        let _span = localcore_trace::span_always("photos", "set_photo_view")
            .extra("q_len", query.len())
            .extra("tags_n", required_tag_paths.len());
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
        let _span = localcore_trace::span_always("photos", "set_photo_ids_view")
            .extra("n", photo_ids.len());
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
        let _span = localcore_trace::span("photos", "photo_structure");
        photo_structure(&read(&self.inner))
    }

    /// Current photo projection in list order.
    ///
    /// Cheaper than [`Self::photo_structure`]: no month sections, one id list.
    pub fn visible_photo_ids(&self) -> Vec<String> {
        read(&self.inner)
            .visible_photo_ids
            .iter()
            .map(ids)
            .collect()
    }

    /// Display-ready photos for one visible window.
    pub fn photo_window(
        &self,
        section_id: String,
        offset: u64,
        limit: u64,
        generation: u64,
    ) -> Result<Vec<GalleryMediaItem>, ViewError> {
        let _span = localcore_trace::span("photos", "photo_window")
            .extra("off", offset)
            .extra("lim", limit);
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

    /// Install the scanner's flat folder list and the photo-id order those
    /// slices address.
    ///
    /// Extending [`Self::build`] would break existing `build(photos)` callers,
    /// so folders arrive on this second call after a scan or snapshot load.
    /// The recursive `PhotoFolder` tree is not stored and never returned.
    ///
    /// Both shells attach folders here after `build` so
    /// [`Self::remove_photos`] can rewrite slices. Folder *windows*
    /// (`folder_structure` / `folder_window`) stay GTK-only until
    /// Phase 5.8; iOS still walks its own `PhotoFolder` tree for UI.
    pub fn set_folders(
        &self,
        folders: Vec<ScannedFolderHost>,
        photo_ids_in_scan_order: Vec<String>,
    ) {
        let _span = localcore_trace::span_always("folders", "set_folders")
            .extra("folders", folders.len())
            .extra("ids", photo_ids_in_scan_order.len());
        let table = FolderTable::new(folders, photo_ids_in_scan_order);
        let visible_folder_ids = table.listing_ids(None);
        let mut guard = write(&self.inner);
        guard.generation = guard.generation.saturating_add(1);
        guard.folders = table;
        guard.visible_folder_parent = None;
        guard.visible_folder_ids = visible_folder_ids;
    }

    /// Folder listing for one parent. `None` is children of the scan root; a
    /// single library root is not shown as a row.
    ///
    /// Section id is `folders`, slot kind is a text row, item ids are child
    /// folder ids. Changing `parent_id` bumps [`Self::view_generation`] so a
    /// window cannot join rows to an older listing.
    ///
    /// iOS folder UI still walks `PhotoFolder` (Phase 5.8).
    pub fn folder_structure(&self, parent_id: Option<String>) -> ViewStructure {
        let _span = localcore_trace::span("folders", "folder_structure");
        let mut guard = write(&self.inner);
        if guard.visible_folder_parent != parent_id {
            guard.visible_folder_ids = guard.folders.listing_ids(parent_id.as_deref());
            guard.visible_folder_parent = parent_id;
            guard.generation = guard.generation.saturating_add(1);
        }
        folder_structure_of(&guard)
    }

    /// Display-ready folder rows for the listing last returned by
    /// [`Self::folder_structure`].
    ///
    /// Title is the folder name. Trailing is the recursive photo count
    /// (`total_photo_count`) as `"1 photo"` / `"N photos"`. Subtitle is the
    /// path leaf when it differs from the name.
    ///
    /// iOS folder UI still walks `PhotoFolder` (Phase 5.8).
    pub fn folder_window(
        &self,
        section_id: String,
        offset: u64,
        limit: u64,
        generation: u64,
    ) -> Result<Vec<GalleryTextRow>, ViewError> {
        let _span = localcore_trace::span("folders", "folder_window")
            .extra("off", offset)
            .extra("lim", limit);
        let guard = read(&self.inner);
        // Generation wins over section lookup: every section id belongs to
        // the requested structure, so an absent old id is stale, not unknown.
        checked_window(generation, guard.generation, 0, 0, 0)?;
        if section_id != "folders" {
            return Err(ViewError::SectionNotFound {
                section_id,
                message: "That Gallery folder section no longer exists.".into(),
                user_actionable: true,
            });
        }
        let range = checked_window(
            generation,
            guard.generation,
            offset,
            limit,
            guard.visible_folder_ids.len(),
        )?;
        Ok(guard.visible_folder_ids[range]
            .iter()
            .filter_map(|id| guard.folders.get(id).map(folder_text_row))
            .collect())
    }

    /// This folder's own photos (the scan slice), so a shell can
    /// [`Self::set_photo_ids_view`]. Recursive totals stay on the text-row
    /// trailing from `total_photo_count`.
    ///
    /// iOS folder UI still walks `PhotoFolder` (Phase 5.8).
    pub fn folder_photo_ids(&self, folder_id: String) -> Vec<String> {
        let _span = localcore_trace::span_always("folders", "folder_photo_ids");
        let ids = read(&self.inner).folders.own_photo_ids(&folder_id);
        localcore_trace::event("folders", format!("folder_photo_ids n={}", ids.len()));
        ids
    }

    /// See-all people listing. Item ids are canonical `People/…` paths.
    ///
    /// Featured float to the front after [`Self::set_person_state`]; hidden
    /// people are appended at the end. The Collections rail uses
    /// [`Self::people_rail_structure`]. Person photos already work via
    /// [`Self::photo_ids_for_tag`] plus [`Self::set_photo_ids_view`].
    pub fn people_structure(&self) -> ViewStructure {
        let _span = localcore_trace::span("catalog", "people_structure");
        let guard = read(&self.inner);
        let ids = guard
            .visible_people
            .iter()
            .map(|person| person.full_path.clone())
            .collect();
        localcore_trace::event(
            "catalog",
            format!("people_structure n={}", guard.visible_people.len()),
        );
        ViewStructure {
            state: content_state(guard.visible_people.len()),
            sections: vec![ViewSection {
                id: "people".into(),
                title: "People".into(),
                slot_kind: ViewSlotKind::TextRow,
                item_ids: ids,
            }],
            actions: Vec::new(),
            generation: guard.generation,
        }
    }

    /// Display-ready people rows (`display_name`, trailing count).
    pub fn people_window(
        &self,
        section_id: String,
        offset: u64,
        limit: u64,
        generation: u64,
    ) -> Result<Vec<GalleryTextRow>, ViewError> {
        let guard = read(&self.inner);
        checked_window(generation, guard.generation, 0, 0, 0)?;
        if section_id != "people" {
            return Err(ViewError::SectionNotFound {
                section_id,
                message: "That Gallery people section no longer exists.".into(),
                user_actionable: true,
            });
        }
        let range = checked_window(
            generation,
            guard.generation,
            offset,
            limit,
            guard.visible_people.len(),
        )?;
        Ok(guard.visible_people[range]
            .iter()
            .map(people_text_row)
            .collect())
    }

    /// Collections-rail people (featured-first, recency gate).
    pub fn people_rail_structure(&self) -> ViewStructure {
        let guard = read(&self.inner);
        let ids = guard
            .rail_people
            .iter()
            .map(|person| person.full_path.clone())
            .collect();
        ViewStructure {
            state: content_state(guard.rail_people.len()),
            sections: vec![ViewSection {
                id: "people".into(),
                title: "People".into(),
                slot_kind: ViewSlotKind::TextRow,
                item_ids: ids,
            }],
            actions: Vec::new(),
            generation: guard.generation,
        }
    }

    /// Window over [`Self::people_rail_structure`].
    pub fn people_rail_window(
        &self,
        section_id: String,
        offset: u64,
        limit: u64,
        generation: u64,
    ) -> Result<Vec<GalleryTextRow>, ViewError> {
        let guard = read(&self.inner);
        checked_window(generation, guard.generation, 0, 0, 0)?;
        if section_id != "people" {
            return Err(ViewError::SectionNotFound {
                section_id,
                message: "That Gallery people section no longer exists.".into(),
                user_actionable: true,
            });
        }
        let range = checked_window(
            generation,
            guard.generation,
            offset,
            limit,
            guard.rail_people.len(),
        )?;
        Ok(guard.rail_people[range]
            .iter()
            .map(people_text_row)
            .collect())
    }

    /// Project `.gallery/log` onto the people lists. Bumps generation.
    /// Both shells call this after attach and after every person mutation.
    pub fn set_person_state(&self, state: PersonStateStructure, now: f64) {
        let mut guard = write(&self.inner);
        guard.person_state = state;
        guard.person_now = now;
        refresh_person_lists(&mut guard);
        guard.generation = guard.generation.saturating_add(1);
    }

    /// Last [`Self::set_person_state`] projection (empty before attach).
    pub fn person_state(&self) -> PersonStateStructure {
        read(&self.inner).person_state.clone()
    }

    /// Canonical `People/…` path for a row id or any spelling of the path.
    pub fn person_full_path(&self, id_or_path: String) -> Option<String> {
        let key = gallery_index::text::match_key(&id_or_path);
        read(&self.inner)
            .people
            .iter()
            .find(|person| {
                gallery_index::text::match_key(&person.id) == key
                    || gallery_index::text::match_key(&person.full_path) == key
            })
            .map(|person| person.full_path.clone())
    }

    /// Cover photo id from the person log, if the user set one.
    pub fn featured_photo_id(&self, person_path: String) -> Option<String> {
        let key = gallery_index::text::nfc(&person_path);
        read(&self.inner)
            .person_state
            .featured_photo
            .iter()
            .find(|pair| gallery_index::text::nfc(&pair.path) == key)
            .map(|pair| pair.value.clone())
    }

    /// Collections hub for non-People tag namespaces (Objects, Scenes, Places,
    /// Albums, Events, Other — as present). Item ids are tag ids.
    ///
    /// People are not a collection section; use [`Self::people_structure`].
    /// Memories section lands in 5.6: the index does not store a
    /// `MemoryStructure` id list. The shell caches the last
    /// [`generate_memories`] / [`MemoryGenerator`] result and drills in with
    /// [`Self::set_photo_ids_view`].
    ///
    /// iOS callers land in Phase 5.8.
    pub fn collection_structure(&self) -> ViewStructure {
        let _span = localcore_trace::span("catalog", "collection_structure");
        let guard = read(&self.inner);
        // Memories section is 5.6: there is no stored MemoryStructure
        // id list on the index. The shell caches generate_memories output and
        // uses set_photo_ids_view.
        let sections: Vec<ViewSection> = collection_groups(&guard.tags)
            .into_iter()
            .map(|group| {
                let leaves = leaf_tags(&group.tags);
                ViewSection {
                    id: collection_section_id(&group.name),
                    title: group.name,
                    slot_kind: ViewSlotKind::TextRow,
                    item_ids: leaves.into_iter().map(|tag| tag.id).collect(),
                }
            })
            .filter(|section| !section.item_ids.is_empty())
            .collect();
        let count: usize = sections.iter().map(|section| section.item_ids.len()).sum();
        localcore_trace::event(
            "catalog",
            format!(
                "collection_structure sections={} leaves={count}",
                sections.len()
            ),
        );
        ViewStructure {
            state: content_state(count),
            sections,
            actions: Vec::new(),
            generation: guard.generation,
        }
    }

    /// Display-ready collection rows for one namespace section.
    ///
    /// iOS callers land in Phase 5.8.
    pub fn collection_window(
        &self,
        section_id: String,
        offset: u64,
        limit: u64,
        generation: u64,
    ) -> Result<Vec<GalleryTextRow>, ViewError> {
        let guard = read(&self.inner);
        checked_window(generation, guard.generation, 0, 0, 0)?;
        let Some(group) = collection_groups(&guard.tags)
            .into_iter()
            .find(|group| collection_section_id(&group.name) == section_id)
        else {
            return Err(ViewError::SectionNotFound {
                section_id,
                message: "That Gallery collection section no longer exists.".into(),
                user_actionable: true,
            });
        };
        let leaves = leaf_tags(&group.tags);
        let range = checked_window(generation, guard.generation, offset, limit, leaves.len())?;
        Ok(leaves[range].iter().map(collection_text_row).collect())
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
    pub fn tag_suggestions(&self) -> TagStructures {
        let guard = read(&self.inner);
        TagStructures {
            tags: guard.tags.iter().map(TagSuggestionRecord::of).collect(),
            people: guard.people.iter().map(TagSuggestionRecord::of).collect(),
        }
    }

    /// How many photos the index currently holds. Cheap; used by the app's
    /// "did the rebuild I am waiting on actually land" assertions and by tests.
    pub fn photo_count(&self) -> u32 {
        read(&self.inner).index.photos().len() as u32
    }

    /// Scanner folder rows currently attached to this table.
    pub fn export_folders(&self) -> Vec<ScannedFolderHost> {
        read(&self.inner).folders.folders().to_vec()
    }

    /// Photo ids in scan order — the array folder slices address.
    pub fn scan_order_photo_ids(&self) -> Vec<String> {
        read(&self.inner).folders.photo_ids().to_vec()
    }

    /// Drop `ids` from the photo table the way iOS `photosRemoved` does:
    /// filter, `CoreIndex::build` the remainder, rewrite folder slices, bump
    /// generation. Does **not** call [`Self::rebuild`] — that would empty the
    /// folder table.
    ///
    /// Unknown ids are ignored. An empty or no-op request leaves generation
    /// and folders alone.
    pub fn remove_photos(&self, ids: Vec<String>) -> RemovePhotosResult {
        let _span =
            localcore_trace::span_always("catalog", "index.remove_photos").extra("n", ids.len());
        let drop: HashSet<StableId> = ids.iter().map(|id| parse_id(id)).collect();
        if drop.is_empty() {
            let guard = read(&self.inner);
            return RemovePhotosResult {
                removed_ids: Vec::new(),
                generation: guard.generation,
                photo_count: guard.index.photos().len() as u32,
                visible_photo_ids: guard
                    .visible_photo_ids
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                empty: guard.index.photos().is_empty(),
            };
        }

        let mut guard = write(&self.inner);
        let photos = guard.index.photos();
        let mut kept_photos = Vec::with_capacity(photos.len());
        let mut kept_offsets = Vec::with_capacity(guard.photo_time_zone_offsets.len());
        let mut removed = Vec::new();
        for (position, photo) in photos.iter().enumerate() {
            if drop.contains(&photo.id) {
                removed.push(photo.id.to_string());
            } else {
                kept_photos.push(photo.clone());
                if position < guard.photo_time_zone_offsets.len() {
                    kept_offsets.push(guard.photo_time_zone_offsets[position]);
                }
            }
        }
        if removed.is_empty() {
            return RemovePhotosResult {
                removed_ids: Vec::new(),
                generation: guard.generation,
                photo_count: photos.len() as u32,
                visible_photo_ids: guard
                    .visible_photo_ids
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                empty: photos.is_empty(),
            };
        }

        let drop_str: HashSet<String> = removed.iter().cloned().collect();
        let index = CoreIndex::build(kept_photos);
        let (tags, people) = index.tag_suggestions();
        let visible_photo_ids: Vec<StableId> = guard
            .visible_photo_ids
            .iter()
            .copied()
            .filter(|id| !drop.contains(id) && index.photo(*id).is_some())
            .collect();
        let visible_sections = photo_view_sections(&index, &visible_photo_ids, &kept_offsets);
        let path_by_id: HashMap<String, String> = index
            .photos()
            .iter()
            .map(|photo| (photo.id.to_string(), photo.url.path().to_string()))
            .collect();
        let folders = guard.folders.without_photo_ids(&drop_str, &path_by_id);
        let visible_folder_ids = folders.listing_ids(guard.visible_folder_parent.as_deref());

        guard.generation = guard.generation.saturating_add(1);
        guard.index = index;
        guard.tags = tags;
        guard.people = people;
        refresh_person_lists(&mut guard);
        guard.visible_photo_ids = visible_photo_ids;
        guard.visible_sections = visible_sections;
        guard.folders = folders;
        guard.visible_folder_ids = visible_folder_ids;
        guard.photo_time_zone_offsets = kept_offsets;

        let photo_count = guard.index.photos().len() as u32;
        RemovePhotosResult {
            removed_ids: removed,
            generation: guard.generation,
            photo_count,
            visible_photo_ids: guard
                .visible_photo_ids
                .iter()
                .map(ToString::to_string)
                .collect(),
            empty: photo_count == 0,
        }
    }
}

impl LibraryIndex {
    /// Categorized Photos search hits. Not on the UniFFI surface — GTK
    /// reads them directly; iOS already derives suggestion icons from
    /// tag namespaces.
    pub fn search_hits(&self, query: &str) -> Vec<gallery_index::SearchHit> {
        read(&self.inner).index.search_hits(query)
    }

    /// Cover id for a tag without building the full photo-id list.
    pub fn first_photo_id_for_tag(&self, full_path: String) -> Option<String> {
        read(&self.inner)
            .index
            .first_photo_id_for_tag(&full_path)
            .map(|id| id.to_string())
    }

    /// Clone the photo table as scan records for a Scan Photos run.
    pub fn export_photos(&self) -> Vec<ScannedMediaHost> {
        read(&self.inner)
            .index
            .photos()
            .iter()
            .map(photo_to_record)
            .collect()
    }
}

fn photo_view_sections(
    index: &CoreIndex,
    item_ids: &[StableId],
    time_zone_offsets: &[i32],
) -> Vec<PhotoViewSection> {
    let _span = localcore_trace::span("photos", "photo_view_sections").extra("ids", item_ids.len());
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

fn folder_structure_of(indexed: &Indexed) -> ViewStructure {
    ViewStructure {
        state: content_state(indexed.visible_folder_ids.len()),
        sections: vec![ViewSection {
            id: "folders".into(),
            title: "Folders".into(),
            slot_kind: ViewSlotKind::TextRow,
            item_ids: indexed.visible_folder_ids.clone(),
        }],
        actions: Vec::new(),
        generation: indexed.generation,
    }
}

fn photo_structure(indexed: &Indexed) -> ViewStructure {
    let _span = localcore_trace::span("photos", "photo_structure_clone")
        .extra("ids", indexed.visible_photo_ids.len());
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

/// [`LibraryIndex::tag_suggestions`]' pair.
///
/// R6 role: structure DTO.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct TagStructures {
    pub tags: Vec<TagStructureItem>,
    pub people: Vec<TagStructureItem>,
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

fn refresh_person_lists(guard: &mut Indexed) {
    let hidden = &guard.person_state.hidden;
    let featured = &guard.person_state.featured;
    guard.visible_people = page_people(&guard.people, hidden, featured);
    guard.rail_people = rail_people(&guard.people, hidden, featured, guard.person_now);
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
///
/// R6 role: structure DTO.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MemoryStructure {
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

impl MemoryStructure {
    fn of(m: &Memory) -> Self {
        MemoryStructure {
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
///
/// R6 role: structure DTO.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ScheduledMemoryStructure {
    pub memory: MemoryStructure,
    /// Local midnight of the day it is about, reference-date seconds.
    pub valid_from: f64,
    /// Local midnight of the following day.
    pub valid_to: f64,
}

// ---------------------------------------------------------------------------
// Memory inputs
// ---------------------------------------------------------------------------

/// A leaf `PhotoFolder`, by reference into the photo list rather than by value.
///
/// R6 role: command DTO.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MemoryFolderCommandItem {
    /// `PhotoFolder.id`. The memory id is `"folder-<this>"`.
    pub id: String,
    pub name: String,
    /// This folder's own photos, in listing order.
    pub photo_ids: Vec<String>,
}

/// `ContactInfo`, reduced to the fields the engine reads. `birthday.year` is
/// routinely absent in address-book data and is never consulted.
///
/// R6 role: command DTO.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MemoryContactCommandItem {
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
///
/// R6 role: command DTO.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MemoryPersonCommandItem {
    pub person_path: String,
    pub contact_id: Option<String>,
}

/// A `[String: Date]` entry — seen memories, surfaced clusters.
///
/// R6 role: command DTO.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MemoryDateCommandItem {
    pub key: String,
    /// Reference-date seconds.
    pub date: f64,
}

pub type TagSuggestionRecord = TagStructureItem;
pub type LibraryIndexSummary = LibraryBuildStructure;
pub type LibraryTagSuggestions = TagStructures;
pub type MemoryRecord = MemoryStructure;
pub type ScheduledMemoryRecord = ScheduledMemoryStructure;
pub type MemoryLeafFolder = MemoryFolderCommandItem;
pub type MemoryContact = MemoryContactCommandItem;
pub type MemoryPersonLink = MemoryPersonCommandItem;
pub type MemoryDateEntry = MemoryDateCommandItem;

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
    pub leaf_folders: Vec<MemoryFolderCommandItem>,
    pub contacts: Vec<MemoryContactCommandItem>,
    pub person_contact_links: Vec<MemoryPersonCommandItem>,
    pub birthdays_enabled: bool,
    pub me_person_path: String,
    pub hidden_people: Vec<String>,
    /// "Now", reference-date seconds.
    pub now: f64,
    pub time_zone_offset_seconds: i32,
    pub horizon_offset_seconds: Vec<i32>,
    pub seed: String,
    pub seen_memory_ids: Vec<MemoryDateCommandItem>,
    pub surfaced_clusters: Vec<MemoryDateCommandItem>,
}

/// Rust-only fixture shape for direct engine tests.
///
/// Production generation receives only [`ScheduledMemoryContext`] and reuses
/// [`LibraryIndex`]'s retained media table. This full-library snapshot is not a
/// UniFFI record and cannot cross to a shell.
#[derive(Debug, Clone)]
pub struct GenerateMemoriesCommand {
    pub photos: Vec<ScannedMediaHost>,
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
    pub leaf_folders: Vec<MemoryFolderCommandItem>,
    pub contacts: Vec<MemoryContactCommandItem>,
    pub person_contact_links: Vec<MemoryPersonCommandItem>,
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
    pub seen_memory_ids: Vec<MemoryDateCommandItem>,
    /// Cluster key → when the cluster last surfaced. −25 within 3 days.
    pub surfaced_clusters: Vec<MemoryDateCommandItem>,
}

pub type MemoryGenerationInputs = GenerateMemoriesCommand;

impl GenerateMemoriesCommand {
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
    GenerationInputs {
        photos,
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

    /// Run the ladder over the library retained by `index`. Returns the
    /// selected top-10, or an empty list if the run was cancelled.
    pub fn generate(
        &self,
        index: Arc<LibraryIndex>,
        context: ScheduledMemoryContext,
    ) -> Vec<MemoryStructure> {
        let _span = localcore_trace::span_always("memory", "MemoryGenerator::generate");
        let inputs = {
            let guard = read(&index.inner);
            scheduled_inputs(&guard, context)
        };
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
pub fn generate_memories(inputs: GenerateMemoriesCommand) -> Vec<MemoryStructure> {
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
pub fn compute_scheduled_memories(
    inputs: GenerateMemoriesCommand,
    horizon_days: i64,
    hidden_memory_ids: Vec<String>,
) -> Vec<ScheduledMemoryStructure> {
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
    use crate::scanner::{ScanTag, ScannedFolderHost};
    use crate::view::MAX_VIEW_WINDOW;

    fn folder_host(
        path: &str,
        name: &str,
        parent_index: Option<u32>,
        photo_start: u32,
        photo_count: u32,
        total_photo_count: i64,
    ) -> ScannedFolderHost {
        ScannedFolderHost {
            id: StableId::for_folder(path).to_string(),
            path: path.to_string(),
            name: name.to_string(),
            parent_index,
            photo_start,
            photo_count,
            cover_photo_path: None,
            total_photo_count,
            date_modified: None,
            date_created: None,
        }
    }

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

    fn retained_generation(
        inputs: MemoryGenerationInputs,
    ) -> (Arc<LibraryIndex>, ScheduledMemoryContext) {
        let index = LibraryIndex::new();
        index.build_with_time_zone_offsets(inputs.photos, inputs.photo_time_zone_offsets);
        let context = ScheduledMemoryContext {
            leaf_folders: inputs.leaf_folders,
            contacts: inputs.contacts,
            person_contact_links: inputs.person_contact_links,
            birthdays_enabled: inputs.birthdays_enabled,
            me_person_path: inputs.me_person_path,
            hidden_people: inputs.hidden_people,
            now: inputs.now,
            time_zone_offset_seconds: inputs.time_zone_offset_seconds,
            horizon_offset_seconds: inputs.horizon_offset_seconds,
            seed: inputs.seed,
            seen_memory_ids: inputs.seen_memory_ids,
            surfaced_clusters: inputs.surfaced_clusters,
        };
        (index, context)
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

    fn nested_location_library() -> (LibraryIndex, Vec<ScanPhoto>, Vec<ScannedFolderHost>) {
        let photos = vec![
            photo("/lib/2024/rome.jpg", Some(300.0), &["Places/Italy/Rome"]),
            photo("/lib/2024/paris/alice.jpg", Some(200.0), &["People/Alice"]),
            photo("/lib/2018/event.jpg", Some(100.0), &[]),
        ];
        let folders = vec![
            folder_host("/lib", "lib", None, 0, 0, 3),
            folder_host("/lib/2024", "2024", Some(0), 0, 1, 2),
            folder_host("/lib/2024/paris", "paris", Some(1), 1, 1, 1),
            folder_host("/lib/2018", "2018", Some(0), 2, 1, 1),
        ];
        let index = LibraryIndex::default();
        index.build(photos.clone());
        index.set_folders(
            folders.clone(),
            photos.iter().map(|p| p.id.clone()).collect(),
        );
        (index, photos, folders)
    }

    #[test]
    fn location_windows_are_empty_on_an_empty_library() {
        let index = LibraryIndex::default();
        let folders = index.folder_structure(None);
        assert_eq!(folders.state, ViewContentState::Empty);
        assert_eq!(folders.sections[0].id, "folders");
        assert!(folders.sections[0].item_ids.is_empty());
        assert!(index
            .folder_window("folders".into(), 0, 20, folders.generation)
            .unwrap()
            .is_empty());

        let people = index.people_structure();
        assert_eq!(people.state, ViewContentState::Empty);
        assert_eq!(people.sections[0].id, "people");
        assert!(index
            .people_window("people".into(), 0, 20, people.generation)
            .unwrap()
            .is_empty());

        let collections = index.collection_structure();
        assert_eq!(collections.state, ViewContentState::Empty);
        assert!(
            collections.sections.is_empty(),
            "memories section is 5.6 and no tag namespaces are present"
        );
        assert!(matches!(
            index.collection_window("places".into(), 0, 20, collections.generation),
            Err(ViewError::SectionNotFound { .. })
        ));
        assert!(index.folder_photo_ids("missing".into()).is_empty());
    }

    #[test]
    fn location_windows_refuse_stale_generations() {
        let (index, _, _) = nested_location_library();
        let folders = index.folder_structure(None);
        let people = index.people_structure();
        let collections = index.collection_structure();
        index.build(vec![photo("/lib/only.jpg", Some(1.0), &[])]);

        assert!(matches!(
            index.folder_window("folders".into(), 0, 20, folders.generation),
            Err(ViewError::StaleGeneration {
                requested,
                current,
                ..
            }) if requested == folders.generation && current > requested
        ));
        assert!(matches!(
            index.people_window("people".into(), 0, 20, people.generation),
            Err(ViewError::StaleGeneration { .. })
        ));
        assert!(matches!(
            index.collection_window("places".into(), 0, 20, collections.generation),
            Err(ViewError::StaleGeneration { .. })
        ));
    }

    #[test]
    fn location_windows_are_bounded_before_rows_are_allocated() {
        let (index, _, _) = nested_location_library();
        let folders = index.folder_structure(None);
        let people = index.people_structure();
        let collections = index.collection_structure();
        let too_big = MAX_VIEW_WINDOW as u64 + 1;
        assert!(matches!(
            index.folder_window("folders".into(), 0, too_big, folders.generation),
            Err(ViewError::WindowTooLarge { .. })
        ));
        assert!(matches!(
            index.people_window("people".into(), 0, too_big, people.generation),
            Err(ViewError::WindowTooLarge { .. })
        ));
        assert!(matches!(
            index.collection_window(
                collections.sections[0].id.clone(),
                0,
                too_big,
                collections.generation
            ),
            Err(ViewError::WindowTooLarge { .. })
        ));
    }

    #[test]
    fn nested_folders_people_and_places_use_location_windows() {
        let (index, photos, folders) = nested_location_library();
        let root = index.folder_structure(None);
        assert_eq!(root.sections[0].id, "folders");
        assert_eq!(
            root.sections[0].item_ids,
            vec![folders[1].id.clone(), folders[3].id.clone()],
            "the sole library root is not a row"
        );
        assert!(!root.sections[0].item_ids.contains(&folders[0].id));

        let rows = index
            .folder_window("folders".into(), 0, 20, root.generation)
            .unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| (row.title.as_str(), row.trailing.as_deref()))
                .collect::<Vec<_>>(),
            vec![("2024", Some("2 photos")), ("2018", Some("1 photo"))]
        );
        assert_eq!(
            index.folder_photo_ids(folders[1].id.clone()),
            vec![photos[0].id.clone()],
            "own photos are the scan slice, not the recursive total"
        );
        assert_eq!(
            index.folder_photo_ids(folders[2].id.clone()),
            vec![photos[1].id.clone()]
        );

        let year = index.folder_structure(Some(folders[1].id.clone()));
        assert!(year.generation > root.generation);
        assert_eq!(year.sections[0].item_ids, vec![folders[2].id.clone()]);
        assert!(matches!(
            index.folder_window("folders".into(), 0, 20, root.generation),
            Err(ViewError::StaleGeneration { .. })
        ));
        assert_eq!(
            index
                .folder_window("folders".into(), 0, 20, year.generation)
                .unwrap()[0]
                .title,
            "paris"
        );

        let people = index.people_structure();
        assert_eq!(people.sections[0].id, "people");
        assert_eq!(people.sections[0].item_ids.len(), 1);
        let person_rows = index
            .people_window("people".into(), 0, 20, people.generation)
            .unwrap();
        assert_eq!(person_rows[0].title, "Alice");
        assert_eq!(person_rows[0].trailing.as_deref(), Some("1 photo"));
        assert_eq!(
            index.photo_ids_for_tag("People/Alice".into()),
            vec![photos[1].id.clone()]
        );

        let collections = index.collection_structure();
        assert!(
            collections
                .sections
                .iter()
                .all(|section| section.id != "people"),
            "People are not a collection section"
        );
        let places = collections
            .sections
            .iter()
            .find(|section| section.id == "places")
            .expect("Places tag namespace is present");
        let place_rows = index
            .collection_window(places.id.clone(), 0, 20, collections.generation)
            .unwrap();
        assert!(place_rows.iter().any(|row| row.title == "Rome"));
        assert!(matches!(
            index.collection_window("people".into(), 0, 20, collections.generation),
            Err(ViewError::SectionNotFound { .. })
        ));
        assert!(matches!(
            index.folder_window("photos".into(), 0, 20, year.generation),
            Err(ViewError::SectionNotFound { .. })
        ));
        assert_eq!(person_rows[0].id, "People/Alice");
    }

    #[test]
    fn remove_photos_drops_sibling_and_bumps_generation() {
        let (index, photos, folders) = nested_location_library();
        index.set_photo_view(String::new(), Vec::new());
        let before = index.photo_structure();
        assert_eq!(before.sections[0].item_ids.len(), 3);
        let alice = photos[1].id.clone();
        let rome = photos[0].id.clone();
        let result = index.remove_photos(vec![alice.clone()]);
        assert_eq!(result.removed_ids, vec![alice.clone()]);
        assert_eq!(result.photo_count, 2);
        assert!(!result.empty);
        assert!(result.generation > before.generation);
        assert!(!result.visible_photo_ids.contains(&alice));
        assert!(result.visible_photo_ids.contains(&rome));
        assert_eq!(index.photo_count(), 2);
        assert_eq!(
            index.folder_photo_ids(folders[1].id.clone()),
            vec![rome.clone()],
            "2024 keeps its remaining own photo"
        );
        assert!(
            index.folder_photo_ids(folders[2].id.clone()).is_empty(),
            "paris own slice is empty after its only photo leaves"
        );
        assert_eq!(
            index.folder_photo_ids(folders[3].id.clone()),
            vec![photos[2].id.clone()]
        );
        let exported = index.export_folders();
        assert_eq!(exported[0].total_photo_count, 2);
        assert_eq!(exported[1].total_photo_count, 1);
        assert_eq!(exported[2].total_photo_count, 0);
        assert_eq!(exported[3].total_photo_count, 1);
        assert!(index.photo_ids_for_tag("People/Alice".into()).is_empty());
        assert!(matches!(
            index.photo_window("photos".into(), 0, 20, before.generation),
            Err(ViewError::StaleGeneration { .. })
        ));
        let fresh = index.photo_structure();
        let rows = index
            .photo_window("photos".into(), 0, 20, fresh.generation)
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.id != alice));
    }

    #[test]
    fn remove_photos_unknown_id_is_noop() {
        let (index, photos, _) = nested_location_library();
        let before = index.view_generation();
        let result = index.remove_photos(vec!["not-a-photo-id".into()]);
        assert!(result.removed_ids.is_empty());
        assert_eq!(result.photo_count, 3);
        assert_eq!(result.generation, before);
        assert_eq!(index.photo_count(), 3);
        assert_eq!(
            index.scan_order_photo_ids(),
            photos
                .iter()
                .map(|photo| photo.id.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn remove_photos_rewrites_a_gone_cover_path() {
        let (index, photos, mut folders) = nested_location_library();
        folders[2].cover_photo_path = Some(photos[1].path.clone());
        folders[1].cover_photo_path = Some(photos[0].path.clone());
        index.set_folders(
            folders.clone(),
            photos.iter().map(|photo| photo.id.clone()).collect(),
        );
        index.remove_photos(vec![photos[1].id.clone()]);
        let exported = index.export_folders();
        assert_eq!(exported[2].cover_photo_path, None);
        assert_eq!(
            exported[1].cover_photo_path.as_deref(),
            Some(photos[0].path.as_str())
        );
    }

    #[test]
    fn person_state_hides_and_features_people_windows() {
        let index = LibraryIndex::default();
        index.build(vec![
            photo("/lib/ada.jpg", Some(400.0), &["People/Ada"]),
            photo("/lib/bob.jpg", Some(400.0), &["People/Bob"]),
            photo("/lib/cy.jpg", Some(400.0), &["People/Cy"]),
        ]);
        let before = index.people_structure();
        assert_eq!(before.sections[0].item_ids.len(), 3);

        index.set_person_state(
            crate::person_log::PersonStateStructure {
                hidden: vec!["People/Cy".into()],
                featured: vec!["People/Bob".into()],
                ..crate::person_log::PersonStateStructure::default()
            },
            500.0,
        );
        let page = index.people_structure();
        assert!(page.generation > before.generation);
        assert_eq!(
            page.sections[0].item_ids,
            vec![
                "People/Bob".to_string(),
                "People/Ada".to_string(),
                "People/Cy".to_string()
            ]
        );
        let rail = index.people_rail_structure();
        assert_eq!(rail.sections[0].item_ids[0], "People/Bob");
        assert!(
            !rail.sections[0].item_ids.iter().any(|id| id == "People/Cy"),
            "hidden people stay off the Collections rail"
        );
        assert_eq!(
            index.person_full_path("people/ada".into()).as_deref(),
            Some("People/Ada")
        );
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
    /// always had. Delete uses [`LibraryIndex::remove_photos`] instead of
    /// another `build` so folder slices survive.
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
        let (index, context) = retained_generation(inputs);
        assert!(generator.generate(index, context).is_empty());
    }

    #[test]
    fn an_uncancelled_generator_matches_the_free_function() {
        let mut inputs = empty_inputs(739_800_000.0);
        inputs.photos = on_this_day_library(12);
        let generator = MemoryGenerator::default();
        let expected = generate_memories(inputs.clone());
        let (index, context) = retained_generation(inputs);
        assert_eq!(generator.generate(index, context), expected);
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
