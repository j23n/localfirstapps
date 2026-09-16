//! GTK routes backed by the generated Contacts screen identifiers.

use shell_kit_gtk::ContactsScreen;

/// Screens assembled by the GTK shell at this milestone.
pub const ROUTED_SCREENS: &[ContactsScreen] = &[
    ContactsScreen::FolderPicker,
    ContactsScreen::ContactList,
    ContactsScreen::ContactDetail,
    ContactsScreen::ContactEdit,
    ContactsScreen::Settings,
    ContactsScreen::TagManagement,
    ContactsScreen::Logs,
    ContactsScreen::SyncConflictGroup,
];

/// Keep platform omissions explicit and make a newly generated screen an
/// exhaustive-match compile error.
#[must_use]
pub const fn gtk_route(screen: ContactsScreen) -> Option<ContactsScreen> {
    match screen {
        ContactsScreen::FolderPicker
        | ContactsScreen::ContactList
        | ContactsScreen::ContactDetail
        | ContactsScreen::ContactEdit
        | ContactsScreen::Settings
        | ContactsScreen::TagManagement
        | ContactsScreen::Logs
        | ContactsScreen::SyncConflictGroup => Some(screen),
        ContactsScreen::AppleConflict => None,
    }
}

#[must_use]
pub(crate) fn route_id(screen: ContactsScreen) -> &'static str {
    gtk_route(screen).expect("screen has no GTK route").as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routed_screen_inventory_uses_generated_ids() {
        let routed: Vec<_> = ContactsScreen::ALL
            .iter()
            .copied()
            .filter_map(gtk_route)
            .collect();
        assert_eq!(routed, ROUTED_SCREENS);
        assert_eq!(route_id(ContactsScreen::Logs), "logs");
    }

    #[test]
    fn only_platform_or_pending_screens_are_unrouted() {
        let unrouted: Vec<_> = ContactsScreen::ALL
            .iter()
            .copied()
            .filter(|screen| gtk_route(*screen).is_none())
            .collect();
        assert_eq!(unrouted, [ContactsScreen::AppleConflict]);
    }
}
