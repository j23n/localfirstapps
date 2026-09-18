//! GTK routes backed by the generated Gallery screen identifiers.

use shell_kit_gtk::GalleryScreen;

/// Screens assembled by the GTK shell at Phase 5.6.
///
/// `face-review` stays an unbound gap.
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
    GalleryScreen::SyncConflictGroup,
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
        | GalleryScreen::Logs
        | GalleryScreen::SyncConflictGroup => Some(screen),
        GalleryScreen::FaceReview => None,
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

/// Key that can cancel Select or pop the visible [`adw::NavigationView`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavKey {
    Escape,
    AltLeft,
}

/// Outcome of leftover-parity back keys. Pure so tests need no display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavKeyAction {
    CancelSelect,
    Pop,
    Ignore,
}

/// Escape cancels Select first, else pops. `Alt+Left` pops. Search focus
/// keeps both keys for the entry (type-to-search / clear).
#[must_use]
pub(crate) fn nav_key_policy(selecting: bool, search_focused: bool, key: NavKey) -> NavKeyAction {
    if search_focused {
        return NavKeyAction::Ignore;
    }
    match key {
        NavKey::Escape if selecting => NavKeyAction::CancelSelect,
        NavKey::Escape | NavKey::AltLeft => NavKeyAction::Pop,
    }
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
        assert_eq!(ROUTED_SCREENS.len(), 15);
        assert_eq!(
            route_id(GalleryScreen::SyncConflictGroup),
            "sync-conflict-group"
        );
        assert_eq!(route_id(GalleryScreen::Photos), "photos");
        assert_eq!(route_id(GalleryScreen::Logs), "logs");
        assert_eq!(route_id(GalleryScreen::Viewer), "viewer");
        assert_eq!(route_id(GalleryScreen::PhotoInfo), "photo-info");
        assert_eq!(route_id(GalleryScreen::Folder), "folder");
        assert_eq!(route_id(GalleryScreen::People), "people");
    }

    #[test]
    fn nav_key_policy_covers_select_search_and_back() {
        assert_eq!(
            nav_key_policy(true, false, NavKey::Escape),
            NavKeyAction::CancelSelect
        );
        assert_eq!(
            nav_key_policy(false, false, NavKey::Escape),
            NavKeyAction::Pop
        );
        assert_eq!(
            nav_key_policy(true, false, NavKey::AltLeft),
            NavKeyAction::Pop
        );
        assert_eq!(
            nav_key_policy(false, false, NavKey::AltLeft),
            NavKeyAction::Pop
        );
        assert_eq!(
            nav_key_policy(true, true, NavKey::Escape),
            NavKeyAction::Ignore
        );
        assert_eq!(
            nav_key_policy(false, true, NavKey::AltLeft),
            NavKeyAction::Ignore
        );
        assert_eq!(
            nav_key_policy(false, true, NavKey::Escape),
            NavKeyAction::Ignore
        );
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
        assert_eq!(unrouted, [GalleryScreen::FaceReview]);
    }
}
