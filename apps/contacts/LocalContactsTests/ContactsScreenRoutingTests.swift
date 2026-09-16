import Testing
@testable import LocalContacts

@Suite("Contacts screen routing")
struct ContactsScreenRoutingTests {
    @Test("every generated screen has a concrete iOS destination")
    func generatedScreenCoverage() {
        let routedScreens = [
            ContactsRouter.screen(for: FolderPickerView.self),
            ContactsRouter.screen(for: ContactListView.self),
            ContactsRouter.screen(for: ContactDetailView.self),
            ContactsRouter.screen(for: ContactEditView.self),
            ContactsRouter.screen(for: SettingsView.self),
            ContactsRouter.screen(for: TagManagementView.self),
            ContactsRouter.screen(for: LogsView.self),
            ContactsRouter.screen(for: ConflictResolutionSheet.self),
            ContactsRouter.screen(for: SyncConflictGroupSheet.self),
        ]

        #expect(routedScreens == ContactsScreen.allCases)
        #expect(ContactsRouter.screen(for: DocumentPickerView.self) == .folderPicker)
    }

    @Test("router carries generated screen identity")
    @MainActor
    func routeIdentity() {
        let diagnostics = ContactsRouter.destination(LogsView())
        let appleConflict = ContactsRouter.destination(
            ConflictResolutionSheet(contact: Contact())
        )
        let syncthingConflict = ContactsRouter.destination(SyncConflictGroupSheet())

        #expect(diagnostics.screen == .logs)
        #expect(diagnostics.screen.rawValue == "logs")
        #expect(appleConflict.screen == .appleConflict)
        #expect(syncthingConflict.screen == .syncConflictGroup)
        #expect(syncthingConflict.screen.rawValue == "sync-conflict-group")
    }
}
