import Contacts
import Foundation
import Testing
@testable import LocalContacts

@Suite("CNSyncService — store-backed")
struct CNSyncServiceStoreTests {

    private func makeService(
        store: FakeCNContactStore = FakeCNContactStore(),
        defaults: UserDefaults = makeIsolatedDefaults()
    ) -> (CNSyncService, FakeCNContactStore, UserDefaults) {
        (CNSyncService(store: store, defaults: defaults), store, defaults)
    }

    @Test("pushContact first time executes and records a mapping")
    func pushFirstTime() async throws {
        let fake = FakeCNContactStore()
        fake.seedLocalContactsGroup()
        let (svc, store, _) = makeService(store: fake)
        let contact = Contact(localContactsID: "lcid-new", givenName: "Alice", familyName: "Wonder")
        try await svc.pushContact(contact)
        #expect(store.executeCount >= 1)
        let mapping = await svc.idMapping()
        #expect(mapping["lcid-new"] != nil)
        #expect(await svc.storedHistoryToken() != nil)
    }

    @Test("pushContact with an existing mapping updates rather than failing")
    func pushExisting() async throws {
        let fake = FakeCNContactStore()
        let group = fake.seedLocalContactsGroup()
        let cn = makeCNContact()
        fake.add(cn, toGroup: group.identifier)
        let (svc, store, _) = makeService(store: fake)
        await svc.setIDMapping(["lcid-1": cn.identifier])
        let contact = Contact(localContactsID: "lcid-1", givenName: "Alicia", familyName: "Wonder")
        try await svc.pushContact(contact)
        #expect(store.executeCount >= 1)
        let mapping = await svc.idMapping()
        #expect(mapping["lcid-1"] == cn.identifier)
    }

    @Test("pushContact without full access does not execute")
    func pushDenied() async throws {
        let fake = FakeCNContactStore()
        fake.authorizationStatus = .denied
        let (svc, store, _) = makeService(store: fake)
        try await svc.pushContact(Contact(givenName: "Alice"))
        #expect(store.executeCount == 0)
    }

    @Test("deleteContact mapped contact executes and removes mapping")
    func deleteMapped() async throws {
        let fake = FakeCNContactStore()
        let group = fake.seedLocalContactsGroup()
        let cn = makeCNContact()
        fake.add(cn, toGroup: group.identifier)
        let (svc, store, _) = makeService(store: fake)
        await svc.setIDMapping(["lcid-1": cn.identifier])
        try await svc.deleteContact(localContactsID: "lcid-1")
        #expect(store.executeCount >= 1)
        let mapping = await svc.idMapping()
        #expect(mapping["lcid-1"] == nil)
    }

    @Test("deleteContact unmapped is a no-op")
    func deleteUnmapped() async throws {
        let fake = FakeCNContactStore()
        fake.seedLocalContactsGroup()
        let (svc, store, _) = makeService(store: fake)
        try await svc.deleteContact(localContactsID: "missing")
        #expect(store.executeCount == 0)
    }

    @Test("fetchChanges with equal tokens returns empty and does not rewrite the token")
    func fetchTokensEqual() async {
        let fake = FakeCNContactStore()
        fake.currentHistoryToken = Data([0xAB])
        let (svc, _, _) = makeService(store: fake)
        await svc.setStoredHistoryToken(Data([0xAB]))
        let events = await svc.fetchChanges(localContacts: [])
        #expect(events.isEmpty)
        #expect(await svc.storedHistoryToken() == Data([0xAB]))
    }

    @Test("fetchChanges reports deleted when a mapped CN is missing from the group")
    func fetchDeleted() async {
        let fake = FakeCNContactStore()
        fake.seedLocalContactsGroup()
        fake.currentHistoryToken = Data([0x02])
        let (svc, _, _) = makeService(store: fake)
        await svc.setIDMapping(["lcid-1": "cn-gone"])
        let local = Contact(localContactsID: "lcid-1", givenName: "Alice")
        let events = await svc.fetchChanges(localContacts: [local])
        #expect(events.count == 1)
        if case .deleted(let id) = events.first?.kind {
            #expect(id == "lcid-1")
        } else {
            Issue.record("expected deleted event")
        }
    }

    @Test("fetchChanges reports updated with stable vCard labels")
    func fetchUpdatedLabels() async {
        let fake = FakeCNContactStore()
        let group = fake.seedLocalContactsGroup()
        let cn = makeCNContact(given: "Alicia", phones: ["+15559999"], phoneLabel: CNLabelPhoneNumberMobile)
        fake.add(cn, toGroup: group.identifier)
        fake.currentHistoryToken = Data([0x03])
        let (svc, _, _) = makeService(store: fake)
        await svc.setIDMapping(["lcid-1": cn.identifier])
        let local = Contact(
            localContactsID: "lcid-1",
            givenName: "Alice",
            familyName: "Wonder",
            phoneNumbers: [LabeledValue(label: "mobile", value: "+15551111")]
        )
        let events = await svc.fetchChanges(localContacts: [local])
        guard case .updated(let data) = events.first?.kind else {
            Issue.record("expected updated event")
            return
        }
        #expect(data.givenName == "Alicia")
        #expect(data.phoneNumbers.first?.label == "mobile")
        #expect(data.phoneNumbers.first?.label != CNLabelPhoneNumberMobile)
    }

    @Test("fetchChanges reports added for a CN not in the mapping")
    func fetchAdded() async {
        let fake = FakeCNContactStore()
        let group = fake.seedLocalContactsGroup()
        let cn = makeCNContact(given: "Carol")
        fake.add(cn, toGroup: group.identifier)
        fake.currentHistoryToken = Data([0x04])
        let (svc, _, _) = makeService(store: fake)
        let events = await svc.fetchChanges(localContacts: [])
        guard case .added(let data) = events.first?.kind else {
            Issue.record("expected added event")
            return
        }
        #expect(data.cnIdentifier == cn.identifier)
        #expect(data.givenName == "Carol")
    }

    @Test("fetchChanges on store error returns empty and leaves the token unchanged")
    func fetchErrorKeepsToken() async {
        let fake = FakeCNContactStore()
        fake.seedLocalContactsGroup()
        fake.fetchError = FakeStoreError.fetchFailed
        fake.currentHistoryToken = Data([0xFF])
        let (svc, _, _) = makeService(store: fake)
        await svc.setStoredHistoryToken(Data([0xAA]))
        let events = await svc.fetchChanges(localContacts: [Contact(localContactsID: "x")])
        #expect(events.isEmpty)
        #expect(await svc.storedHistoryToken() == Data([0xAA]))
    }

    @Test("fullReconciliation clears mapping then re-adds every contact")
    func fullReconciliation() async throws {
        let fake = FakeCNContactStore()
        let group = fake.seedLocalContactsGroup()
        fake.add(makeCNContact(), toGroup: group.identifier)
        let (svc, store, _) = makeService(store: fake)
        await svc.setIDMapping(["old": "cn-old"])
        let contacts = [
            Contact(localContactsID: "lcid-a", givenName: "Alice"),
            Contact(localContactsID: "lcid-b", givenName: "Bob"),
        ]
        try await svc.fullReconciliation(contacts: contacts)
        #expect(store.executeCount >= 1 + contacts.count)
        let mapping = await svc.idMapping()
        #expect(mapping["old"] == nil)
        #expect(mapping["lcid-a"] != nil)
        #expect(mapping["lcid-b"] != nil)
    }

    @Test("claimCNContact records the mapping used by a later push")
    func claimThenPush() async throws {
        let fake = FakeCNContactStore()
        let group = fake.seedLocalContactsGroup()
        let cn = makeCNContact()
        fake.add(cn, toGroup: group.identifier)
        let (svc, store, _) = makeService(store: fake)
        await svc.claimCNContact(cnIdentifier: cn.identifier, forLocalContactsID: "lcid-1")
        #expect(await svc.idMapping()["lcid-1"] == cn.identifier)
        let before = store.executeCount
        try await svc.pushContact(Contact(localContactsID: "lcid-1", givenName: "Alice"))
        #expect(store.executeCount > before)
        #expect(await svc.idMapping()["lcid-1"] == cn.identifier)
    }

    @Test("limited access is not full access")
    func limitedIsNotFull() async {
        let fake = FakeCNContactStore()
        fake.authorizationStatus = .limited
        let (svc, store, _) = makeService(store: fake)
        #expect(await svc.hasFullAccess == false)
        try? await svc.pushContact(Contact(givenName: "Alice"))
        #expect(store.executeCount == 0)
        #expect(await svc.fetchChanges(localContacts: []).isEmpty)
    }
}
