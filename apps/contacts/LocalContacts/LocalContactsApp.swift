import SwiftUI

@main
struct LocalContactsApp: App {
    @State private var store = ContactsStore()
    private let syncService = CNSyncService()
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(store)
                .task {
                    await store.restoreFolder()
                }
                .onChange(of: scenePhase) { _, newPhase in
                    if newPhase == .active {
                        Task {
                            await checkForExternalChanges()
                        }
                    }
                }
        }
    }

    private func checkForExternalChanges() async {
        // Reload vcf files from disk
        await store.loadContacts()

        // Check CNContactStore for external changes
        let events = await syncService.fetchChanges(localContacts: store.contacts)

        for event in events {
            switch event.kind {
            case .updated(let data):
                if let contact = store.contacts.first(where: { $0.localContactsID == data.localContactsID }) {
                    contact.conflictState = .externalEdit
                }
            case .deleted(let localContactsID):
                if let contact = store.contacts.first(where: { $0.localContactsID == localContactsID }) {
                    contact.conflictState = .externalDelete
                }
            case .added:
                // Could prompt to import — for now just log
                print("External contact added in CNContactStore")
            }
        }
    }
}
