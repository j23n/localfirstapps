import SwiftUI

/// A concrete Contacts destination with an identity from the generated UI spec.
protocol ContactsScreenDestination: View {
    nonisolated static var contactsScreen: ContactsScreen { get }
}

/// Keeps the existing destination view while giving its root accessibility
/// element the stable identifier generated from `ui-spec/screens.toml`.
struct ContactsScreenRoute<Destination: ContactsScreenDestination>: View {
    let screen: ContactsScreen
    private let destination: Destination

    init(destination: Destination) {
        screen = Destination.contactsScreen
        self.destination = destination
    }

    var body: some View {
        destination
            .accessibilityIdentifier(screen.rawValue)
    }
}

enum ContactsRouter {
    @MainActor
    static func destination<Destination: ContactsScreenDestination>(
        _ destination: Destination
    ) -> ContactsScreenRoute<Destination> {
        ContactsScreenRoute(destination: destination)
    }

    nonisolated static func screen<Destination: ContactsScreenDestination>(
        for _: Destination.Type
    ) -> ContactsScreen {
        Destination.contactsScreen
    }
}

extension FolderPickerView: ContactsScreenDestination {
    nonisolated static var contactsScreen: ContactsScreen { .folderPicker }
}

extension DocumentPickerView: ContactsScreenDestination {
    nonisolated static var contactsScreen: ContactsScreen { .folderPicker }
}

extension ContactListView: ContactsScreenDestination {
    nonisolated static var contactsScreen: ContactsScreen { .contactList }
}

extension ContactDetailView: ContactsScreenDestination {
    nonisolated static var contactsScreen: ContactsScreen { .contactDetail }
}

extension ContactEditView: ContactsScreenDestination {
    nonisolated static var contactsScreen: ContactsScreen { .contactEdit }
}

extension SettingsView: ContactsScreenDestination {
    nonisolated static var contactsScreen: ContactsScreen { .settings }
}

extension TagManagementView: ContactsScreenDestination {
    nonisolated static var contactsScreen: ContactsScreen { .tagManagement }
}

extension LogsView: ContactsScreenDestination {
    nonisolated static var contactsScreen: ContactsScreen { .logs }
}

extension ConflictResolutionSheet: ContactsScreenDestination {
    nonisolated static var contactsScreen: ContactsScreen { .appleConflict }
}

extension SyncConflictGroupSheet: ContactsScreenDestination {
    nonisolated static var contactsScreen: ContactsScreen { .syncConflictGroup }
}
