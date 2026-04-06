import Contacts
import Foundation

actor CNSyncService {
    private let store = CNContactStore()
    private let containerNameKey = "LocalContacts"
    private let containerIDKey = "LocalContacts_ContainerID"
    private let historyTokenKey = "LocalContacts_ChangeHistoryToken"
    private let idMappingKey = "LocalContacts_IDMapping" // [LCID: CNContactIdentifier]

    nonisolated(unsafe) private static let fullKeysToFetch: [any CNKeyDescriptor] = [
        CNContactGivenNameKey as CNKeyDescriptor,
        CNContactFamilyNameKey as CNKeyDescriptor,
        CNContactMiddleNameKey as CNKeyDescriptor,
        CNContactNamePrefixKey as CNKeyDescriptor,
        CNContactNameSuffixKey as CNKeyDescriptor,
        CNContactPhoneNumbersKey as CNKeyDescriptor,
        CNContactEmailAddressesKey as CNKeyDescriptor,
        CNContactPostalAddressesKey as CNKeyDescriptor,
        CNContactBirthdayKey as CNKeyDescriptor,
        CNContactImageDataKey as CNKeyDescriptor,
    ]

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

    // MARK: - Container & Group

    private func defaultContainerID() -> String {
        (try? store.defaultContainerIdentifier()) ?? ""
    }

    /// Find or create the single "LocalContacts" group in the default container.
    /// Also cleans up any duplicate groups from prior bugs.
    private func resolveGroup() throws -> (containerID: String, group: CNGroup) {
        let containerID = defaultContainerID()
        let predicate = CNGroup.predicateForGroupsInContainer(withIdentifier: containerID)
        let groups = try store.groups(matching: predicate)
        let matches = groups.filter { $0.name == containerNameKey }

        if let first = matches.first {
            // Clean up duplicates if any
            if matches.count > 1 {
                let saveRequest = CNSaveRequest()
                for dupe in matches.dropFirst() {
                    saveRequest.delete(dupe.mutableCopy() as! CNMutableGroup)
                }
                try store.execute(saveRequest)
            }
            return (containerID, first)
        }

        // Create new group
        let newGroup = CNMutableGroup()
        newGroup.name = containerNameKey
        let saveRequest = CNSaveRequest()
        saveRequest.add(newGroup, toContainerWithIdentifier: containerID)
        try store.execute(saveRequest)

        let refreshed = try store.groups(matching: predicate)
        guard let created = refreshed.first(where: { $0.name == containerNameKey }) else {
            throw CNSyncError.groupCreationFailed
        }
        return (containerID, created)
    }

    // MARK: - Push Single Contact

    func pushContact(_ contact: Contact) async throws {
        guard authorizationStatus == .authorized else { return }

        let (containerID, group) = try resolveGroup()
        let existingCN = try findCNContact(localContactsID: contact.localContactsID)

        let saveRequest = CNSaveRequest()

        if let existing = existingCN {
            let mutable = existing.mutableCopy() as! CNMutableContact
            mapContactToCN(contact, cn: mutable)
            saveRequest.update(mutable)
            try store.execute(saveRequest)
        } else {
            let newCN = CNMutableContact()
            mapContactToCN(contact, cn: newCN)
            saveRequest.add(newCN, toContainerWithIdentifier: containerID)
            saveRequest.addMember(newCN, to: group)
            try store.execute(saveRequest)
            setCNIdentifier(newCN.identifier, forLocalContactsID: contact.localContactsID)
        }

        saveHistoryToken()
    }

    func deleteContact(localContactsID: String) async throws {
        guard authorizationStatus == .authorized else { return }

        if let existing = try findCNContact(localContactsID: localContactsID) {
            let mutable = existing.mutableCopy() as! CNMutableContact
            let saveRequest = CNSaveRequest()
            saveRequest.delete(mutable)
            try store.execute(saveRequest)
            removeMappingForLocalContactsID(localContactsID)
            saveHistoryToken()
        }
    }

    /// Map an existing CNContact identifier to a local contact ID.
    /// Used when importing an externally-created CN contact — we claim
    /// ownership of the existing CN contact instead of pushing a duplicate.
    func claimCNContact(cnIdentifier: String, forLocalContactsID lcID: String) {
        setCNIdentifier(cnIdentifier, forLocalContactsID: lcID)
        saveHistoryToken()
    }

    // MARK: - Full Reconciliation

    /// Deletes all contacts in the LocalContacts group, removes duplicate groups,
    /// then re-adds every contact fresh. This is a clean "nuke and pave".
    func fullReconciliation(contacts: [Contact]) async throws {
        guard authorizationStatus == .authorized else { return }

        let containerID = defaultContainerID()

        // 1. Find ALL "LocalContacts" groups and delete their member contacts + the groups themselves
        let predicate = CNGroup.predicateForGroupsInContainer(withIdentifier: containerID)
        let allGroups = try store.groups(matching: predicate)
        let lcGroups = allGroups.filter { $0.name == containerNameKey }

        let keysToFetch: [CNKeyDescriptor] = [
            CNContactIdentifierKey as CNKeyDescriptor,
        ]

        let deleteRequest = CNSaveRequest()
        var hasDeletes = false

        for group in lcGroups {
            let memberPredicate = CNContact.predicateForContactsInGroup(withIdentifier: group.identifier)
            let members = try store.unifiedContacts(matching: memberPredicate, keysToFetch: keysToFetch)

            for member in members {
                deleteRequest.delete(member.mutableCopy() as! CNMutableContact)
                hasDeletes = true
            }

            deleteRequest.delete(group.mutableCopy() as! CNMutableGroup)
            hasDeletes = true
        }

        if hasDeletes {
            try store.execute(deleteRequest)
        }

        // 2. Clear mapping only after deletes succeeded
        saveIDMapping([:])

        // 3. Create fresh group
        let newGroup = CNMutableGroup()
        newGroup.name = containerNameKey
        let groupReq = CNSaveRequest()
        groupReq.add(newGroup, toContainerWithIdentifier: containerID)
        try store.execute(groupReq)

        let refreshed = try store.groups(matching: predicate)
        guard let group = refreshed.first(where: { $0.name == containerNameKey }) else {
            throw CNSyncError.groupCreationFailed
        }

        // 4. Add all contacts fresh
        for contact in contacts {
            let newCN = CNMutableContact()
            mapContactToCN(contact, cn: newCN)
            let saveReq = CNSaveRequest()
            saveReq.add(newCN, toContainerWithIdentifier: containerID)
            saveReq.addMember(newCN, to: group)
            try store.execute(saveReq)
            setCNIdentifier(newCN.identifier, forLocalContactsID: contact.localContactsID)
        }

        saveHistoryToken()
    }

    // MARK: - Delta Sync

    struct ChangeEvent: Sendable {
        enum Kind: Sendable {
            case added(contactData: CNContactData)
            case updated(contactData: CNContactData)
            case deleted(localContactsID: String)
        }
        let kind: Kind
    }

    struct CNContactData: Sendable {
        let cnIdentifier: String
        let localContactsID: String
        let givenName: String
        let familyName: String
        let middleName: String
        let namePrefix: String
        let nameSuffix: String
        let phoneNumbers: [(label: String, value: String)]
        let emailAddresses: [(label: String, value: String)]
        let postalAddresses: [(label: String, street: String, city: String, state: String, postalCode: String, country: String)]
        let birthday: DateComponents?
        let imageData: Data?
    }

    func fetchChanges(localContacts: [Contact]) async -> [ChangeEvent] {
        guard authorizationStatus == .authorized else { return [] }

        let currentToken = store.currentHistoryToken
        let lastToken = UserDefaults.standard.data(forKey: historyTokenKey)
        if let current = currentToken, let last = lastToken, current == last {
            return []
        }

        let keysToFetch = Self.fullKeysToFetch

        let knownByID = Dictionary(uniqueKeysWithValues: localContacts.map { ($0.localContactsID, $0) })
        var events: [ChangeEvent] = []

        do {
            let (_, group) = try resolveGroup()
            let predicate = CNContact.predicateForContactsInGroup(withIdentifier: group.identifier)
            let cnContacts = try store.unifiedContacts(matching: predicate, keysToFetch: keysToFetch)

            var foundIDs = Set<String>()

            for cn in cnContacts {
                if let lcID = extractLocalContactsID(from: cn) {
                    foundIDs.insert(lcID)
                    if let local = knownByID[lcID], contactDiffers(local: local, cn: cn) {
                        let data = extractCNContactData(from: cn, localContactsID: lcID)
                        events.append(ChangeEvent(kind: .updated(contactData: data)))
                    }
                } else {
                    let data = extractCNContactData(from: cn, localContactsID: "")
                    events.append(ChangeEvent(kind: .added(contactData: data)))
                }
            }

            for id in knownByID.keys where !foundIDs.contains(id) {
                events.append(ChangeEvent(kind: .deleted(localContactsID: id)))
            }
        } catch {
            print("Failed to fetch contacts for change detection: \(error)")
        }

        saveHistoryToken()
        return events
    }

    /// Compare local Contact fields against CNContact to detect real changes.
    private func contactDiffers(local: Contact, cn: CNContact) -> Bool {
        // Name fields
        if local.givenName != cn.givenName { return true }
        if local.familyName != cn.familyName { return true }
        if local.middleName != cn.middleName { return true }
        if local.namePrefix != cn.namePrefix { return true }
        if local.nameSuffix != cn.nameSuffix { return true }

        // Phone numbers (compare values, ignore label differences)
        let localPhones = local.phoneNumbers.map(\.value).sorted()
        let cnPhones = cn.phoneNumbers.map { $0.value.stringValue }.sorted()
        if localPhones != cnPhones { return true }

        // Email addresses
        let localEmails = local.emailAddresses.map { $0.value.lowercased() }.sorted()
        let cnEmails = cn.emailAddresses.map { ($0.value as String).lowercased() }.sorted()
        if localEmails != cnEmails { return true }

        // Postal addresses
        let localAddrs = local.postalAddresses.map {
            [$0.value.street, $0.value.city, $0.value.state, $0.value.postalCode, $0.value.country].joined(separator: "|")
        }.sorted()
        let cnAddrs = cn.postalAddresses.map {
            let a = $0.value
            return [a.street, a.city, a.state, a.postalCode, a.country].joined(separator: "|")
        }.sorted()
        if localAddrs != cnAddrs { return true }

        // Birthday
        if local.birthday != cn.birthday { return true }

        // Photo: skip comparison — CNContactStore re-encodes images,
        // so byte-level comparison would always trigger false positives

        return false
    }

    /// Fetch the CNContact data for a specific local contact, for import purposes.
    func fetchCNContactData(localContactsID: String) async -> CNContactData? {
        guard let cn = try? findCNContact(localContactsID: localContactsID) else { return nil }
        return extractCNContactData(from: cn, localContactsID: localContactsID)
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

        if let photoData = contact.photoData {
            cn.imageData = photoData
        }
    }

    // MARK: - ID Mapping (LCID <-> CNContact.identifier)

    private func loadIDMapping() -> [String: String] {
        UserDefaults.standard.dictionary(forKey: idMappingKey) as? [String: String] ?? [:]
    }

    private func saveIDMapping(_ mapping: [String: String]) {
        UserDefaults.standard.set(mapping, forKey: idMappingKey)
    }

    private func setCNIdentifier(_ cnID: String, forLocalContactsID lcID: String) {
        var mapping = loadIDMapping()
        mapping[lcID] = cnID
        saveIDMapping(mapping)
    }

    private func removeMappingForLocalContactsID(_ lcID: String) {
        var mapping = loadIDMapping()
        mapping.removeValue(forKey: lcID)
        saveIDMapping(mapping)
    }

    private func findCNContact(localContactsID: String) throws -> CNContact? {
        let mapping = loadIDMapping()
        guard let cnID = mapping[localContactsID] else { return nil }

        let predicate = CNContact.predicateForContacts(withIdentifiers: [cnID])
        return try store.unifiedContacts(matching: predicate, keysToFetch: Self.fullKeysToFetch).first
    }

    private func extractLocalContactsID(from cn: CNContact) -> String? {
        let mapping = loadIDMapping()
        return mapping.first(where: { $0.value == cn.identifier })?.key
    }

    private func extractCNContactData(from cn: CNContact, localContactsID: String) -> CNContactData {
        CNContactData(
            cnIdentifier: cn.identifier,
            localContactsID: localContactsID,
            givenName: cn.givenName,
            familyName: cn.familyName,
            middleName: cn.middleName,
            namePrefix: cn.namePrefix,
            nameSuffix: cn.nameSuffix,
            phoneNumbers: cn.phoneNumbers.map { (label: $0.label ?? "mobile", value: $0.value.stringValue) },
            emailAddresses: cn.emailAddresses.map { (label: $0.label ?? "home", value: $0.value as String) },
            postalAddresses: cn.postalAddresses.map { lv in
                let a = lv.value
                return (label: lv.label ?? "home", street: a.street, city: a.city, state: a.state, postalCode: a.postalCode, country: a.country)
            },
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
