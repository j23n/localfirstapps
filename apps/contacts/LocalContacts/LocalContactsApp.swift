import SwiftUI

@main
struct LocalContactsApp: App {
    @State private var store = ContactsStore()
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

        await store.loadContacts()

        let events = await store.syncService.fetchChanges(localContacts: store.contacts)
        guard !events.isEmpty else { return }

        for event in events {
            switch event.kind {
            case .updated(let data):
                if let contact = store.contacts.first(where: { $0.localContactsID == data.localContactsID }),
                   contact.conflictState == nil {
                    contact.conflictState = .externalEdit
                }
            case .deleted(let localContactsID):
                if let contact = store.contacts.first(where: { $0.localContactsID == localContactsID }),
                   contact.conflictState == nil {
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
        contact.organization = data.organization
        contact.jobTitle = data.jobTitle
        contact.nickname = data.nickname
        contact.fullName = [data.givenName, data.middleName, data.familyName]
            .filter { !$0.isEmpty }.joined(separator: " ")
        contact.phoneNumbers = data.phoneNumbers.map { LabeledValue(label: $0.label, value: $0.value) }
        contact.emailAddresses = data.emailAddresses.map { LabeledValue(label: $0.label, value: $0.value) }
        contact.urls = data.urls.map { LabeledValue(label: $0.label, value: $0.value) }
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
            // Claim the EXISTING CN contact — don't push a duplicate
            await store.syncService.claimCNContact(
                cnIdentifier: data.cnIdentifier,
                forLocalContactsID: contact.localContactsID
            )
        } catch {
            print("Failed to import external contact: \(error)")
        }
    }
}
