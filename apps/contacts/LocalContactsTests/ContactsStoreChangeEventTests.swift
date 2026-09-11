import Foundation
import Testing
@testable import LocalContacts

@MainActor
@Suite("ContactsStore — change events")
struct ContactsStoreChangeEventTests {

    private func makeDefaults() -> UserDefaults {
        let name = "LocalContactsTests-Events-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: name)!
        defaults.removePersistentDomain(forName: name)
        return defaults
    }

    private func makeTempFolder() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("LocalContactsTests-Events-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    @Test("updated sets externalEdit only when conflictState is nil")
    func updatedSetsConflictOnce() async {
        let existing = Contact(localContactsID: "lcid-1", givenName: "Alice")
        let store = ContactsStore()
        store.contacts = [existing]

        let data = CNSyncService.CNContactData.stub(localContactsID: "lcid-1")
        await store.applyChangeEvents([.init(kind: .updated(contactData: data))])
        #expect(existing.conflictState?.isExternalEdit == true)

        existing.conflictState = .externalDelete
        let other = CNSyncService.CNContactData.stub(localContactsID: "lcid-1", givenName: "Zed")
        await store.applyChangeEvents([.init(kind: .updated(contactData: other))])
        if case .externalDelete = existing.conflictState {
            // preserved
        } else {
            Issue.record("existing conflict was overwritten")
        }
    }

    @Test("deleted sets externalDelete")
    func deletedSetsConflict() async {
        let existing = Contact(localContactsID: "lcid-1", givenName: "Alice")
        let store = ContactsStore()
        store.contacts = [existing]
        await store.applyChangeEvents([.init(kind: .deleted(localContactsID: "lcid-1"))])
        if case .externalDelete = existing.conflictState {
            // ok
        } else {
            Issue.record("expected externalDelete")
        }
    }

    @Test("empty event list is a no-op")
    func emptyEvents() async {
        let existing = Contact(localContactsID: "lcid-1", givenName: "Alice")
        let store = ContactsStore()
        store.contacts = [existing]
        await store.applyChangeEvents([])
        #expect(existing.conflictState == nil)
        #expect(store.contacts.count == 1)
    }

    @Test("unknown localContactsID on update/delete is ignored")
    func unknownIDIgnored() async {
        let existing = Contact(localContactsID: "lcid-1", givenName: "Alice")
        let store = ContactsStore()
        store.contacts = [existing]
        await store.applyChangeEvents([
            .init(kind: .updated(contactData: .stub(localContactsID: "missing"))),
            .init(kind: .deleted(localContactsID: "missing")),
        ])
        #expect(existing.conflictState == nil)
        #expect(store.contacts.count == 1)
    }

    @Test("added imports a new contact and claims the CN identifier")
    func addedImports() async throws {
        let folder = try makeTempFolder()
        defer { try? FileManager.default.removeItem(at: folder) }

        let defaults = makeDefaults()
        let fake = FakeCNContactStore()
        let sync = CNSyncService(store: fake, defaults: defaults)
        let store = ContactsStore(syncService: sync)
        store.folderURL = folder

        let data = CNSyncService.CNContactData.stub(
            cnIdentifier: "cn-imported",
            localContactsID: "",
            givenName: "Carol"
        )
        await store.applyChangeEvents([.init(kind: .added(contactData: data))])

        #expect(store.contacts.count == 1)
        #expect(store.contacts.first?.givenName == "Carol")
        let mapping = await sync.idMapping()
        #expect(mapping.values.contains("cn-imported"))
    }
}
