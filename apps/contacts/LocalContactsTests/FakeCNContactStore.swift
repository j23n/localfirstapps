import Contacts
import Foundation
@testable import LocalContacts

/// In-memory `CNContactStoreProtocol` for sync tests. Ignores predicates
/// (the protocol uses explicit identifier / group lookups instead).
final class FakeCNContactStore: CNContactStoreProtocol, @unchecked Sendable {
    var authorizationStatus: CNAuthorizationStatus = .authorized
    var currentHistoryToken: Data? = Data([0x01])
    var defaultContainerID = "container-local"
    var groups: [CNGroup] = []
    var contactsByID: [String: CNContact] = [:]
    var groupMemberIDs: [String: [String]] = [:]
    var fetchError: Error?
    var executeError: Error?
    var executeCount = 0
    var requestAccessResult = true

    func requestAccess(for entityType: CNEntityType) async throws -> Bool {
        requestAccessResult
    }

    func containers(matching predicate: NSPredicate?) throws -> [CNContainer] {
        []
    }

    func defaultContainerIdentifier() -> String {
        defaultContainerID
    }

    func groups(matching predicate: NSPredicate?) throws -> [CNGroup] {
        groups
    }

    func contacts(withIdentifiers identifiers: [String], keysToFetch: [any CNKeyDescriptor]) throws -> [CNContact] {
        if let fetchError { throw fetchError }
        return identifiers.compactMap { contactsByID[$0] }
    }

    func contacts(inGroup groupIdentifier: String, keysToFetch: [any CNKeyDescriptor]) throws -> [CNContact] {
        if let fetchError { throw fetchError }
        let ids = groupMemberIDs[groupIdentifier] ?? []
        return ids.compactMap { contactsByID[$0] }
    }

    func execute(_ saveRequest: CNSaveRequest) throws {
        executeCount += 1
        if let executeError { throw executeError }
        if !groups.contains(where: { $0.name == CNSyncService.groupName }) {
            let group = CNMutableGroup()
            group.name = CNSyncService.groupName
            groups.append(group)
        }
    }

    @discardableResult
    func seedLocalContactsGroup() -> CNGroup {
        let group = CNMutableGroup()
        group.name = CNSyncService.groupName
        groups.append(group)
        return group
    }

    func add(_ contact: CNContact, toGroup groupID: String) {
        contactsByID[contact.identifier] = contact
        groupMemberIDs[groupID, default: []].append(contact.identifier)
    }
}

enum FakeStoreError: Error {
    case fetchFailed
}

func makeCNContact(
    given: String = "Alice",
    family: String = "Wonder",
    phones: [String] = [],
    emails: [String] = [],
    phoneLabel: String = CNLabelPhoneNumberMobile
) -> CNMutableContact {
    let cn = CNMutableContact()
    cn.givenName = given
    cn.familyName = family
    cn.phoneNumbers = phones.map {
        CNLabeledValue(label: phoneLabel, value: CNPhoneNumber(stringValue: $0))
    }
    cn.emailAddresses = emails.map {
        CNLabeledValue(label: CNLabelHome, value: $0 as NSString)
    }
    return cn
}

func makeIsolatedDefaults() -> UserDefaults {
    let name = "LocalContactsTests-Sync-\(UUID().uuidString)"
    let defaults = UserDefaults(suiteName: name)!
    defaults.removePersistentDomain(forName: name)
    return defaults
}
