import Contacts
import Foundation

actor CNSyncService {
    private let store = CNContactStore()
    private let containerNameKey = "LocalContacts"
    private let containerIDKey = "LocalContacts_ContainerID"
    private let historyTokenKey = "LocalContacts_ChangeHistoryToken"

    // MARK: - Authorization

    func requestAccess() async -> Bool {
        do {
            return try await store.requestAccess(for: .contacts)
        } catch {
            print("CNContactStore access error: \(error)")
            return false
        }
    }

    var authorizationStatus: CNAuthorizationStatus {
        CNContactStore.authorizationStatus(for: .contacts)
    }

    // MARK: - Container

    private func findOrCreateContainer() throws -> String {
        // Check stored container ID
        if let storedID = UserDefaults.standard.string(forKey: containerIDKey) {
            // Verify it still exists
            do {
                let containers = try store.containers(matching: CNContainer.predicateForContainers(withIdentifiers: [storedID]))
                if !containers.isEmpty {
                    return storedID
                }
            } catch {
                // Container no longer exists, will create new
            }
        }

        // Look for existing container by name
        do {
            let allContainers = try store.containers(matching: nil)
            if let existing = allContainers.first(where: { $0.name == containerNameKey }) {
                UserDefaults.standard.set(existing.identifier, forKey: containerIDKey)
                return existing.identifier
            }
        } catch {
            // Fall through to create
        }

        // Note: iOS doesn't allow creating custom containers programmatically.
        // We use the default container and group contacts under a "LocalContacts" group instead.
        let defaultContainer = try store.defaultContainerIdentifier()
        UserDefaults.standard.set(defaultContainer, forKey: containerIDKey)
        return defaultContainer
    }

    private func findOrCreateGroup(inContainer containerID: String) throws -> CNGroup {
        let predicate = CNGroup.predicateForGroupsInContainer(withIdentifier: containerID)
        let groups = try store.groups(matching: predicate)

        if let existing = groups.first(where: { $0.name == containerNameKey }) {
            return existing
        }

        let newGroup = CNMutableGroup()
        newGroup.name = containerNameKey
        let saveRequest = CNSaveRequest()
        saveRequest.add(newGroup, toContainerWithIdentifier: containerID)
        try store.execute(saveRequest)

        // Re-fetch to get the persisted group
        let refreshedGroups = try store.groups(matching: predicate)
        guard let created = refreshedGroups.first(where: { $0.name == containerNameKey }) else {
            throw CNSyncError.groupCreationFailed
        }
        return created
    }

    // MARK: - Push to CNContactStore

    func pushContact(_ contact: Contact) async throws {
        guard authorizationStatus == .authorized else { return }

        let containerID = try findOrCreateContainer()
        let group = try findOrCreateGroup(inContainer: containerID)

        // Check if contact already exists
        let existingCN = try findCNContact(localContactsID: contact.localContactsID)

        let saveRequest = CNSaveRequest()

        if let existing = existingCN {
            let mutable = existing.mutableCopy() as! CNMutableContact
            mapContactToCN(contact, cn: mutable)
            saveRequest.update(mutable)
        } else {
            let newCN = CNMutableContact()
            mapContactToCN(contact, cn: newCN)
            saveRequest.add(newCN, toContainerWithIdentifier: containerID)
            saveRequest.addMember(newCN, to: group)
        }

        try store.execute(saveRequest)
        saveHistoryToken()
    }

    func deleteContact(localContactsID: String) async throws {
        guard authorizationStatus == .authorized else { return }

        if let existing = try findCNContact(localContactsID: localContactsID) {
            let mutable = existing.mutableCopy() as! CNMutableContact
            let saveRequest = CNSaveRequest()
            saveRequest.delete(mutable)
            try store.execute(saveRequest)
            saveHistoryToken()
        }
    }

    // MARK: - Delta Sync (CNChangeHistory)

    struct ChangeEvent: Sendable {
        enum Kind: Sendable {
            case added(contactData: CNContactData)
            case updated(contactData: CNContactData)
            case deleted(localContactsID: String)
        }
        let kind: Kind
    }

    struct CNContactData: Sendable {
        let localContactsID: String
        let givenName: String
        let familyName: String
        let phoneNumbers: [(label: String, value: String)]
        let emailAddresses: [(label: String, value: String)]
        let note: String
        let birthday: DateComponents?
        let imageData: Data?
    }

    /// Detects changes by comparing current CNContactStore state against known local contacts.
    /// CNChangeHistoryFetchRequest's enumerator is NS_SWIFT_UNAVAILABLE, so we use a
    /// snapshot-comparison approach: fetch all contacts in our group and diff against known IDs.
    func fetchChanges(knownIDs: Set<String>) async -> [ChangeEvent] {
        guard authorizationStatus == .authorized else { return [] }

        // Check if anything changed via history token
        let currentToken = store.currentHistoryToken
        let lastToken = UserDefaults.standard.data(forKey: historyTokenKey)

        // If tokens match, no changes
        if let current = currentToken, let last = lastToken, current == last {
            return []
        }

        let keysToFetch: [CNKeyDescriptor] = [
            CNContactGivenNameKey as CNKeyDescriptor,
            CNContactFamilyNameKey as CNKeyDescriptor,
            CNContactPhoneNumbersKey as CNKeyDescriptor,
            CNContactEmailAddressesKey as CNKeyDescriptor,
            CNContactNoteKey as CNKeyDescriptor,
            CNContactBirthdayKey as CNKeyDescriptor,
            CNContactImageDataKey as CNKeyDescriptor,
        ]

        var events: [ChangeEvent] = []

        do {
            guard let containerID = try? findOrCreateContainer(),
                  let group = try? findOrCreateGroup(inContainer: containerID) else {
                return []
            }

            let predicate = CNContact.predicateForContactsInGroup(withIdentifier: group.identifier)
            let cnContacts = try store.unifiedContacts(matching: predicate, keysToFetch: keysToFetch)

            var foundIDs = Set<String>()

            for cn in cnContacts {
                if let lcID = extractLocalContactsID(from: cn) {
                    foundIDs.insert(lcID)
                    if knownIDs.contains(lcID) {
                        // Exists locally — flag as potentially updated
                        let data = extractCNContactData(from: cn, localContactsID: lcID)
                        events.append(ChangeEvent(kind: .updated(contactData: data)))
                    }
                } else {
                    // New contact without LCID — added externally
                    let data = extractCNContactData(from: cn, localContactsID: "")
                    events.append(ChangeEvent(kind: .added(contactData: data)))
                }
            }

            // Contacts in our known set but missing from CN — deleted externally
            for id in knownIDs where !foundIDs.contains(id) {
                events.append(ChangeEvent(kind: .deleted(localContactsID: id)))
            }
        } catch {
            print("Failed to fetch contacts for change detection: \(error)")
        }

        saveHistoryToken()
        return events
    }

    // MARK: - Full Reconciliation (fallback)

    func fullReconciliation(contacts: [Contact]) async throws {
        guard authorizationStatus == .authorized else { return }

        let containerID = try findOrCreateContainer()
        let group = try findOrCreateGroup(inContainer: containerID)

        for contact in contacts {
            let saveRequest = CNSaveRequest()
            let existingCN = try findCNContact(localContactsID: contact.localContactsID)

            if let existing = existingCN {
                let mutable = existing.mutableCopy() as! CNMutableContact
                mapContactToCN(contact, cn: mutable)
                saveRequest.update(mutable)
            } else {
                let newCN = CNMutableContact()
                mapContactToCN(contact, cn: newCN)
                saveRequest.add(newCN, toContainerWithIdentifier: containerID)
                saveRequest.addMember(newCN, to: group)
            }

            try store.execute(saveRequest)
        }

        saveHistoryToken()
    }

    // MARK: - Mapping

    private func mapContactToCN(_ contact: Contact, cn: CNMutableContact) {
        cn.givenName = contact.givenName
        cn.familyName = contact.familyName
        cn.middleName = contact.middleName
        cn.namePrefix = contact.namePrefix
        cn.nameSuffix = contact.nameSuffix

        cn.phoneNumbers = contact.phoneNumbers.map { phone in
            let label = cnLabel(from: phone.label, isPhone: true)
            return CNLabeledValue(label: label, value: CNPhoneNumber(stringValue: phone.value))
        }

        cn.emailAddresses = contact.emailAddresses.map { email in
            let label = cnLabel(from: email.label, isPhone: false)
            return CNLabeledValue(label: label, value: email.value as NSString)
        }

        cn.postalAddresses = contact.postalAddresses.map { addr in
            let label = cnLabel(from: addr.label, isPhone: false)
            let postal = CNMutablePostalAddress()
            postal.street = addr.value.street
            postal.city = addr.value.city
            postal.state = addr.value.state
            postal.postalCode = addr.value.postalCode
            postal.country = addr.value.country
            return CNLabeledValue(label: label, value: postal)
        }

        cn.birthday = contact.birthday

        if !contact.note.isEmpty {
            cn.note = contact.note
        }

        if let photoData = contact.photoData {
            cn.imageData = photoData
        }

        // Store the localContactsID in the contact's note or custom field
        // We use a custom note prefix for identification
        let idMarker = "[LCID:\(contact.localContactsID)]"
        if !cn.note.contains(idMarker) {
            cn.note = cn.note.isEmpty ? idMarker : "\(cn.note)\n\(idMarker)"
        }
    }

    private func findCNContact(localContactsID: String) throws -> CNContact? {
        let keysToFetch: [CNKeyDescriptor] = [
            CNContactGivenNameKey as CNKeyDescriptor,
            CNContactFamilyNameKey as CNKeyDescriptor,
            CNContactMiddleNameKey as CNKeyDescriptor,
            CNContactNamePrefixKey as CNKeyDescriptor,
            CNContactNameSuffixKey as CNKeyDescriptor,
            CNContactPhoneNumbersKey as CNKeyDescriptor,
            CNContactEmailAddressesKey as CNKeyDescriptor,
            CNContactPostalAddressesKey as CNKeyDescriptor,
            CNContactBirthdayKey as CNKeyDescriptor,
            CNContactNoteKey as CNKeyDescriptor,
            CNContactImageDataKey as CNKeyDescriptor,
        ]

        let predicate = CNContact.predicateForContactsInContainer(withIdentifier:
            UserDefaults.standard.string(forKey: containerIDKey) ?? store.defaultContainerIdentifier())
        let allContacts = try store.unifiedContacts(matching: predicate, keysToFetch: keysToFetch)

        let idMarker = "[LCID:\(localContactsID)]"
        return allContacts.first { $0.note.contains(idMarker) }
    }

    private func extractLocalContactsID(from cn: CNContact) -> String? {
        let pattern = "\\[LCID:([A-F0-9\\-]+)\\]"
        guard let regex = try? NSRegularExpression(pattern: pattern, options: .caseInsensitive),
              let match = regex.firstMatch(in: cn.note, range: NSRange(cn.note.startIndex..., in: cn.note)),
              let range = Range(match.range(at: 1), in: cn.note) else {
            return nil
        }
        return String(cn.note[range])
    }

    private func extractCNContactData(from cn: CNContact, localContactsID: String) -> CNContactData {
        CNContactData(
            localContactsID: localContactsID,
            givenName: cn.givenName,
            familyName: cn.familyName,
            phoneNumbers: cn.phoneNumbers.map { (label: $0.label ?? "mobile", value: $0.value.stringValue) },
            emailAddresses: cn.emailAddresses.map { (label: $0.label ?? "home", value: $0.value as String) },
            note: cn.note,
            birthday: cn.birthday,
            imageData: cn.imageData
        )
    }

    private func cnLabel(from label: String, isPhone: Bool) -> String {
        switch label.lowercased() {
        case "home": return CNLabelHome
        case "work": return CNLabelWork
        case "mobile", "cell": return CNLabelPhoneNumberMobile
        case "main": return CNLabelPhoneNumberMain
        case "iphone": return CNLabelPhoneNumberiPhone
        case "fax": return isPhone ? CNLabelPhoneNumberWorkFax : CNLabelWork
        case "pager": return CNLabelPhoneNumberPager
        case "other": return CNLabelOther
        default: return CNLabelOther
        }
    }

    private func saveHistoryToken() {
        if let token = store.currentHistoryToken {
            UserDefaults.standard.set(token, forKey: historyTokenKey)
        }
    }
}

enum CNSyncError: LocalizedError {
    case groupCreationFailed
    case containerNotFound

    var errorDescription: String? {
        switch self {
        case .groupCreationFailed: "Failed to create LocalContacts group."
        case .containerNotFound: "Could not find contacts container."
        }
    }
}
