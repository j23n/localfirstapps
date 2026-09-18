//! Folder tree over the scan table, without FFI location windows.
//!
//! [`LibraryIndex::folder_structure`] writes `visible_folder_parent` and
//! bumps the shared view generation. A Folders-tab click that goes through
//! that path invalidates photo windows and is why drill-in felt lagged.

use gallery_ffi::ScannedFolderHost;

/// One folder for the explorer sidebar or grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub parent_id: Option<String>,
    pub photo_count: u32,
    pub total_photo_count: i64,
    pub cover_photo_path: Option<String>,
    pub has_children: bool,
}

/// Child-folder indices of `parent_id`. `None` is the Folders-tab root:
/// children of the scan root, skipping that root when it is the only one.
#[must_use]
pub fn listing_indices(folders: &[ScannedFolderHost], parent_id: Option<&str>) -> Vec<usize> {
    match parent_id {
        None => match unique_root_index(folders) {
            Some(root) => children_of(folders, root),
            None => folders
                .iter()
                .enumerate()
                .filter(|(_, folder)| folder.parent_index.is_none())
                .map(|(index, _)| index)
                .collect(),
        },
        Some(id) => folders
            .iter()
            .position(|folder| folder.id == id)
            .map(|index| children_of(folders, index))
            .unwrap_or_default(),
    }
}

/// Display rows for [`listing_indices`].
#[must_use]
pub fn listing_entries(folders: &[ScannedFolderHost], parent_id: Option<&str>) -> Vec<FolderEntry> {
    listing_indices(folders, parent_id)
        .into_iter()
        .map(|index| entry_at(folders, index))
        .collect()
}

/// Root → current, excluding the hidden scan root. Empty at the tab root.
#[must_use]
pub fn crumb_trail(folders: &[ScannedFolderHost], id: Option<&str>) -> Vec<FolderEntry> {
    let Some(id) = id else {
        return Vec::new();
    };
    let Some(start) = folders.iter().position(|folder| folder.id == id) else {
        return Vec::new();
    };
    let mut chain = Vec::new();
    let mut current = Some(start);
    while let Some(index) = current {
        chain.push(entry_at(folders, index));
        current = folders
            .get(index)
            .and_then(|folder| folder.parent_index)
            .map(|parent| parent as usize)
            .filter(|parent| *parent < folders.len());
    }
    chain.reverse();
    if unique_root_index(folders).is_some() {
        chain.retain(|entry| entry.parent_id.is_some());
    }
    chain
}

/// `folder_stack` for `id`: `[None]` at the tab root, then each crumb id.
#[must_use]
pub fn path_stack(folders: &[ScannedFolderHost], id: Option<&str>) -> Vec<Option<String>> {
    let mut stack = vec![None];
    for entry in crumb_trail(folders, id) {
        stack.push(Some(entry.id));
    }
    stack
}

/// Scan-root id when that root is skipped as a Folders row.
#[must_use]
pub fn skipped_root_id(folders: &[ScannedFolderHost]) -> Option<&str> {
    unique_root_index(folders).map(|index| folders[index].id.as_str())
}

#[must_use]
pub fn entry_at(folders: &[ScannedFolderHost], index: usize) -> FolderEntry {
    let folder = &folders[index];
    let parent_id = folder
        .parent_index
        .and_then(|parent| folders.get(parent as usize))
        .map(|parent| parent.id.clone());
    FolderEntry {
        id: folder.id.clone(),
        name: folder.name.clone(),
        path: folder.path.clone(),
        parent_id,
        photo_count: folder.photo_count,
        total_photo_count: folder.total_photo_count,
        cover_photo_path: folder.cover_photo_path.clone(),
        has_children: folders
            .iter()
            .any(|child| child.parent_index == Some(index as u32)),
    }
}

fn unique_root_index(folders: &[ScannedFolderHost]) -> Option<usize> {
    let mut found = None;
    for (index, folder) in folders.iter().enumerate() {
        if folder.parent_index.is_none() {
            if found.is_some() {
                return None;
            }
            found = Some(index);
        }
    }
    found
}

fn children_of(folders: &[ScannedFolderHost], parent: usize) -> Vec<usize> {
    folders
        .iter()
        .enumerate()
        .filter(|(_, folder)| folder.parent_index == Some(parent as u32))
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(
        id: &str,
        name: &str,
        parent_index: Option<u32>,
        photo_count: u32,
        total_photo_count: i64,
    ) -> ScannedFolderHost {
        ScannedFolderHost {
            id: id.to_string(),
            path: format!("/{name}"),
            name: name.to_string(),
            parent_index,
            photo_start: 0,
            photo_count,
            cover_photo_path: None,
            total_photo_count,
            date_modified: None,
            date_created: None,
        }
    }

    fn tree() -> Vec<ScannedFolderHost> {
        vec![
            folder("folder-root", "lib", None, 1, 3),
            folder("folder-2024", "2024", Some(0), 1, 2),
            folder("folder-paris", "paris", Some(1), 1, 1),
        ]
    }

    #[test]
    fn root_listing_skips_the_single_scan_root() {
        let folders = tree();
        let rows = listing_entries(&folders, None);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "folder-2024");
        assert!(rows[0].has_children);
        assert_eq!(skipped_root_id(&folders), Some("folder-root"));
    }

    #[test]
    fn nested_listing_and_crumbs() {
        let folders = tree();
        let year = listing_entries(&folders, Some("folder-2024"));
        assert_eq!(year.len(), 1);
        assert_eq!(year[0].id, "folder-paris");
        assert!(!year[0].has_children);

        assert!(crumb_trail(&folders, None).is_empty());
        let crumbs = crumb_trail(&folders, Some("folder-paris"));
        assert_eq!(
            crumbs
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["folder-2024", "folder-paris"]
        );
        assert_eq!(
            path_stack(&folders, Some("folder-paris")),
            vec![
                None,
                Some("folder-2024".into()),
                Some("folder-paris".into())
            ]
        );
    }

    #[test]
    fn several_roots_are_listed() {
        let folders = vec![folder("a", "A", None, 1, 1), folder("b", "B", None, 2, 2)];
        let rows = listing_entries(&folders, None);
        assert_eq!(rows.len(), 2);
        assert!(skipped_root_id(&folders).is_none());
        assert_eq!(
            crumb_trail(&folders, Some("b"))
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["b"]
        );
    }
}
