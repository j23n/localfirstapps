//! Flat folder table and display-row helpers for location windows.
//!
//! Scanner folders stay in [`crate::scanner::ScannedFolderHost`] form so the
//! recursive `PhotoFolder` tree never crosses this crate's FFI again.

use std::collections::{HashMap, HashSet};

use gallery_index::TagSuggestion;

use crate::scanner::ScannedFolderHost;
use crate::view::GalleryTextRow;

/// Scanner folders plus the photo-id order their slices address.
#[derive(Debug, Clone, Default)]
pub(crate) struct FolderTable {
    folders: Vec<ScannedFolderHost>,
    photo_ids: Vec<String>,
    by_id: HashMap<String, usize>,
    children: Vec<Vec<usize>>,
    roots: Vec<usize>,
}

impl FolderTable {
    pub(crate) fn new(folders: Vec<ScannedFolderHost>, photo_ids: Vec<String>) -> Self {
        let _span = localcore_trace::span("folders", "FolderTable::new")
            .extra("folders", folders.len())
            .extra("ids", photo_ids.len());
        let mut children = vec![Vec::new(); folders.len()];
        let mut roots = Vec::new();
        let mut by_id = HashMap::with_capacity(folders.len());
        for (index, folder) in folders.iter().enumerate() {
            by_id.insert(folder.id.clone(), index);
            match folder.parent_index {
                Some(parent) => {
                    let parent = parent as usize;
                    if parent < index {
                        children[parent].push(index);
                    }
                }
                None => roots.push(index),
            }
        }
        Self {
            folders,
            photo_ids,
            by_id,
            children,
            roots,
        }
    }

    pub(crate) fn empty() -> Self {
        Self::default()
    }

    pub(crate) fn folders(&self) -> &[ScannedFolderHost] {
        &self.folders
    }

    pub(crate) fn photo_ids(&self) -> &[String] {
        &self.photo_ids
    }

    /// Drop ids, keep scan order, and rewrite every folder's `[start, count)`.
    ///
    /// Remaining own-photos stay contiguous because the scan array only
    /// shrinks. Recursive totals are recomputed from the new own-counts.
    /// A cover path that named a dropped photo becomes the first remaining
    /// own photo's path.
    pub(crate) fn without_photo_ids(
        &self,
        drop: &HashSet<String>,
        path_by_id: &HashMap<String, String>,
    ) -> Self {
        if drop.is_empty() {
            return self.clone();
        }
        let mut new_ids = Vec::with_capacity(self.photo_ids.len());
        let mut old_to_new = HashMap::with_capacity(self.photo_ids.len());
        for (old, id) in self.photo_ids.iter().enumerate() {
            if !drop.contains(id) {
                old_to_new.insert(old, new_ids.len() as u32);
                new_ids.push(id.clone());
            }
        }
        let mut folders = self.folders.clone();
        for folder in &mut folders {
            let start = folder.photo_start as usize;
            let end = start.saturating_add(folder.photo_count as usize);
            let kept: Vec<(u32, &String)> = (start..end.min(self.photo_ids.len()))
                .filter_map(|old| {
                    let id = &self.photo_ids[old];
                    if drop.contains(id) {
                        None
                    } else {
                        Some((*old_to_new.get(&old)?, id))
                    }
                })
                .collect();
            folder.photo_count = kept.len() as u32;
            folder.photo_start = kept.first().map(|(index, _)| *index).unwrap_or(0);
            let cover_gone = folder.cover_photo_path.as_ref().is_some_and(|cover| {
                !kept
                    .iter()
                    .any(|(_, id)| path_by_id.get(*id).is_some_and(|path| path == cover))
            });
            if cover_gone {
                folder.cover_photo_path = kept
                    .first()
                    .and_then(|(_, id)| path_by_id.get(*id).cloned());
            }
        }
        let mut table = FolderTable::new(folders, new_ids);
        table.recompute_totals();
        table
    }

    fn recompute_totals(&mut self) {
        fn walk(table: &mut FolderTable, index: usize) -> i64 {
            let children = table.children[index].clone();
            let mut total = i64::from(table.folders[index].photo_count);
            for child in children {
                total += walk(table, child);
            }
            table.folders[index].total_photo_count = total;
            total
        }
        let roots = self.roots.clone();
        for root in roots {
            walk(self, root);
        }
    }

    /// Child folder ids for `parent_id`.
    ///
    /// `None` is the Folders-tab root: children of the scan root, skipping the
    /// library root itself when it is the only root.
    pub(crate) fn listing_ids(&self, parent_id: Option<&str>) -> Vec<String> {
        self.listing_indices(parent_id)
            .into_iter()
            .map(|index| self.folders[index].id.clone())
            .collect()
    }

    fn listing_indices(&self, parent_id: Option<&str>) -> Vec<usize> {
        match parent_id {
            None => {
                if self.roots.len() == 1 {
                    self.children[self.roots[0]].clone()
                } else {
                    self.roots.clone()
                }
            }
            Some(id) => self
                .by_id
                .get(id)
                .map(|&index| self.children[index].clone())
                .unwrap_or_default(),
        }
    }

    pub(crate) fn get(&self, id: &str) -> Option<&ScannedFolderHost> {
        self.by_id.get(id).map(|&index| &self.folders[index])
    }

    /// This folder's own photos (the scan slice), not the recursive total.
    pub(crate) fn own_photo_ids(&self, folder_id: &str) -> Vec<String> {
        let Some(folder) = self.get(folder_id) else {
            return Vec::new();
        };
        let start = folder.photo_start as usize;
        let end = start.saturating_add(folder.photo_count as usize);
        self.photo_ids
            .get(start..end.min(self.photo_ids.len()))
            .unwrap_or(&[])
            .to_vec()
    }
}

pub(crate) fn folder_text_row(folder: &ScannedFolderHost) -> GalleryTextRow {
    let leaf = folder.path.rsplit('/').find(|part| !part.is_empty());
    let subtitle = leaf
        .filter(|part| *part != folder.name.as_str())
        .map(str::to_string);
    GalleryTextRow {
        id: folder.id.clone(),
        title: folder.name.clone(),
        subtitle,
        trailing: Some(photo_count_label(saturating_count(
            folder.total_photo_count,
        ))),
    }
}

pub(crate) fn people_text_row(person: &TagSuggestion) -> GalleryTextRow {
    GalleryTextRow {
        id: person.full_path.clone(),
        title: person.display_name.clone(),
        subtitle: None,
        trailing: Some(photo_count_label(person.count)),
    }
}

pub(crate) fn collection_text_row(tag: &TagSuggestion) -> GalleryTextRow {
    GalleryTextRow {
        id: tag.id.clone(),
        title: tag.display_name.clone(),
        subtitle: Some(tag.full_path.replace('/', " › ")),
        trailing: Some(photo_count_label(tag.count)),
    }
}

pub(crate) fn collection_section_id(name: &str) -> String {
    name.to_ascii_lowercase()
}

fn saturating_count(count: i64) -> usize {
    usize::try_from(count.max(0)).unwrap_or(usize::MAX)
}

pub(crate) fn photo_count_label(count: usize) -> String {
    if count == 1 {
        "1 photo".into()
    } else {
        format!("{count} photos")
    }
}
