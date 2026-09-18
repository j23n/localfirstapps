//! GTK routes backed by the generated Gallery screen identifiers.

use shell_kit_gtk::GalleryScreen;

/// Screens assembled by the GTK shell at Phase 5.6.
///
/// `face-review` stays an unbound gap. `sync-conflict-group` is **5.8-xmp-ui**.
pub const ROUTED_SCREENS: &[GalleryScreen] = &[
    GalleryScreen::FolderPicker,
    GalleryScreen::Folders,
    GalleryScreen::Folder,
    GalleryScreen::Photos,
    GalleryScreen::Collections,
    GalleryScreen::Memory,
    GalleryScreen::People,
    GalleryScreen::Person,
    GalleryScreen::Events,
    GalleryScreen::Album,
    GalleryScreen::Viewer,
    GalleryScreen::PhotoInfo,
    GalleryScreen::Settings,
    GalleryScreen::Logs,
];

/// Keep platform omissions explicit and make a newly generated screen an
/// exhaustive-match compile error.
#[must_use]
pub const fn gtk_route(screen: GalleryScreen) -> Option<GalleryScreen> {
    match screen {
        GalleryScreen::FolderPicker
        | GalleryScreen::Folders
        | GalleryScreen::Folder
        | GalleryScreen::Photos
        | GalleryScreen::Collections
        | GalleryScreen::Memory
        | GalleryScreen::People
        | GalleryScreen::Person
        | GalleryScreen::Events
        | GalleryScreen::Album
        | GalleryScreen::Viewer
        | GalleryScreen::PhotoInfo
        | GalleryScreen::Settings
        | GalleryScreen::Logs => Some(screen),
        GalleryScreen::SyncConflictGroup | GalleryScreen::FaceReview => None,
    }
}

#[must_use]
pub(crate) fn route_id(screen: GalleryScreen) -> &'static str {
    gtk_route(screen).expect("screen has no GTK route").as_str()
}

/// Outcome of a `folders_nav` pop for the folder-id stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FolderStackPop {
    /// Viewer / photo-info (or any non-folder page): leave `folder_stack`.
    Leave,
    /// Popped a folder page. `parent` is the remaining top (`None` = Folders root).
    PopFolder { parent: Option<String> },
}

/// Only Folder pages pop `folder_stack`. Viewer and photo-info stay put.
#[must_use]
pub(crate) fn folder_stack_pop_policy(
    popped_widget_name: &str,
    stack_before_pop: &[Option<String>],
) -> FolderStackPop {
    if popped_widget_name != route_id(GalleryScreen::Folder) {
        return FolderStackPop::Leave;
    }
    let parent = if stack_before_pop.len() >= 2 {
        stack_before_pop[stack_before_pop.len() - 2].clone()
    } else {
        None
    };
    FolderStackPop::PopFolder { parent }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routed_screen_inventory_uses_generated_ids() {
        let routed: Vec<_> = GalleryScreen::ALL
            .iter()
            .copied()
            .filter_map(gtk_route)
            .collect();
        assert_eq!(routed, ROUTED_SCREENS);
        assert_eq!(ROUTED_SCREENS.len(), 14);
        assert_eq!(route_id(GalleryScreen::Photos), "photos");
        assert_eq!(route_id(GalleryScreen::Logs), "logs");
        assert_eq!(route_id(GalleryScreen::Viewer), "viewer");
        assert_eq!(route_id(GalleryScreen::PhotoInfo), "photo-info");
        assert_eq!(route_id(GalleryScreen::Folder), "folder");
        assert_eq!(route_id(GalleryScreen::People), "people");
    }

    #[test]
    fn folder_stack_pop_policy_only_folder_pages() {
        let nested = vec![
            None,
            Some("folder-2024".into()),
            Some("folder-paris".into()),
        ];
        assert_eq!(
            folder_stack_pop_policy(route_id(GalleryScreen::Viewer), &nested),
            FolderStackPop::Leave
        );
        assert_eq!(
            folder_stack_pop_policy(route_id(GalleryScreen::PhotoInfo), &nested),
            FolderStackPop::Leave
        );
        assert_eq!(
            folder_stack_pop_policy(route_id(GalleryScreen::Folders), &nested),
            FolderStackPop::Leave
        );
        assert_eq!(
            folder_stack_pop_policy(route_id(GalleryScreen::Folder), &nested),
            FolderStackPop::PopFolder {
                parent: Some("folder-2024".into())
            }
        );
        assert_eq!(
            folder_stack_pop_policy(
                route_id(GalleryScreen::Folder),
                &[None, Some("folder-2024".into())]
            ),
            FolderStackPop::PopFolder { parent: None }
        );
        assert_eq!(
            folder_stack_pop_policy(route_id(GalleryScreen::Folder), &[None]),
            FolderStackPop::PopFolder { parent: None }
        );
    }

    #[test]
    fn pending_and_unbound_screens_are_unrouted() {
        let unrouted: Vec<_> = GalleryScreen::ALL
            .iter()
            .copied()
            .filter(|screen| gtk_route(*screen).is_none())
            .collect();
        assert_eq!(
            unrouted,
            [GalleryScreen::SyncConflictGroup, GalleryScreen::FaceReview]
        );
    }
}
