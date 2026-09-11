import Contacts
import Foundation

/// Narrow store surface used by `CNSyncService`.
///
/// Identifier / group membership lookups are explicit methods rather than
/// `NSPredicate` so tests can inject a fake without parsing opaque predicates.
protocol CNContactStoreProtocol: AnyObject, Sendable {
    var currentHistoryToken: Data? { get }
    var authorizationStatus: CNAuthorizationStatus { get }

    func requestAccess(for entityType: CNEntityType) async throws -> Bool
    func containers(matching predicate: NSPredicate?) throws -> [CNContainer]
    func defaultContainerIdentifier() -> String
    func groups(matching predicate: NSPredicate?) throws -> [CNGroup]
    func contacts(withIdentifiers identifiers: [String], keysToFetch: [any CNKeyDescriptor]) throws -> [CNContact]
    func contacts(inGroup groupIdentifier: String, keysToFetch: [any CNKeyDescriptor]) throws -> [CNContact]
    func execute(_ saveRequest: CNSaveRequest) throws
}

/// Production adapter around `CNContactStore`.
final class CNContactStoreAdapter: CNContactStoreProtocol, @unchecked Sendable {
    private let store = CNContactStore()

    var currentHistoryToken: Data? { store.currentHistoryToken }

    var authorizationStatus: CNAuthorizationStatus {
        CNContactStore.authorizationStatus(for: .contacts)
    }

    func requestAccess(for entityType: CNEntityType) async throws -> Bool {
        try await store.requestAccess(for: entityType)
    }

    func containers(matching predicate: NSPredicate?) throws -> [CNContainer] {
        try store.containers(matching: predicate)
    }

    func defaultContainerIdentifier() -> String {
        store.defaultContainerIdentifier()
    }

    func groups(matching predicate: NSPredicate?) throws -> [CNGroup] {
        try store.groups(matching: predicate)
    }

    func contacts(withIdentifiers identifiers: [String], keysToFetch: [any CNKeyDescriptor]) throws -> [CNContact] {
        let predicate = CNContact.predicateForContacts(withIdentifiers: identifiers)
        return try store.unifiedContacts(matching: predicate, keysToFetch: keysToFetch)
    }

    func contacts(inGroup groupIdentifier: String, keysToFetch: [any CNKeyDescriptor]) throws -> [CNContact] {
        let predicate = CNContact.predicateForContactsInGroup(withIdentifier: groupIdentifier)
        return try store.unifiedContacts(matching: predicate, keysToFetch: keysToFetch)
    }

    func execute(_ saveRequest: CNSaveRequest) throws {
        try store.execute(saveRequest)
    }
}
