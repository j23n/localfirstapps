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
        guard store.folderURL != nil else { return }

        // Reload vcf files from disk
        await store.loadContacts()

        // Check CNContactStore for external changes
        let events = await syncService.fetchChanges(localContacts: store.contacts)

        // Only touch UI state if there are actual changes
        guard !events.isEmpty else { return }

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
            case .added(let data):
                await importExternalContact(data)
            }
        }
    }

    private func importExternalContact(_ data: CNSyncService.CNContactData) async {
        let contact = Contact()
        contact.givenName = data.givenName
        contact.familyName = data.familyName
        contact.middleName = data.middleName
        contact.namePrefix = data.namePrefix
        contact.nameSuffix = data.nameSuffix
        contact.fullName = [data.givenName, data.middleName, data.familyName]
            .filter { !$0.isEmpty }.joined(separator: " ")
        contact.phoneNumbers = data.phoneNumbers.map { LabeledValue(label: $0.label, value: $0.value) }
        contact.emailAddresses = data.emailAddresses.map { LabeledValue(label: $0.label, value: $0.value) }
        contact.postalAddresses = data.postalAddresses.map {
            LabeledValue(label: $0.label, value: PostalAddress(
                street: $0.street, city: $0.city, state: $0.state,
                postalCode: $0.postalCode, country: $0.country
            ))
        }
        contact.birthday = data.birthday
        if let photo = data.imageData { contact.photoData = photo }

        do {
            try await store.save(contact)
            // Push back so the CN contact gets linked via ID mapping
            try? await syncService.pushContact(contact)
        } catch {
            print("Failed to import external contact: \(error)")
        }
    }
}
