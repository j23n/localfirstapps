//! Generation-checked `gio::ListModel` over FFI structure ids.
//!
//! Items are `{id, section, index}` only. Photo/text records live in a page
//! cache keyed `(generation, section, page)` with windows of ≤256. A stale
//! generation replaces the model; it is never patched.

use std::collections::{HashMap, HashSet};

use gallery_ffi::view::MAX_VIEW_WINDOW;
use gallery_ffi::{ViewError, ViewStructure};
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

/// One visible window. Matches [`MAX_VIEW_WINDOW`].
pub const PAGE: u64 = MAX_VIEW_WINDOW as u64;

/// Lightweight list row. The model does not hold `GalleryMediaItem`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewItem {
    pub id: String,
    pub section: String,
    pub index: u32,
}

/// First month of a calendar year on the Photos timeline.
///
/// `first_index` is the [`ViewList::photos_flat`] / `gio::ListModel` index of
/// that month's first id. Years come from existing `photo_structure` month
/// sections — not a year FFI window and not leftover grouping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YearMark {
    pub year: String,
    pub first_index: u32,
    pub section_id: String,
}

/// Pure structure projection. Unit-testable without a display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewList {
    pub generation: u64,
    items: Vec<ViewItem>,
}

impl ViewList {
    /// Flatten every section's `item_ids` in structure order.
    #[must_use]
    pub fn from_structure(structure: &ViewStructure) -> Self {
        let mut items = Vec::new();
        for section in &structure.sections {
            push_section(&mut items, &section.id, &section.item_ids);
        }
        Self {
            generation: structure.generation,
            items,
        }
    }

    /// One section, or empty when that id is absent (`SectionNotFound` → empty).
    #[must_use]
    pub fn from_section(structure: &ViewStructure, section_id: &str) -> Self {
        let items = structure
            .sections
            .iter()
            .find(|section| section.id == section_id)
            .map(|section| {
                let mut items = Vec::new();
                push_section(&mut items, &section.id, &section.item_ids);
                items
            })
            .unwrap_or_default();
        Self {
            generation: structure.generation,
            items,
        }
    }

    /// Synthetic flattened `photos` slice (same order as `photo_window("photos")`).
    #[must_use]
    pub fn photos_flat(structure: &ViewStructure) -> Self {
        let mut items = Vec::new();
        for section in &structure.sections {
            push_section(&mut items, "photos", &section.item_ids);
        }
        Self {
            generation: structure.generation,
            items,
        }
    }

    #[must_use]
    pub fn n_items(&self) -> u32 {
        u32::try_from(self.items.len()).unwrap_or(u32::MAX)
    }

    #[must_use]
    pub fn item(&self, position: u32) -> Option<ViewItem> {
        self.items.get(position as usize).cloned()
    }

    #[must_use]
    pub fn items(&self) -> &[ViewItem] {
        &self.items
    }

    #[must_use]
    pub fn into_items(self) -> Vec<ViewItem> {
        self.items
    }
}

/// True when `incoming` is the same photo id list already shown.
///
/// A warm enrich / stale reload must not replace the `gio::ListModel` if
/// this holds — factory unbind would cancel every in-flight thumb.
#[must_use]
pub fn same_item_ids(bound: &[String], incoming: &[ViewItem]) -> bool {
    bound.len() == incoming.len() && bound.iter().zip(incoming).all(|(id, item)| id == &item.id)
}

/// Year marks in structure order. Later months of a seen year are ignored.
///
/// Year is the last title token when it is four digits, otherwise `YYYY`
/// from an id suffix `:YYYY-MM`. `Unknown Date` / `:unknown` are skipped.
/// `first_index` is the running sum of prior `item_ids.len()` so it matches
/// [`ViewList::photos_flat`].
#[must_use]
pub fn years_from_structure(structure: &ViewStructure) -> Vec<YearMark> {
    let mut marks = Vec::new();
    let mut seen = HashSet::new();
    let mut first_index = 0_u32;
    for section in &structure.sections {
        if let Some(year) = year_of_section(&section.title, &section.id) {
            if seen.insert(year.clone()) {
                marks.push(YearMark {
                    year,
                    first_index,
                    section_id: section.id.clone(),
                });
            }
        }
        first_index = first_index.saturating_add(section_len(section.item_ids.len()));
    }
    marks
}

fn section_len(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

fn year_of_section(title: &str, id: &str) -> Option<String> {
    if title == "Unknown Date" {
        return None;
    }
    let suffix = id.rsplit_once(':').map(|(_, rest)| rest).unwrap_or(id);
    if suffix == "unknown" {
        return None;
    }
    if let Some(token) = title.split_whitespace().last() {
        if is_year_token(token) {
            return Some(token.to_string());
        }
    }
    year_from_month_suffix(suffix)
}

fn is_year_token(token: &str) -> bool {
    token.len() == 4 && token.bytes().all(|b| b.is_ascii_digit())
}

fn year_from_month_suffix(suffix: &str) -> Option<String> {
    let (year, month) = suffix.split_once('-')?;
    if is_year_token(year) && month.len() == 2 && month.bytes().all(|b| b.is_ascii_digit()) {
        Some(year.to_string())
    } else {
        None
    }
}

fn push_section(items: &mut Vec<ViewItem>, section: &str, ids: &[String]) {
    for id in ids {
        let index = u32::try_from(items.len()).unwrap_or(u32::MAX);
        items.push(ViewItem {
            id: id.clone(),
            section: section.to_string(),
            index,
        });
    }
}

/// Page index for a flat offset (`offset / 256`).
#[must_use]
pub fn page_index(offset: u64) -> u64 {
    offset / PAGE
}

/// Inclusive start offset of the page that contains `index`.
#[must_use]
#[allow(dead_code)]
pub fn page_offset(index: u32) -> u64 {
    (u64::from(index) / PAGE) * PAGE
}

/// `(generation, section, page)` cache. A generation mismatch drops the map.
#[derive(Debug)]
pub struct PageCache<T> {
    generation: u64,
    pages: HashMap<(String, u64), Vec<T>>,
}

impl<T> Default for PageCache<T> {
    fn default() -> Self {
        Self {
            generation: 0,
            pages: HashMap::new(),
        }
    }
}

impl<T> PageCache<T> {
    #[must_use]
    pub fn new(generation: u64) -> Self {
        Self {
            generation,
            pages: HashMap::new(),
        }
    }

    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Drop every page. Caller replaces the `gio::ListModel` as well.
    pub fn replace_generation(&mut self, generation: u64) {
        self.generation = generation;
        self.pages.clear();
    }

    pub fn get(&self, generation: u64, section: &str, page: u64) -> Option<&[T]> {
        if generation != self.generation {
            return None;
        }
        self.pages
            .get(&(section.to_string(), page))
            .map(Vec::as_slice)
    }

    pub fn insert(&mut self, generation: u64, section: String, page: u64, rows: Vec<T>) {
        if generation != self.generation {
            self.replace_generation(generation);
        }
        self.pages.insert((section, page), rows);
    }

    /// Load one page. `StaleGeneration` is returned to the caller so it can
    /// replace the model. `SectionNotFound` is an empty page, not a patch.
    pub fn fetch(
        &mut self,
        generation: u64,
        section: &str,
        index: u32,
        load: impl FnOnce(u64, u64) -> Result<Vec<T>, ViewError>,
    ) -> Result<Option<&T>, ViewError> {
        if generation != self.generation {
            return Err(ViewError::StaleGeneration {
                requested: generation,
                current: self.generation,
                message: "This Gallery view changed. Reload its structure and retry.".into(),
                user_actionable: true,
            });
        }
        let page = page_index(u64::from(index));
        let key = (section.to_string(), page);
        if !self.pages.contains_key(&key) {
            let rows = match load(page * PAGE, PAGE) {
                Ok(rows) => rows,
                Err(ViewError::SectionNotFound { .. }) => Vec::new(),
                Err(error) => return Err(error),
            };
            self.pages.insert(key.clone(), rows);
        }
        let local = (index as usize) % MAX_VIEW_WINDOW;
        Ok(self.pages.get(&key).and_then(|rows| rows.get(local)))
    }
}

/// `gio::ListStore` of [`ViewItem`] boxed values. This is the ListModel the
/// grid/list factories bind. gtk-rs `ListModelImpl` subclasses emit `unsafe`
/// impls; this crate stays `forbid(unsafe_code)` and uses ListStore instead.
#[derive(Clone, Debug)]
pub struct ViewListModel {
    store: gio::ListStore,
    generation: u64,
}

impl ViewListModel {
    #[must_use]
    pub fn empty(generation: u64) -> Self {
        Self {
            store: gio::ListStore::new::<glib::BoxedAnyObject>(),
            generation,
        }
    }

    pub fn append_items(&self, items: &[ViewItem]) {
        if items.is_empty() {
            return;
        }
        let added: Vec<glib::BoxedAnyObject> = items
            .iter()
            .map(|item| glib::BoxedAnyObject::new(item.clone()))
            .collect();
        self.store.splice(self.store.n_items(), 0, &added);
    }

    /// Drop matching rows from the existing store, high index first, so
    /// GTK keeps the scroll position. Do not `splice(0, n, kept)`.
    pub fn remove_ids(&self, drop: &HashSet<String>) -> u32 {
        if drop.is_empty() {
            return 0;
        }
        let mut positions: Vec<u32> = (0..self.n_items())
            .filter(|&index| self.item(index).is_some_and(|item| drop.contains(&item.id)))
            .collect();
        positions.sort_unstable();
        for position in positions.iter().rev() {
            self.store.remove(*position);
        }
        positions.len() as u32
    }

    #[must_use]
    pub fn from_list(list: &ViewList) -> Self {
        let _span = localcore_trace::span("listmodel", "from_list")
            .extra("n", list.n_items())
            .extra("gen", list.generation);
        let model = Self::empty(list.generation);
        model.append_items(list.items());
        model
    }

    #[must_use]
    pub fn from_structure(structure: &ViewStructure) -> Self {
        Self::from_list(&ViewList::from_structure(structure))
    }

    #[must_use]
    pub fn photos_flat(structure: &ViewStructure) -> Self {
        Self::from_list(&ViewList::photos_flat(structure))
    }

    #[must_use]
    pub fn from_section(structure: &ViewStructure, section_id: &str) -> Self {
        Self::from_list(&ViewList::from_section(structure, section_id))
    }

    #[must_use]
    pub fn n_items(&self) -> u32 {
        self.store.n_items()
    }

    pub fn set_generation(&mut self, generation: u64) {
        self.generation = generation;
    }

    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub fn store(&self) -> gio::ListStore {
        self.store.clone()
    }

    #[must_use]
    pub fn item(&self, position: u32) -> Option<ViewItem> {
        let boxed = self
            .store
            .item(position)?
            .downcast::<glib::BoxedAnyObject>()
            .ok()?;
        let item = boxed.borrow::<ViewItem>().clone();
        Some(item)
    }
}

/// Read a boxed factory item.
#[must_use]
pub fn view_item_from_object(object: &glib::Object) -> Option<ViewItem> {
    let boxed = object.downcast_ref::<glib::BoxedAnyObject>()?;
    let item = boxed.borrow::<ViewItem>().clone();
    Some(item)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gallery_ffi::{ViewContentState, ViewSection, ViewSlotKind};

    fn structure_with(count: usize, generation: u64) -> ViewStructure {
        let item_ids = (0..count).map(|i| format!("id-{i}")).collect();
        ViewStructure {
            state: ViewContentState::Content,
            sections: vec![ViewSection {
                id: "photos".into(),
                title: "Photos".into(),
                slot_kind: ViewSlotKind::MediaItem,
                item_ids,
            }],
            actions: Vec::new(),
            generation,
        }
    }

    fn month_structure(generation: u64) -> ViewStructure {
        ViewStructure {
            state: ViewContentState::Content,
            sections: vec![
                ViewSection {
                    id: "photos:0:2024-01".into(),
                    title: "January 2024".into(),
                    slot_kind: ViewSlotKind::MediaItem,
                    item_ids: vec!["a".into(), "b".into()],
                },
                ViewSection {
                    id: "photos:1:2024-02".into(),
                    title: "February 2024".into(),
                    slot_kind: ViewSlotKind::MediaItem,
                    item_ids: vec!["c".into()],
                },
            ],
            actions: Vec::new(),
            generation,
        }
    }

    #[test]
    fn same_item_ids_matches_order() {
        let list = ViewList::from_structure(&structure_with(4, 1));
        let ids: Vec<String> = list.items().iter().map(|item| item.id.clone()).collect();
        assert!(same_item_ids(&ids, list.items()));
        assert!(!same_item_ids(&ids[..3], list.items()));
        let mut swapped = ids.clone();
        swapped.swap(0, 1);
        assert!(!same_item_ids(&swapped, list.items()));
    }

    #[test]
    fn n_items_matches_structure_count() {
        let structure = structure_with(20, 3);
        let list = ViewList::from_structure(&structure);
        assert_eq!(list.n_items(), 20);
        assert_eq!(list.item(0).unwrap().id, "id-0");
        assert_eq!(list.item(0).unwrap().section, "photos");
        assert_eq!(list.item(19).unwrap().index, 19);
        assert!(list.item(20).is_none());
    }

    #[test]
    fn paging_three_hundred_ids_stays_bounded() {
        let structure = structure_with(300, 8);
        let list = ViewList::from_structure(&structure);
        assert_eq!(list.n_items(), 300);
        assert_eq!(page_index(0), 0);
        assert_eq!(page_index(256), 1);
        assert_eq!(page_offset(256), 256);
        assert!(PAGE <= 256);
        assert_eq!(PAGE, MAX_VIEW_WINDOW as u64);
    }

    #[test]
    fn stale_generation_replaces_the_list_not_patches() {
        let old = ViewList::from_structure(&structure_with(4, 1));
        let new = ViewList::from_structure(&structure_with(6, 2));
        assert_eq!(old.n_items(), 4);
        assert_eq!(new.n_items(), 6);
        assert_ne!(old.generation, new.generation);
        assert_eq!(old.item(0).unwrap().id, "id-0");
        assert_eq!(new.item(5).unwrap().id, "id-5");
    }

    #[test]
    fn page_cache_refuses_to_serve_another_generation() {
        let mut cache = PageCache::new(4);
        cache.insert(4, "photos".into(), 0, vec!["a", "b"]);
        assert_eq!(cache.get(4, "photos", 0), Some(["a", "b"].as_slice()));
        assert!(cache.get(5, "photos", 0).is_none());
        let stale = cache.fetch(5, "photos", 0, |_, _| Ok(vec!["nope"]));
        assert!(matches!(stale, Err(ViewError::StaleGeneration { .. })));
        cache.replace_generation(5);
        let row = cache
            .fetch(5, "photos", 0, |offset, limit| {
                assert_eq!(offset, 0);
                assert!(limit <= 256);
                Ok(vec!["fresh"])
            })
            .unwrap();
        assert_eq!(row, Some(&"fresh"));
    }

    #[test]
    fn missing_section_is_empty_not_a_stale_patch() {
        let mut cache = PageCache::<String>::new(1);
        let row = cache
            .fetch(1, "missing", 0, |_, _| {
                Err(ViewError::SectionNotFound {
                    section_id: "missing".into(),
                    message: "gone".into(),
                    user_actionable: true,
                })
            })
            .unwrap();
        assert!(row.is_none());
    }

    #[test]
    fn flat_photos_uses_the_photos_section_id() {
        let list = ViewList::photos_flat(&month_structure(9));
        assert_eq!(list.n_items(), 3);
        assert!(list.items().iter().all(|item| item.section == "photos"));
        assert_eq!(list.item(2).unwrap().id, "c");
    }

    #[test]
    fn remove_ids_drops_rows_without_resetting_the_store() {
        let model = ViewListModel::from_structure(&structure_with(5, 1));
        let store = model.store();
        assert_eq!(
            model.remove_ids(&HashSet::from(["id-1".into(), "id-3".into()])),
            2
        );
        assert_eq!(model.n_items(), 3);
        assert_eq!(model.item(0).unwrap().id, "id-0");
        assert_eq!(model.item(1).unwrap().id, "id-2");
        assert_eq!(model.item(2).unwrap().id, "id-4");
        assert_eq!(store.n_items(), 3);
    }

    #[test]
    fn gio_list_model_n_items_matches_structure() {
        let structure = structure_with(20, 1);
        let model = ViewListModel::from_structure(&structure);
        assert_eq!(model.n_items(), 20);
        assert_eq!(model.generation(), 1);
        assert_eq!(model.item(3).unwrap().id, "id-3");
        let replaced = ViewListModel::from_structure(&structure_with(7, 2));
        assert_eq!(replaced.n_items(), 7);
        assert_eq!(model.n_items(), 20);
    }

    #[test]
    fn years_from_month_fixture_are_one_year_at_flat_index_zero() {
        let structure = month_structure(9);
        let years = years_from_structure(&structure);
        assert_eq!(
            years,
            [YearMark {
                year: "2024".into(),
                first_index: 0,
                section_id: "photos:0:2024-01".into(),
            }]
        );
        let list = ViewList::photos_flat(&structure);
        assert_eq!(list.n_items(), 3);
        assert_eq!(list.item(years[0].first_index).unwrap().id, "a");
        assert_eq!(list.item(years[0].first_index).unwrap().index, 0);
    }

    fn two_year_unknown_structure() -> ViewStructure {
        ViewStructure {
            state: ViewContentState::Content,
            sections: vec![
                ViewSection {
                    id: "photos:0:unknown".into(),
                    title: "Unknown Date".into(),
                    slot_kind: ViewSlotKind::MediaItem,
                    item_ids: vec!["u0".into(), "u1".into()],
                },
                ViewSection {
                    id: "photos:2:2024-01".into(),
                    title: "January 2024".into(),
                    slot_kind: ViewSlotKind::MediaItem,
                    item_ids: vec!["a".into(), "b".into()],
                },
                ViewSection {
                    id: "photos:4:2024-02".into(),
                    title: "February 2024".into(),
                    slot_kind: ViewSlotKind::MediaItem,
                    item_ids: vec!["c".into()],
                },
                ViewSection {
                    id: "photos:5:2023-12".into(),
                    title: "Holiday".into(),
                    slot_kind: ViewSlotKind::MediaItem,
                    item_ids: vec!["d".into(), "e".into(), "f".into()],
                },
            ],
            actions: Vec::new(),
            generation: 4,
        }
    }

    #[test]
    fn years_from_two_years_keep_first_month_of_each() {
        let structure = two_year_unknown_structure();
        let years = years_from_structure(&structure);
        assert_eq!(
            years,
            [
                YearMark {
                    year: "2024".into(),
                    first_index: 2,
                    section_id: "photos:2:2024-01".into(),
                },
                YearMark {
                    year: "2023".into(),
                    first_index: 5,
                    section_id: "photos:5:2023-12".into(),
                },
            ]
        );
        let list = ViewList::photos_flat(&structure);
        assert_eq!(list.n_items(), 8);
        for mark in &years {
            assert_eq!(list.item(mark.first_index).unwrap().index, mark.first_index);
        }
        assert_eq!(list.item(2).unwrap().id, "a");
        assert_eq!(list.item(5).unwrap().id, "d");
    }

    #[test]
    fn years_skip_unknown_date_and_keep_flat_indexes() {
        let structure = ViewStructure {
            state: ViewContentState::Content,
            sections: vec![
                ViewSection {
                    id: "photos:0:2022-06".into(),
                    title: "June 2022".into(),
                    slot_kind: ViewSlotKind::MediaItem,
                    item_ids: vec!["j".into()],
                },
                ViewSection {
                    id: "photos:1:unknown".into(),
                    title: "Unknown Date".into(),
                    slot_kind: ViewSlotKind::MediaItem,
                    item_ids: vec!["u".into(), "v".into()],
                },
            ],
            actions: Vec::new(),
            generation: 1,
        };
        let years = years_from_structure(&structure);
        assert_eq!(
            years,
            [YearMark {
                year: "2022".into(),
                first_index: 0,
                section_id: "photos:0:2022-06".into(),
            }]
        );
        assert_eq!(ViewList::photos_flat(&structure).n_items(), 3);
        assert!(years_from_structure(&ViewStructure {
            state: ViewContentState::Empty,
            sections: vec![ViewSection {
                id: "photos:0:unknown".into(),
                title: "Unknown Date".into(),
                slot_kind: ViewSlotKind::MediaItem,
                item_ids: vec!["u".into()],
            }],
            actions: Vec::new(),
            generation: 2,
        })
        .is_empty());
    }
}
