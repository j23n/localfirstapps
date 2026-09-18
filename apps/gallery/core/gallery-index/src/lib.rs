//! Search and tag indexes over a photo table.
//!
//! Pure functions of `allPhotos`. The sort tiebreak, the corpus join,
//! and Swift's canonical-equivalence substring matching are easy to
//! get quietly wrong. Fixtures
//! `core/fixtures/memories-conformance/{search_index,tag_index}.json`
//! pin all three; `tests/index_conformance.rs` runs this code against
//! them.
//!
//! One owner, one photo table: [`LibraryIndex`] holds the photos and
//! every index is a list of **indices** into it.

#![forbid(unsafe_code)]

pub mod collections;
pub mod people;
pub mod search;
pub mod tags;
pub mod text;

use std::collections::HashMap;

use gallery_model::{PhotoFile, StableId};

pub use collections::{collection_groups, leaf_tags, CollectionGroup};
pub use people::{visible_people, PEOPLE_RAIL_CAP};
pub use search::{
    corpus_entry, corpus_terms, photo_count_label, SearchHit, SearchKind, DISTANT_PAST,
};
pub use tags::{matches_by_prefix, TagIndex, TagSuggestion};

/// The photo table plus every index built over it.
///
/// Rebuilt wholesale from `all_photos`, exactly as the Swift pair is: the Store
/// calls `build(allPhotos:)` on both after every `apply(_:)`, and partial
/// updates were never part of the contract.
#[derive(Debug, Clone)]
pub struct LibraryIndex {
    photos: Vec<PhotoFile>,
    by_id: HashMap<StableId, u32>,
    /// Indices into `photos`, date descending with the `url.path` tiebreak.
    sorted: Vec<u32>,
    /// Parallel to `photos`: the newline-joined, match-key-folded corpus.
    corpus: Vec<String>,
    /// Parallel to `photos`: each photo's tag paths, already [`text::match_key`]ed.
    ///
    /// Folding is not free — `to_lowercase` + NFC allocates a `String` per tag
    /// — and `photo_carries` runs it **per photo per required tag per query**.
    /// A tag-branch query over 20k photos re-normalised ~60k paths every
    /// keystroke, so the fold moved to build time where it happens once.
    ///
    /// Deliberately *not* the tag buckets, even though they hold the same
    /// folded strings: the buckets prefix-expand `objects`/`scenes` as well as
    /// `places`, while `photo_carries` expands `places` alone. That asymmetry
    /// is pinned Swift behaviour (landmines 18/19) — answering a query from the
    /// buckets would quietly widen `Objects/Vehicle` into a prefix filter.
    tag_keys: Vec<Vec<String>>,
    tags: TagIndex,
}

impl LibraryIndex {
    /// Build every index from `all_photos`. One pass per index, no photo copies
    /// beyond the single table this takes ownership of.
    pub fn build(all_photos: Vec<PhotoFile>) -> Self {
        let _span = localcore_trace::span_always("catalog", "LibraryIndex::build")
            .extra("photos", all_photos.len());
        let keys: Vec<search::SortKey> = all_photos.iter().map(search::SortKey::new).collect();
        let mut sorted: Vec<u32> = (0..all_photos.len() as u32).collect();
        // Stable, so the comparator alone decides — the Swift `sorted` is not
        // stable, which is precisely why it needs the path tiebreak.
        sorted.sort_by(|a, b| keys[*a as usize].cmp(&keys[*b as usize]));

        // First id wins, mirroring `uniquingKeysWith: { a, _ in a }`.
        let mut by_id: HashMap<StableId, u32> = HashMap::with_capacity(all_photos.len());
        for (i, photo) in all_photos.iter().enumerate() {
            by_id.entry(photo.id).or_insert(i as u32);
        }

        let corpus: Vec<String> = all_photos.iter().map(search::corpus_entry).collect();
        let tag_keys: Vec<Vec<String>> = all_photos
            .iter()
            .map(|p| {
                p.hierarchical_tags
                    .iter()
                    .map(|t| text::match_key(&t.full_path))
                    .collect()
            })
            .collect();
        let tags = TagIndex::build(&all_photos);

        LibraryIndex {
            photos: all_photos,
            by_id,
            sorted,
            corpus,
            tag_keys,
            tags,
        }
    }

    /// The photo table, in the order it was handed over.
    pub fn photos(&self) -> &[PhotoFile] {
        &self.photos
    }

    /// The tag index.
    pub fn tags(&self) -> &TagIndex {
        &self.tags
    }

    /// Date-descending photo list. Backs `store.sortedPhotos` — this order *is*
    /// the grid.
    pub fn sorted_photos(&self) -> impl Iterator<Item = &PhotoFile> {
        self.sorted.iter().map(|i| &self.photos[*i as usize])
    }

    /// The sorted list as ids, for the FFI boundary.
    pub fn sorted_photo_ids(&self) -> Vec<StableId> {
        self.sorted_photos().map(|p| p.id).collect()
    }

    /// `photoByID` — O(1) lookup.
    pub fn photo(&self, id: StableId) -> Option<&PhotoFile> {
        self.by_id.get(&id).map(|i| &self.photos[*i as usize])
    }

    /// `TagIndex.photos(forTag:)`.
    pub fn photos_for_tag(&self, full_path: &str) -> Vec<&PhotoFile> {
        self.tags
            .indices_for_key(&text::match_key(full_path))
            .iter()
            .map(|i| &self.photos[*i as usize])
            .collect()
    }

    /// First photo credited to `full_path`, without allocating the full list.
    pub fn first_photo_id_for_tag(&self, full_path: &str) -> Option<StableId> {
        self.tags
            .indices_for_key(&text::match_key(full_path))
            .first()
            .map(|i| self.photos[*i as usize].id)
    }

    /// `TagIndex.aggregateTagsAndPeople` over this index.
    pub fn tag_suggestions(&self) -> (Vec<TagSuggestion>, Vec<TagSuggestion>) {
        self.tags.aggregate(&self.photos)
    }

    /// `SearchIndex.search(query:requiredTags:allTags:)`.
    ///
    /// Required tags AND together and are applied **before** the query. Then
    /// one of two branches:
    ///
    /// - the query equals a known tag path → filter by that tag (with
    ///   `Places/*` prefix expansion), or
    /// - anything else → a single substring match against the corpus.
    ///
    /// `all_tags` is the caller's aggregated list rather than this index's own,
    /// mirroring the Swift signature: the Store owns that list, and it is what
    /// makes *virtual* prefix tags (`places/italy/lazio`, which no photo
    /// carries) queryable. Pass [`Self::tag_suggestions`]'s first element to
    /// get the app's behaviour.
    pub fn search(
        &self,
        query: &str,
        required_tags: &[TagSuggestion],
        all_tags: &[TagSuggestion],
    ) -> Vec<&PhotoFile> {
        self.search_indices(query, required_tags, all_tags)
            .into_iter()
            .map(|i| &self.photos[i as usize])
            .collect()
    }

    /// [`Self::search`] as photo-table indices — the allocation-free form the
    /// paging FFI wants.
    pub fn search_indices(
        &self,
        query: &str,
        required_tags: &[TagSuggestion],
        all_tags: &[TagSuggestion],
    ) -> Vec<u32> {
        let mut results = self.sorted.clone();

        for tag in required_tags {
            let path = text::match_key(&tag.full_path);
            let is_places =
                tag.namespace.as_deref().map(text::lowercased).as_deref() == Some("places");
            results.retain(|i| self.photo_carries(*i, &path, is_places));
        }

        if query.is_empty() {
            return results;
        }

        let q = text::match_key(query);
        match all_tags.iter().find(|t| text::match_key(&t.full_path) == q) {
            Some(matched) => {
                let is_places = matched
                    .namespace
                    .as_deref()
                    .map(text::lowercased)
                    .as_deref()
                    == Some("places");
                results.retain(|i| self.photo_carries(*i, &q, is_places));
            }
            // `q` is already in the corpus's canonical form, so the fallback
            // is a plain byte search. Date-shaped queries also match
            // `dateTaken` so "June 2023" can be a first-class hit.
            None => {
                let date_query = search::parse_date_query(&q);
                results.retain(|i| {
                    self.corpus[*i as usize].contains(&q)
                        || date_query.is_some_and(|query| {
                            search::photo_matches_date(&self.photos[*i as usize], query)
                        })
                });
            }
        }
        results
    }

    /// Live-search hits for the Photos search field: matching tags and
    /// capture dates, each carrying a kind icon like contacts field hits.
    #[must_use]
    pub fn search_hits(&self, query: &str) -> Vec<SearchHit> {
        let needle = query.trim();
        if needle.is_empty() {
            return Vec::new();
        }
        let q = text::match_key(needle);
        let (tags, _) = self.tag_suggestions();
        let mut ranked: Vec<(usize, SearchHit)> = tags
            .into_iter()
            .filter_map(|tag| {
                let kind = SearchKind::from_namespace(tag.namespace.as_deref())?;
                let name = text::match_key(&tag.display_name);
                let path = text::match_key(&tag.full_path);
                if !name.contains(&q) && !path.contains(&q) {
                    return None;
                }
                Some((
                    tag.count,
                    SearchHit {
                        id: tag.id,
                        kind,
                        title: tag.display_name,
                        subtitle: Some(search::photo_count_label(tag.count)),
                    },
                ))
            })
            .collect();
        ranked.extend(self.date_hits(&q));
        ranked.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| left.1.kind.cmp(&right.1.kind))
                .then_with(|| left.1.title.cmp(&right.1.title))
        });
        ranked.truncate(8);
        ranked.into_iter().map(|(_, hit)| hit).collect()
    }

    fn date_hits(&self, q: &str) -> Vec<(usize, SearchHit)> {
        let mut years = std::collections::BTreeMap::<i32, usize>::new();
        let mut months = std::collections::BTreeMap::<(i32, u32), usize>::new();
        for photo in &self.photos {
            let Some(date) = photo.date_taken else {
                continue;
            };
            let civil = gallery_model::CivilDateTime::from_unix_secs_f64(date.unix_secs_f64());
            *years.entry(civil.year).or_default() += 1;
            *months.entry((civil.year, civil.month)).or_default() += 1;
        }
        let mut hits = Vec::new();
        for (year, count) in years {
            let title = year.to_string();
            if !text::match_key(&title).contains(q) {
                continue;
            }
            hits.push((
                count,
                SearchHit {
                    id: format!("date:{year:04}"),
                    kind: SearchKind::Date,
                    title,
                    subtitle: Some(search::photo_count_label(count)),
                },
            ));
        }
        for ((year, month), count) in months {
            let name = search::month_name(month);
            let title = format!("{name} {year}");
            let iso = format!("{year:04}-{month:02}");
            if !text::match_key(&title).contains(q)
                && !iso.contains(q)
                && !text::match_key(name).contains(q)
            {
                continue;
            }
            hits.push((
                count,
                SearchHit {
                    id: format!("date:{iso}"),
                    kind: SearchKind::Date,
                    title,
                    subtitle: Some(search::photo_count_label(count)),
                },
            ));
        }
        hits
    }

    /// Does photo `idx` carry `path` (already [`text::match_key`]ed), counting a nested
    /// path when `is_places`?
    ///
    /// Reads the pre-folded [`Self::tag_keys`] rather than re-normalising the
    /// photo's tags, which is what keeps a tag-branch query linear in *tags*
    /// rather than linear in tags × a Unicode normalisation each.
    fn photo_carries(&self, idx: u32, path: &str, is_places: bool) -> bool {
        self.tag_keys[idx as usize].iter().any(|hp| {
            // `hp == path || (isPlaces && hp.hasPrefix(path + "/"))`,
            // without building the concatenation on every comparison.
            hp == path
                || (is_places
                    && hp.len() > path.len()
                    && hp.starts_with(path)
                    && hp.as_bytes()[path.len()] == b'/')
        })
    }
}

#[cfg(test)]
mod search_hit_tests {
    use super::*;
    use gallery_model::{AppleDate, CivilDateTime, HierarchicalTag, PhotoFile};

    fn dated(path: &str, year: i32, month: u32, tags: &[&str]) -> PhotoFile {
        let mut photo = PhotoFile::new(path, path.rsplit('/').next().unwrap_or(path), 0);
        photo.date_taken = Some(AppleDate::from_unix_secs_f64(
            CivilDateTime::new(year, month, 15, 12, 0, 0).as_naive_unix_secs() as f64,
        ));
        photo.hierarchical_tags = tags.iter().map(|tag| HierarchicalTag::new(tag)).collect();
        photo
    }

    #[test]
    fn search_hits_name_the_matched_category() {
        let index = LibraryIndex::build(vec![dated(
            "/a.jpg",
            2023,
            6,
            &[
                "Places/Italy/Rome",
                "People/Ada",
                "Scenes/Beach",
                "Objects/Cat",
            ],
        )]);
        let kinds = |query: &str| {
            index
                .search_hits(query)
                .into_iter()
                .map(|hit| hit.kind)
                .collect::<Vec<_>>()
        };
        assert!(kinds("rome").contains(&SearchKind::Location));
        assert!(kinds("ada").contains(&SearchKind::People));
        assert!(kinds("beach").contains(&SearchKind::Scene));
        assert!(kinds("cat").contains(&SearchKind::Objects));
        assert!(kinds("2023").contains(&SearchKind::Date));
        assert!(kinds("june").contains(&SearchKind::Date));
        assert_eq!(SearchKind::Location.symbol(), "mark-location-symbolic");
        assert_eq!(SearchKind::Date.label(), "Date");
    }

    #[test]
    fn date_query_filters_photos_without_changing_tag_queries() {
        let index = LibraryIndex::build(vec![
            dated("/a.jpg", 2023, 6, &[]),
            dated("/b.jpg", 2023, 7, &["Places/Rome"]),
        ]);
        let (tags, _) = index.tag_suggestions();
        assert_eq!(index.search("june", &[], &tags).len(), 1);
        assert_eq!(index.search("rome", &[], &tags).len(), 1);
        assert_eq!(index.search("2023-06", &[], &tags).len(), 1);
    }

    #[test]
    fn first_photo_id_for_tag_skips_the_full_list() {
        let index = LibraryIndex::build(vec![
            dated("/a.jpg", 2023, 6, &["People/Ada"]),
            dated("/b.jpg", 2023, 7, &["People/Ada"]),
        ]);
        let all = index.photos_for_tag("People/Ada");
        assert_eq!(all.len(), 2);
        assert_eq!(index.first_photo_id_for_tag("People/Ada"), Some(all[0].id));
        assert_eq!(index.first_photo_id_for_tag("People/Missing"), None);
    }
}
