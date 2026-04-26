import Foundation
import Testing
@testable import LocalContacts

@MainActor
@Suite("ContactsStore — computed properties")
struct ContactsStoreComputedTests {

    private func makeStore(_ contacts: [Contact] = []) -> ContactsStore {
        let store = ContactsStore()
        store.contacts = contacts
        return store
    }

    private func contact(
        lcid: String = UUID().uuidString,
        fileName: String = "",
        given: String = "",
        family: String = "",
        org: String = "",
        title: String = "",
        phones: [String] = [],
        emails: [String] = [],
        categories: [String] = [],
        conflict: ConflictState? = nil
    ) -> Contact {
        Contact(
            localContactsID: lcid,
            fileName: fileName,
            familyName: family,
            givenName: given,
            organization: org,
            jobTitle: title,
            phoneNumbers: phones.map { LabeledValue(label: "mobile", value: $0) },
            emailAddresses: emails.map { LabeledValue(label: "home", value: $0) },
            categories: categories,
            conflictState: conflict
        )
    }

    // MARK: - allTags

    @Test("allTags counts occurrences across contacts")
    func allTagsCounts() {
        let store = makeStore([
            contact(given: "A", categories: ["friends", "work"]),
            contact(given: "B", categories: ["friends"]),
            contact(given: "C", categories: ["work", "vip"]),
        ])
        let tags = Dictionary(uniqueKeysWithValues: store.allTags.map { ($0.tag, $0.count) })
        #expect(tags["friends"] == 2)
        #expect(tags["work"] == 2)
        #expect(tags["vip"] == 1)
    }

    @Test("allTags is sorted alphabetically")
    func allTagsSorted() {
        let store = makeStore([
            contact(given: "A", categories: ["zeta", "alpha", "mu"]),
        ])
        #expect(store.allTags.map(\.tag) == ["alpha", "mu", "zeta"])
    }

    @Test("allTags is empty when no contacts have categories")
    func allTagsEmpty() {
        let store = makeStore([contact(given: "A")])
        #expect(store.allTags.isEmpty)
    }

    // MARK: - filteredContacts

    @Test("filteredContacts: empty search returns all, sorted by displayName")
    func filterAll() {
        let store = makeStore([
            contact(given: "Charlie"),
            contact(given: "Alice"),
            contact(given: "Bob"),
        ])
        #expect(store.filteredContacts.map(\.givenName) == ["Alice", "Bob", "Charlie"])
    }

    @Test("filteredContacts: search by name (case-insensitive substring)")
    func searchName() {
        let store = makeStore([
            contact(given: "Alice"),
            contact(given: "Bob"),
            contact(given: "Alfred"),
        ])
        store.searchText = "AL"
        let names = Set(store.filteredContacts.map(\.givenName))
        #expect(names == Set(["Alice", "Alfred"]))
    }

    @Test("filteredContacts: search matches organization")
    func searchOrg() {
        let store = makeStore([
            contact(given: "Alice", org: "Acme Inc"),
            contact(given: "Bob", org: "Other"),
        ])
        store.searchText = "acme"
        #expect(store.filteredContacts.map(\.givenName) == ["Alice"])
    }

    @Test("filteredContacts: search matches phone substring")
    func searchPhone() {
        let store = makeStore([
            contact(given: "Alice", phones: ["+1-555-1234"]),
            contact(given: "Bob", phones: ["+1-555-9999"]),
        ])
        store.searchText = "9999"
        #expect(store.filteredContacts.map(\.givenName) == ["Bob"])
    }

    @Test("filteredContacts: search matches email (case-insensitive)")
    func searchEmail() {
        let store = makeStore([
            contact(given: "Alice", emails: ["alice@Example.COM"]),
            contact(given: "Bob", emails: ["bob@nowhere.com"]),
        ])
        store.searchText = "example"
        #expect(store.filteredContacts.map(\.givenName) == ["Alice"])
    }

    @Test("filteredContacts: tag filter narrows results")
    func filterByTag() {
        let store = makeStore([
            contact(given: "Alice", categories: ["friends"]),
            contact(given: "Bob", categories: ["work"]),
            contact(given: "Carol", categories: ["friends", "work"]),
        ])
        store.selectedTag = "friends"
        let names = Set(store.filteredContacts.map(\.givenName))
        #expect(names == Set(["Alice", "Carol"]))
    }

    @Test("filteredContacts: showConflictsOnly overrides selectedTag")
    func conflictsOverrideTag() {
        let conflicted = contact(
            given: "Bob",
            conflict: .externalDelete
        )
        let store = makeStore([
            contact(given: "Alice", categories: ["friends"]),
            conflicted,
        ])
        store.selectedTag = "friends"
        store.showConflictsOnly = true
        #expect(store.filteredContacts.map(\.givenName) == ["Bob"])
    }

    @Test("filteredContacts: locale-aware case-insensitive sort")
    func sortLocaleAware() {
        let store = makeStore([
            contact(given: "ábel"),
            contact(given: "abel"),
            contact(given: "Charlie"),
        ])
        // 'ábel' and 'abel' should sort adjacent and both before 'Charlie'.
        let names = store.filteredContacts.map(\.givenName)
        #expect(names.last == "Charlie")
        #expect(Set(names.prefix(2)) == Set(["abel", "ábel"]))
    }

    // MARK: - groupedContacts

    @Test("groupedContacts groups by sortLetter and sorts A→Z then #")
    func grouped() {
        let store = makeStore([
            contact(given: "Alice", family: "Wonder"),
            contact(given: "Bob", family: "Builder"),
            contact(given: "Anne", family: "Apple"),
            contact(given: "1Numeric"),
        ])
        let result = store.groupedContacts
        let letters = result.map(\.letter)
        #expect(letters == ["A", "B", "W", "#"].sorted())
        // A group should contain Anne (family Apple).
        let aGroup = result.first { $0.letter == "A" }
        #expect(aGroup?.contacts.contains { $0.givenName == "Anne" } == true)
    }

    // MARK: - hasConflicts

    @Test("hasConflicts reflects any contact with conflictState")
    func hasConflicts() {
        let store = makeStore([contact(given: "A")])
        #expect(!store.hasConflicts)
        store.contacts.append(contact(given: "B", conflict: .externalDelete))
        #expect(store.hasConflicts)
    }

    // MARK: - layoutMode

    @Test("layoutMode is .empty for no contacts")
    func layoutEmpty() {
        let store = makeStore()
        #expect(store.layoutMode == .empty)
    }

    @Test("layoutMode is .oneFilePerContact for single contact (per docstring)")
    func layoutSingleContactIsPerFile() {
        let store = makeStore([contact(fileName: "alice.vcf", given: "Alice")])
        #expect(store.layoutMode == .oneFilePerContact)
    }

    @Test("layoutMode is .oneFilePerContact when each contact has own file")
    func layoutPerFile() {
        let store = makeStore([
            contact(fileName: "a.vcf", given: "Alice"),
            contact(fileName: "b.vcf", given: "Bob"),
            contact(fileName: "c.vcf", given: "Carol"),
        ])
        #expect(store.layoutMode == .oneFilePerContact)
    }

    @Test("layoutMode is .singleFile when all share a file")
    func layoutSingleFile() {
        let store = makeStore([
            contact(fileName: "everyone.vcf", given: "Alice"),
            contact(fileName: "everyone.vcf", given: "Bob"),
            contact(fileName: "everyone.vcf", given: "Carol"),
        ])
        #expect(store.layoutMode == .singleFile(fileName: "everyone.vcf"))
    }

    @Test("layoutMode is .mixed when some files have multiple, others single")
    func layoutMixed() {
        let store = makeStore([
            contact(fileName: "shared.vcf", given: "Alice"),
            contact(fileName: "shared.vcf", given: "Bob"),
            contact(fileName: "solo.vcf", given: "Carol"),
        ])
        #expect(store.layoutMode == .mixed)
        #expect(!store.layoutMode.isSupported)
    }
}
