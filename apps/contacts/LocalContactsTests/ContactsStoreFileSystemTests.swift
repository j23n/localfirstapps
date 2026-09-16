import Foundation
import Testing
@testable import LocalContacts

/// Integration tests that exercise `ContactsStore` against a real folder on
/// disk in a per-test temporary directory. Stays away from `setFolder()` /
/// bookmarks — that path goes through UserDefaults and security-scoped
/// resources, neither of which we want to touch in tests. We assign
/// `folderURL` directly and let `loadContacts`/`save`/`delete` work on
/// the temp folder.
@MainActor
@Suite("ContactsStore — file system")
struct ContactsStoreFileSystemTests {

    // MARK: - Fixture helpers

    private func makeTempFolder() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("LocalContactsTests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    private func cleanup(_ url: URL) {
        try? FileManager.default.removeItem(at: url)
    }

    private func writeFixture(_ vcard: String, named name: String, in folder: URL) throws {
        let url = folder.appendingPathComponent(name)
        try vcard.data(using: .utf8)!.write(to: url, options: .atomic)
    }

    private func readFile(_ name: String, in folder: URL) throws -> String {
        let url = folder.appendingPathComponent(name)
        return try String(contentsOf: url, encoding: .utf8)
    }

    private func makeStore(folder: URL) -> ContactsStore {
        let store = ContactsStore(deviceId: "test")
        store.folderURL = folder
        return store
    }

    // MARK: - Load

    @Test("loadContacts reads all .vcf files from folder")
    func loadAll() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Alice\r\nX-LOCALCONTACTS-ID:lcid-1\r\nEND:VCARD\r\n",
            named: "alice.vcf", in: folder
        )
        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Bob\r\nX-LOCALCONTACTS-ID:lcid-2\r\nEND:VCARD\r\n",
            named: "bob.vcf", in: folder
        )

        let store = makeStore(folder: folder)
        await store.loadContacts()

        #expect(store.contacts.count == 2)
        #expect(Set(store.contacts.map(\.fullName)) == Set(["Alice", "Bob"]))
        #expect(store.lastSyncedAt != nil)
    }

    @Test("loadContacts excludes Syncthing conflict copies")
    func loadExcludesConflictCopies() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Alice\r\nX-LOCALCONTACTS-ID:lcid-1\r\nEND:VCARD\r\n",
            named: "alice.vcf", in: folder
        )
        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Alice Conflict\r\nX-LOCALCONTACTS-ID:lcid-conflict\r\nEND:VCARD\r\n",
            named: "alice.sync-conflict-20200901-120000-PHONE01.vcf", in: folder
        )

        let store = makeStore(folder: folder)
        await store.loadContacts()

        #expect(store.contacts.count == 1)
        #expect(store.contacts.first?.fullName == "Alice")
        #expect(store.contacts.first?.fileName == "alice.vcf")
        #expect(store.hasSyncConflictGroups)
        #expect(store.syncConflictGroups.first?.id == "alice.vcf")
        #expect(store.syncConflictGroups.first?.disposition == .choice)
    }

    @Test("resolveSyncGroup auto-merges disjoint fields and drops the copy")
    func resolveSyncGroupAuto() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Alice\r\nX-LOCALCONTACTS-ID:lcid-1\r\nTEL;TYPE=cell:1\r\nEND:VCARD\r\n",
            named: "alice.vcf", in: folder
        )
        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Alice\r\nX-LOCALCONTACTS-ID:lcid-1\r\nEMAIL;TYPE=home:a@b.com\r\nEND:VCARD\r\n",
            named: "alice.sync-conflict-20200901-120000-PHONE01.vcf", in: folder
        )

        let store = makeStore(folder: folder)
        await store.loadContacts()
        #expect(store.hasSyncConflictGroups)
        #expect(store.syncConflictGroups.first?.trailing == "auto")
        #expect(store.syncConflictGroups.first?.disposition == .auto)

        try await store.resolveSyncGroup(canonicalName: "alice.vcf")
        #expect(!store.hasSyncConflictGroups)
        #expect(store.contacts.count == 1)
        let onDisk = try readFile("alice.vcf", in: folder)
        #expect(onDisk.contains("TYPE=cell:1"))
        #expect(onDisk.contains("a@b.com"))
        let copyExists = FileManager.default.fileExists(
            atPath: folder.appendingPathComponent(
                "alice.sync-conflict-20200901-120000-PHONE01.vcf"
            ).path
        )
        #expect(!copyExists)
    }

    @Test("loadContacts ignores non-vcf files")
    func loadIgnoresOtherFiles() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Alice\r\nX-LOCALCONTACTS-ID:lcid-1\r\nEND:VCARD\r\n",
            named: "alice.vcf", in: folder
        )
        try writeFixture("not a contact", named: "notes.txt", in: folder)

        let store = makeStore(folder: folder)
        await store.loadContacts()
        #expect(store.contacts.count == 1)
    }

    @Test("loadContacts skips when folderURL is nil")
    func loadWithoutFolder() async {
        let store = ContactsStore()
        await store.loadContacts()
        #expect(store.contacts.isEmpty)
    }

    // MARK: - ID migration

    @Test("loadContacts assigns a UUID when X-LOCALCONTACTS-ID is missing, and persists it")
    func idMigration() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Alice\r\nEND:VCARD\r\n",
            named: "alice.vcf", in: folder
        )

        let store = makeStore(folder: folder)
        await store.loadContacts()

        let firstID = try #require(store.contacts.first?.localContactsID)
        #expect(!firstID.isEmpty)

        // The on-disk file should now contain the assigned ID.
        let onDisk = try readFile("alice.vcf", in: folder)
        #expect(onDisk.contains("X-LOCALCONTACTS-ID:\(firstID)"))

        // Reloading must not regenerate the ID.
        await store.loadContacts()
        #expect(store.contacts.first?.localContactsID == firstID)
    }

    // MARK: - Save: one-file-per-contact layout

    @Test("save writes a new .vcf file in oneFilePerContact layout")
    func saveCreatesNewFile() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        let store = makeStore(folder: folder)
        let alice = Contact(localContactsID: "lcid-a", familyName: "Wonder", givenName: "Alice")
        try await store.save(alice)

        #expect(alice.fileName == "alice-wonder.vcf")
        let onDisk = try readFile("alice-wonder.vcf", in: folder)
        #expect(onDisk.contains("FN:Alice Wonder"))
        #expect(store.contacts.count == 1)
        let logDir = folder.appendingPathComponent(".contacts/log/test")
        let logs = try FileManager.default.contentsOfDirectory(atPath: logDir.path)
        #expect(logs.contains { $0.hasSuffix(".ndjson") })
    }

    @Test("save appends -1 when filename collides on disk")
    func saveCollisionSuffix() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        // Pre-populate a file that doesn't belong to any tracked contact.
        try writeFixture("dummy", named: "alice-wonder.vcf", in: folder)

        let store = makeStore(folder: folder)
        let alice = Contact(localContactsID: "lcid-a", familyName: "Wonder", givenName: "Alice")
        try await store.save(alice)

        #expect(alice.fileName == "alice-wonder-1.vcf")
        // Original file untouched.
        let onDisk = try readFile("alice-wonder.vcf", in: folder)
        #expect(onDisk == "dummy")
    }

    // MARK: - Save: single-file layout

    @Test("save in singleFile layout appends to the shared file (no new file)")
    func saveInSingleFileLayout() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        // Seed with two contacts in one file → singleFile layout.
        let bundle = """
        BEGIN:VCARD\r
        VERSION:3.0\r
        X-LOCALCONTACTS-ID:lcid-1\r
        FN:Alice\r
        END:VCARD\r
        BEGIN:VCARD\r
        VERSION:3.0\r
        X-LOCALCONTACTS-ID:lcid-2\r
        FN:Bob\r
        END:VCARD\r
        """
        try writeFixture(bundle, named: "everyone.vcf", in: folder)

        let store = makeStore(folder: folder)
        await store.loadContacts()
        #expect(store.layoutMode == .singleFile(fileName: "everyone.vcf"))

        let carol = Contact(localContactsID: "lcid-3", givenName: "Carol")
        try await store.save(carol)

        // Carol must be assigned the bundle filename, not a new one.
        #expect(carol.fileName == "everyone.vcf")

        // Folder must still contain only one .vcf file.
        let files = try FileManager.default.contentsOfDirectory(atPath: folder.path)
            .filter { $0.hasSuffix(".vcf") }
        #expect(files == ["everyone.vcf"])

        // The single file now contains 3 BEGIN:VCARD blocks.
        let onDisk = try readFile("everyone.vcf", in: folder)
        let blockCount = onDisk.components(separatedBy: "BEGIN:VCARD").count - 1
        #expect(blockCount == 3)
    }

    // MARK: - Save preserves siblings in multi-vCard files

    @Test("editing one contact in a multi-vCard file preserves the others (regression)")
    func saveDoesNotDropSiblings() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        let bundle = """
        BEGIN:VCARD\r
        VERSION:3.0\r
        X-LOCALCONTACTS-ID:lcid-A\r
        FN:Alice\r
        END:VCARD\r
        BEGIN:VCARD\r
        VERSION:3.0\r
        X-LOCALCONTACTS-ID:lcid-B\r
        FN:Bob\r
        END:VCARD\r
        BEGIN:VCARD\r
        VERSION:3.0\r
        X-LOCALCONTACTS-ID:lcid-C\r
        FN:Carol\r
        END:VCARD\r
        """
        try writeFixture(bundle, named: "team.vcf", in: folder)

        let store = makeStore(folder: folder)
        await store.loadContacts()
        #expect(store.contacts.count == 3)

        // Edit Bob.
        let bob = try #require(store.contacts.first { $0.localContactsID == "lcid-B" })
        bob.fullName = "Robert"
        try await store.save(bob)

        // File should still contain Alice + Carol + the updated Bob.
        let onDisk = try readFile("team.vcf", in: folder)
        #expect(onDisk.contains("FN:Alice"))
        #expect(onDisk.contains("FN:Robert"))
        #expect(onDisk.contains("FN:Carol"))
        #expect(!onDisk.contains("FN:Bob\r\n"))

        // And in-memory state holds 3 contacts (no duplicates, no losses).
        await store.loadContacts()
        #expect(store.contacts.count == 3)
    }

    @Test("save re-reads siblings from disk so an out-of-band edit survives")
    func saveReReadsSiblingsFromDisk() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        let bundle = """
        BEGIN:VCARD\r
        VERSION:3.0\r
        X-LOCALCONTACTS-ID:lcid-A\r
        FN:Alice\r
        END:VCARD\r
        BEGIN:VCARD\r
        VERSION:3.0\r
        X-LOCALCONTACTS-ID:lcid-B\r
        FN:Bob\r
        END:VCARD\r
        """
        try writeFixture(bundle, named: "pair.vcf", in: folder)

        let store = makeStore(folder: folder)
        await store.loadContacts()
        #expect(store.contacts.count == 2)

        // Simulate Syncthing rewriting Alice on disk without updating memory.
        let stale = """
        BEGIN:VCARD\r
        VERSION:3.0\r
        X-LOCALCONTACTS-ID:lcid-A\r
        FN:Alicia\r
        END:VCARD\r
        BEGIN:VCARD\r
        VERSION:3.0\r
        X-LOCALCONTACTS-ID:lcid-B\r
        FN:Bob\r
        END:VCARD\r
        """
        try writeFixture(stale, named: "pair.vcf", in: folder)

        let bob = try #require(store.contacts.first { $0.localContactsID == "lcid-B" })
        bob.fullName = "Robert"
        try await store.save(bob)

        await store.loadContacts()
        let names = Set(store.contacts.map(\.fullName))
        #expect(names == Set(["Alicia", "Robert"]))
        #expect(store.contacts.count == 2)
    }

    // MARK: - Delete

    @Test("delete removes a per-file contact and its file")
    func deleteRemovesFile() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nX-LOCALCONTACTS-ID:lcid-A\r\nFN:Alice\r\nEND:VCARD\r\n",
            named: "alice.vcf", in: folder
        )
        let store = makeStore(folder: folder)
        await store.loadContacts()
        let alice = try #require(store.contacts.first)

        try await store.delete(alice)

        #expect(store.contacts.isEmpty)
        let exists = FileManager.default.fileExists(
            atPath: folder.appendingPathComponent("alice.vcf").path
        )
        #expect(!exists)
    }

    @Test("delete from a multi-vCard file rewrites the file without removing it")
    func deleteFromMultiCardFile() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        let bundle = """
        BEGIN:VCARD\r
        VERSION:3.0\r
        X-LOCALCONTACTS-ID:lcid-A\r
        FN:Alice\r
        END:VCARD\r
        BEGIN:VCARD\r
        VERSION:3.0\r
        X-LOCALCONTACTS-ID:lcid-B\r
        FN:Bob\r
        END:VCARD\r
        """
        try writeFixture(bundle, named: "pair.vcf", in: folder)

        let store = makeStore(folder: folder)
        await store.loadContacts()
        let bob = try #require(store.contacts.first { $0.localContactsID == "lcid-B" })
        try await store.delete(bob)

        let exists = FileManager.default.fileExists(
            atPath: folder.appendingPathComponent("pair.vcf").path
        )
        #expect(exists)
        let onDisk = try readFile("pair.vcf", in: folder)
        #expect(onDisk.contains("FN:Alice"))
        #expect(!onDisk.contains("FN:Bob"))
    }

    // MARK: - Bulk delete

    @Test("deleteMultiple removes survivors only and rewrites each touched file once")
    func bulkDeleteCollapsesWrites() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        let team = """
        BEGIN:VCARD\r
        VERSION:3.0\r
        X-LOCALCONTACTS-ID:lcid-A\r
        FN:Alice\r
        END:VCARD\r
        BEGIN:VCARD\r
        VERSION:3.0\r
        X-LOCALCONTACTS-ID:lcid-B\r
        FN:Bob\r
        END:VCARD\r
        BEGIN:VCARD\r
        VERSION:3.0\r
        X-LOCALCONTACTS-ID:lcid-C\r
        FN:Carol\r
        END:VCARD\r
        """
        try writeFixture(team, named: "team.vcf", in: folder)
        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nX-LOCALCONTACTS-ID:lcid-D\r\nFN:Dave\r\nEND:VCARD\r\n",
            named: "dave.vcf", in: folder
        )

        let store = makeStore(folder: folder)
        await store.loadContacts()
        #expect(store.contacts.count == 4)

        try await store.deleteMultiple(["lcid-A", "lcid-C", "lcid-D"])

        // Bob survives in team.vcf; team file rewritten without A/C; dave.vcf removed.
        let team2 = try readFile("team.vcf", in: folder)
        #expect(team2.contains("FN:Bob"))
        #expect(!team2.contains("FN:Alice"))
        #expect(!team2.contains("FN:Carol"))

        let daveExists = FileManager.default.fileExists(
            atPath: folder.appendingPathComponent("dave.vcf").path
        )
        #expect(!daveExists)
        #expect(store.contacts.map(\.localContactsID) == ["lcid-B"])
    }

    // MARK: - Tag rename / delete

    @Test("renameTag rewrites every touched file and dedups when target already present")
    func renameTagDedupes() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nX-LOCALCONTACTS-ID:lcid-A\r\nFN:Alice\r\nCATEGORIES:friends,vip\r\nEND:VCARD\r\n",
            named: "alice.vcf", in: folder
        )
        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nX-LOCALCONTACTS-ID:lcid-B\r\nFN:Bob\r\nCATEGORIES:friends\r\nEND:VCARD\r\n",
            named: "bob.vcf", in: folder
        )

        let store = makeStore(folder: folder)
        await store.loadContacts()

        // Rename "friends" → "vip" on Alice should dedup; on Bob should rename outright.
        try await store.renameTag("friends", to: "vip")

        let alice = try #require(store.contacts.first { $0.localContactsID == "lcid-A" })
        let bob = try #require(store.contacts.first { $0.localContactsID == "lcid-B" })
        #expect(alice.categories == ["vip"])
        #expect(bob.categories == ["vip"])

        let onDiskAlice = try readFile("alice.vcf", in: folder)
        #expect(onDiskAlice.contains("CATEGORIES:vip"))
        #expect(!onDiskAlice.contains("friends"))
    }

    @Test("renameTag updates selectedTag if it referred to the old name")
    func renameTagUpdatesSelection() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nX-LOCALCONTACTS-ID:lcid-A\r\nFN:Alice\r\nCATEGORIES:friends\r\nEND:VCARD\r\n",
            named: "alice.vcf", in: folder
        )

        let store = makeStore(folder: folder)
        await store.loadContacts()
        store.selectedTag = "friends"

        try await store.renameTag("friends", to: "buddies")
        #expect(store.selectedTag == "buddies")
    }

    @Test("deleteTag removes the tag from all contacts and clears selection")
    func deleteTag() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nX-LOCALCONTACTS-ID:lcid-A\r\nFN:Alice\r\nCATEGORIES:friends,vip\r\nEND:VCARD\r\n",
            named: "alice.vcf", in: folder
        )

        let store = makeStore(folder: folder)
        await store.loadContacts()
        store.selectedTag = "friends"

        try await store.deleteTag("friends")

        let alice = try #require(store.contacts.first)
        #expect(alice.categories == ["vip"])
        #expect(store.selectedTag == nil)
    }

    // MARK: - assignTag

    @Test("assignTag adds the tag without duplicating it")
    func assignTagNoDuplicate() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nX-LOCALCONTACTS-ID:lcid-A\r\nFN:Alice\r\nCATEGORIES:vip\r\nEND:VCARD\r\n",
            named: "alice.vcf", in: folder
        )
        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nX-LOCALCONTACTS-ID:lcid-B\r\nFN:Bob\r\nEND:VCARD\r\n",
            named: "bob.vcf", in: folder
        )

        let store = makeStore(folder: folder)
        await store.loadContacts()

        try await store.assignTag("vip", to: ["lcid-A", "lcid-B"])

        let alice = try #require(store.contacts.first { $0.localContactsID == "lcid-A" })
        let bob = try #require(store.contacts.first { $0.localContactsID == "lcid-B" })
        #expect(alice.categories == ["vip"])      // not duplicated
        #expect(bob.categories == ["vip"])

        // Bob's file must actually be rewritten with the new tag — alice was
        // a no-op (already tagged) so we don't bother re-checking her file.
        let onDiskBob = try readFile("bob.vcf", in: folder)
        #expect(onDiskBob.contains("CATEGORIES:vip"))
    }

    @Test("save throws noFolder when folderURL is nil")
    func saveWithoutFolder() async {
        let store = ContactsStore()
        let contact = Contact(givenName: "Alice")
        await #expect(throws: ContactsStoreError.noFolder) {
            try await store.save(contact)
        }
    }

    @Test("mixed layout: editing a solo file does not rewrite the shared file")
    func mixedLayoutSoloEditLeavesSharedUntouched() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        let shared = """
        BEGIN:VCARD\r
        VERSION:3.0\r
        X-LOCALCONTACTS-ID:lcid-A\r
        FN:Alice\r
        END:VCARD\r
        BEGIN:VCARD\r
        VERSION:3.0\r
        X-LOCALCONTACTS-ID:lcid-B\r
        FN:Bob\r
        END:VCARD\r
        """
        try writeFixture(shared, named: "shared.vcf", in: folder)
        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nX-LOCALCONTACTS-ID:lcid-C\r\nFN:Carol\r\nEND:VCARD\r\n",
            named: "carol.vcf", in: folder
        )

        let store = makeStore(folder: folder)
        await store.loadContacts()
        #expect(store.layoutMode == .mixed)

        let beforeShared = try readFile("shared.vcf", in: folder)
        let carol = try #require(store.contacts.first { $0.localContactsID == "lcid-C" })
        carol.fullName = "Caroline"
        try await store.save(carol)

        #expect(try readFile("shared.vcf", in: folder) == beforeShared)
        #expect(try readFile("carol.vcf", in: folder).contains("FN:Caroline"))
    }

    @Test("save refuses a corrupt multi-vCard without replacing it from stale memory")
    func saveCorruptFileIsRefused() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        let bundle = """
        BEGIN:VCARD\r
        VERSION:3.0\r
        X-LOCALCONTACTS-ID:lcid-A\r
        FN:Alice\r
        END:VCARD\r
        BEGIN:VCARD\r
        VERSION:3.0\r
        X-LOCALCONTACTS-ID:lcid-B\r
        FN:Bob\r
        END:VCARD\r
        """
        try writeFixture(bundle, named: "pair.vcf", in: folder)

        let store = makeStore(folder: folder)
        await store.loadContacts()
        #expect(store.contacts.count == 2)

        try writeFixture("not-a-vcard", named: "pair.vcf", in: folder)

        let bob = try #require(store.contacts.first { $0.localContactsID == "lcid-B" })
        bob.fullName = "Robert"
        await #expect(throws: ContactsError.NotFound) {
            try await store.save(bob)
        }

        #expect(try readFile("pair.vcf", in: folder) == "not-a-vcard")
        #expect(store.contacts.count == 2)
    }

    @Test("searchHits, fieldRows, and export bind core display records")
    func coreDisplayAndExport() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Alice\r\nX-LOCALCONTACTS-ID:lcid-1\r\nEMAIL;TYPE=home:a@b.com\r\nEND:VCARD\r\n",
            named: "alice.vcf",
            in: folder
        )

        let store = makeStore(folder: folder)
        await store.loadContacts()
        store.searchText = "b.com"

        let hits = store.searchHits
        #expect(hits.count == 1)
        #expect(hits[0].title == "Alice")
        #expect(hits[0].subtitle == "a@b.com")
        #expect(hits[0].trailing == "Email")

        let fields = store.fieldRows(id: "lcid-1")
        #expect(fields.contains { $0.label == "Name" && $0.value == "Alice" })
        #expect(fields.contains { $0.id == "email:0" && $0.value == "a@b.com" })

        let exported = try store.exportVCardText(id: "lcid-1")
        #expect(exported.contains("FN:Alice"))
        #expect(exported.contains("a@b.com"))
    }

    @Test("save(draft) writes through ContactEditDraft and keeps a custom FN")
    func saveDraftPreservesCustomFullName() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Lady Lovelace\r\nN:Lovelace;Ada;;;\r\nX-LOCALCONTACTS-ID:lcid-1\r\nEND:VCARD\r\n",
            named: "ada.vcf",
            in: folder
        )

        let store = makeStore(folder: folder)
        await store.loadContacts()
        var draft = try store.contactEditDraft(id: "lcid-1")
        #expect(draft.fullName == "Lady Lovelace")
        draft.note = "wrote the engine"
        let saved = try await store.save(draft)
        #expect(saved.fullName == "Lady Lovelace")
        #expect(saved.note == "wrote the engine")
        let onDisk = try readFile("ada.vcf", in: folder)
        #expect(onDisk.contains("FN:Lady Lovelace"))
        #expect(onDisk.contains("N:Lovelace;Ada;"))
        #expect(onDisk.contains("wrote the engine"))
    }

    @Test("conflictPreview lists discarded copies for an auto group")
    func conflictPreviewAuto() async throws {
        let folder = try makeTempFolder()
        defer { cleanup(folder) }

        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Alice\r\nX-LOCALCONTACTS-ID:lcid-1\r\nTEL;TYPE=cell:1\r\nEND:VCARD\r\n",
            named: "alice.vcf",
            in: folder
        )
        try writeFixture(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Alice\r\nX-LOCALCONTACTS-ID:lcid-1\r\nEMAIL;TYPE=home:a@b.com\r\nEND:VCARD\r\n",
            named: "alice.sync-conflict-20200901-120000-PHONE01.vcf",
            in: folder
        )

        let store = makeStore(folder: folder)
        await store.loadContacts()
        let preview = try store.conflictPreview(canonicalName: "alice.vcf")
        #expect(preview.kind == .auto)
        #expect(preview.fields.isEmpty)
        #expect(preview.discarded.contains { $0.contains("sync-conflict") })
        #expect(preview.mergedFields.contains { $0.value == "a@b.com" })
    }
}
