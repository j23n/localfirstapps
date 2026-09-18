import Foundation
import Observation
import os
import ShellKitSwift

@Observable
@MainActor
final class ContactsStore {
    var contacts: [Contact] = []
    var searchText: String = ""
    var selectedTag: String?
    var showConflictsOnly = false
    var folderURL: URL?
    var isLoading = false
    private(set) var progressRevealed = false
    var isSuppressingReload = false
    var errorMessage: String?
    var lastSyncedAt: Date?
    /// Syncthing `.vcf` groups (ADR 0005 R8). Not Apple CN conflicts.
    var syncConflictGroups: [ConflictRow] = []

    private var session: ContactsSession?
    /// Per-device log partition (ADR 0005 R5). Not synced.
    let deviceId: String
    let bookmarkManager = BookmarkManager()
    let folderAccess = FolderAccessManager()
    let syncService: CNSyncService

    static let deviceIdKey = "LocalContacts_DeviceId"

    init(syncService: CNSyncService = CNSyncService(), deviceId: String? = nil) {
        self.syncService = syncService
        self.deviceId = deviceId ?? Self.storedDeviceId()
    }

    static func storedDeviceId(defaults: UserDefaults = .standard) -> String {
        if let existing = defaults.string(forKey: deviceIdKey), !existing.isEmpty {
            return existing
        }
        let id = "ios-\(UUID().uuidString)"
        defaults.set(id, forKey: deviceIdKey)
        return id
    }

    // MARK: - Computed

    var allTags: [(tag: String, count: Int)] {
        if let session, let rows = try? session.tagRows() {
            return rows.map { row in
                let count = row.trailing?
                    .split(separator: " ")
                    .first
                    .flatMap { Int($0) } ?? 0
                return (tag: row.id, count: count)
            }
        }
        var tagCounts: [String: Int] = [:]
        for contact in contacts {
            for tag in contact.categories {
                tagCounts[tag, default: 0] += 1
            }
        }
        return tagCounts.map { (tag: $0.key, count: $0.value) }.sorted { $0.tag < $1.tag }
    }

    var filteredContacts: [Contact] {
        let tag = showConflictsOnly ? nil : selectedTag
        var result: [Contact]
        if let session,
           let rows = try? session.filteredListRows(query: searchText, tag: tag) {
            let contactsByID = Dictionary(
                uniqueKeysWithValues: contacts.map { ($0.localContactsID, $0) }
            )
            result = rows.compactMap { contactsByID[$0.id] }
        } else {
            result = contacts
            if let tag {
                result = result.filter { $0.categories.contains(tag) }
            }
            if !searchText.isEmpty {
                let query = searchText.lowercased()
                result = result.filter { contact in
                    contact.displayName.lowercased().contains(query)
                    || contact.organization.lowercased().contains(query)
                    || contact.jobTitle.lowercased().contains(query)
                    || contact.phoneNumbers.contains { $0.value.contains(query) }
                    || contact.emailAddresses.contains { $0.value.lowercased().contains(query) }
                }
            }
            result.sort {
                $0.displayName.localizedCaseInsensitiveCompare($1.displayName) == .orderedAscending
            }
        }

        if showConflictsOnly {
            result = result.filter { $0.conflictState != nil }
        }

        return result
    }

    var groupedContacts: [(letter: String, contacts: [Contact])] {
        let grouped = Dictionary(grouping: filteredContacts) { $0.sortLetter }
        return grouped
            .map { (letter: $0.key, contacts: $0.value) }
            .sorted { $0.letter < $1.letter }
    }

    var hasConflicts: Bool {
        contacts.contains { $0.conflictState != nil }
    }

    var hasSyncConflictGroups: Bool {
        !syncConflictGroups.isEmpty
    }

    /// Default list rows from core (`title` + organization/email `subtitle`).
    var listRows: [TextRow] {
        let tag = showConflictsOnly ? nil : selectedTag
        if let session, let rows = try? session.filteredListRows(query: "", tag: tag) {
            if showConflictsOnly {
                let conflicted = Set(
                    contacts.compactMap { $0.conflictState == nil ? nil : $0.localContactsID }
                )
                return rows.filter { conflicted.contains($0.id) }
            }
            return rows
        }
        return filteredContacts.map { contact in
            TextRow(
                id: contact.localContactsID,
                title: contact.displayName,
                subtitle: contact.organization.isEmpty
                    ? contact.emailAddresses.first?.value
                    : contact.organization,
                trailing: nil
            )
        }
    }

    /// Live-search hits. Empty query yields no hits; the list uses [`listRows`].
    var searchHits: [SearchHit] {
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return [] }
        let tag = showConflictsOnly ? nil : selectedTag
        if let session, let hits = try? session.searchHits(query: query, tag: tag) {
            if showConflictsOnly {
                let conflicted = Set(
                    contacts.compactMap { $0.conflictState == nil ? nil : $0.localContactsID }
                )
                return hits.filter { conflicted.contains($0.id) }
            }
            return hits
        }
        return filteredContacts.map { contact in
            SearchHit(
                id: contact.localContactsID,
                title: contact.displayName,
                subtitle: contact.organization,
                trailing: contact.organization.isEmpty ? nil : "Organization",
                symbol: "contact-new-symbolic"
            )
        }
    }

    func listRow(for id: String) -> TextRow? {
        listRows.first { $0.id == id }
    }

    func fieldRows(id: String) -> [FieldRow] {
        (try? session?.fieldRows(id: id)) ?? []
    }

    func exportVCardText(id: String) throws -> String {
        try openSession().exportVcardText(id: id)
    }

    func newContactDraft() -> ContactEditDraft {
        if let session {
            return session.newContactDraft()
        }
        return ContactEditDraft(
            id: nil,
            contentToken: nil,
            fullName: "",
            familyName: "",
            givenName: "",
            middleName: "",
            namePrefix: "",
            nameSuffix: "",
            organization: "",
            jobTitle: "",
            nickname: "",
            urls: [],
            phones: [],
            emails: [],
            addresses: [],
            birthday: nil,
            note: "",
            categories: [],
            photo: nil
        )
    }

    func contactEditDraft(id: String) throws -> ContactEditDraft {
        try openSession().contactEditDraft(id: id)
    }

    @discardableResult
    func save(_ draft: ContactEditDraft) async throws -> ContactEditDraft {
        let session = try openSession()
        let saved = try session.saveContact(command: SaveContactCommand(draft: draft))
        try refreshFromSession(session)
        return saved
    }

    /// The folder's vCard layout, derived from how contacts are distributed across files.
    /// Two layouts are supported: every contact in its own file, or every contact in a single
    /// shared file. Anything else is `.mixed` and should be reconciled by the user.
    ///
    /// A folder containing exactly one contact is reported as `.oneFilePerContact`, since
    /// the layout is only distinguishable once a second contact joins the file. Adding a
    /// second contact in this state creates a new file rather than appending to the first.
    var layoutMode: FolderLayoutMode {
        var counts: [String: Int] = [:]
        for contact in contacts {
            counts[contact.fileName, default: 0] += 1
        }
        if counts.isEmpty { return .empty }
        let oversize = counts.filter { $0.value > 1 }
        if oversize.isEmpty { return .oneFilePerContact }
        if counts.count == 1, let only = counts.first {
            return .singleFile(fileName: only.key)
        }
        return .mixed
    }

    // MARK: - Folder Management

    func setFolder(_ url: URL) async {
        do {
            try bookmarkManager.saveBookmark(for: url)
        } catch {
            errorMessage = "Failed to save folder bookmark: \(error.localizedDescription)"
            return
        }

        if let resolved = bookmarkManager.loadBookmark() {
            await folderAccess.startAccessing(resolved)
            folderURL = resolved
            session = nil
            await loadContacts()
        }
    }

    func restoreFolder() async {
        if let path = Self.folderPath(fromLaunchArguments: ProcessInfo.processInfo.arguments) {
            folderURL = URL(fileURLWithPath: path, isDirectory: true)
            session = nil
            await loadContacts()
            return
        }
        guard let url = bookmarkManager.loadBookmark() else { return }
        await folderAccess.startAccessing(url)
        folderURL = url
        await loadContacts()
    }

    /// UI tests (and debug launches) can skip the folder picker with
    /// `--contacts-folder <path>`.
    static func folderPath(fromLaunchArguments arguments: [String]) -> String? {
        guard let index = arguments.firstIndex(of: "--contacts-folder"),
              arguments.indices.contains(index + 1) else { return nil }
        return arguments[index + 1]
    }

    // MARK: - Load

    func loadContacts() async {
        guard let url = folderURL else { return }
        guard !isSuppressingReload else { return }
        isLoading = true
        progressRevealed = false
        errorMessage = nil
        let reveal = Task { await self.revealProgressIfNeeded() }

        do {
            let opened = try ContactsSession.open(root: url.path, device: deviceId)
            session = opened
            try opened.reload()
            try refreshFromSession(opened)
        } catch {
            errorMessage = "Failed to read folder: \(error.localizedDescription)"
        }

        lastSyncedAt = Date()
        isLoading = false
        progressRevealed = false
        reveal.cancel()
    }

    private func revealProgressIfNeeded() async {
        try? await Task.sleep(for: .milliseconds(500))
        if isLoading {
            progressRevealed = true
        }
    }

    var chromeProgress: ShellProgressData? {
        guard isLoading, progressRevealed, !contacts.isEmpty else { return nil }
        return ShellProgressData(label: "Reloading", cancel: false)
    }

    var settingsProgress: ShellProgressData? {
        guard isLoading else { return nil }
        return ShellProgressData(label: "Reloading", cancel: false)
    }

    // MARK: - Save

    func save(_ contact: Contact) async throws {
        let session = try persist(contact)
        let id = contact.localContactsID
        let apple = contact.conflictState
        try refreshFromSession(session)
        if let updated = contacts.first(where: { $0.localContactsID == id }) {
            updated.conflictState = apple
        }
    }

    // MARK: - Delete

    func delete(_ contact: Contact) async throws {
        let session = try openSession()
        try session.delete(id: contact.localContactsID)
        contacts.removeAll { $0.localContactsID == contact.localContactsID }
        try? await syncService.deleteContact(localContactsID: contact.localContactsID)
        syncConflictGroups = (try? session.conflictRows()) ?? []
    }

    // MARK: - Tag Management

    func renameTag(_ oldName: String, to newName: String) async throws {
        let trimmed = newName.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty, trimmed != oldName else { return }

        let session = try openSession()
        _ = try session.renameTag(oldName: oldName, newName: trimmed)
        try refreshFromSession(session)

        if selectedTag == oldName {
            selectedTag = trimmed
        }
    }

    func deleteTag(_ tagName: String) async throws {
        let session = try openSession()
        _ = try session.removeTag(tag: tagName)
        try refreshFromSession(session)

        if selectedTag == tagName {
            selectedTag = nil
        }
    }

    // MARK: - Bulk Operations

    func deleteMultiple(_ contactIDs: Set<String>) async throws {
        let toDelete = contacts.filter { contactIDs.contains($0.localContactsID) }
        guard !toDelete.isEmpty else { return }

        let session = try openSession()
        _ = try session.deleteMany(ids: Array(contactIDs))
        try refreshFromSession(session)

        for contact in toDelete {
            try? await syncService.deleteContact(localContactsID: contact.localContactsID)
        }
        syncConflictGroups = (try? session.conflictRows()) ?? []
    }

    func assignTag(_ tag: String, to contactIDs: Set<String>) async throws {
        let session = try openSession()
        _ = try session.assignTag(tag: tag, ids: Array(contactIDs))
        try refreshFromSession(session)
    }

    // MARK: - External change events

    /// Apply Apple Contacts change events: mark conflicts or import new contacts.
    /// Existing conflict state is left untouched.
    func applyChangeEvents(_ events: [CNSyncService.ChangeEvent]) async {
        for event in events {
            switch event.kind {
            case .updated(let data):
                if let contact = contacts.first(where: { $0.localContactsID == data.localContactsID }),
                   contact.conflictState == nil {
                    contact.conflictState = .externalEdit(data)
                }
            case .deleted(let localContactsID):
                if let contact = contacts.first(where: { $0.localContactsID == localContactsID }),
                   contact.conflictState == nil {
                    contact.conflictState = .externalDelete
                }
            case .added(let data):
                await importExternalContact(data)
            }
        }
    }

    func importExternalContact(_ data: CNSyncService.CNContactData) async {
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
            try await save(contact)
            await syncService.claimCNContact(
                cnIdentifier: data.cnIdentifier,
                forLocalContactsID: contact.localContactsID
            )
        } catch {
            Log.sync.error("Failed to import external contact: \(error.localizedDescription)")
        }
    }

    // MARK: - Import External Changes

    func applyExternalData(_ data: CNSyncService.CNContactData, to contact: Contact) async throws {
        contact.givenName = data.givenName
        contact.familyName = data.familyName
        contact.middleName = data.middleName
        contact.namePrefix = data.namePrefix
        contact.nameSuffix = data.nameSuffix
        contact.organization = data.organization
        contact.jobTitle = data.jobTitle
        contact.nickname = data.nickname

        contact.urls = data.urls.map {
            LabeledValue(label: $0.label, value: $0.value)
        }
        contact.phoneNumbers = data.phoneNumbers.map {
            LabeledValue(label: $0.label, value: $0.value)
        }
        contact.emailAddresses = data.emailAddresses.map {
            LabeledValue(label: $0.label, value: $0.value)
        }
        contact.postalAddresses = data.postalAddresses.map {
            LabeledValue(label: $0.label, value: PostalAddress(
                street: $0.street, city: $0.city, state: $0.state,
                postalCode: $0.postalCode, country: $0.country
            ))
        }

        contact.birthday = data.birthday
        if let imageData = data.imageData {
            contact.photoData = imageData
        }

        let parts = [contact.givenName, contact.middleName, contact.familyName].filter { !$0.isEmpty }
        contact.fullName = parts.joined(separator: " ")

        contact.conflictState = nil
        try await save(contact)
    }

    // MARK: - Syncthing groups

    func resolveSyncGroup(canonicalName: String, choiceIds: [String] = []) async throws {
        let session = try openSession()
        try session.resolveGroup(canonicalName: canonicalName, choiceIds: choiceIds)
        try refreshFromSession(session)
    }

    func syncConflictChoiceRows(canonicalName: String) throws -> [TextRow] {
        try openSession().conflictChoiceRows(canonicalName: canonicalName)
    }

    func conflictPreview(canonicalName: String) throws -> ConflictPreview {
        try openSession().conflictPreview(canonicalName: canonicalName)
    }

    // MARK: - Session

    @discardableResult
    private func persist(_ contact: Contact) throws -> ContactsSession {
        let session = try openSession()
        let saved = try session.saveContact(command: SaveContactCommand(draft: editDraft(from: contact)))
        guard let id = saved.id else { throw ContactsStoreError.missingSavedID }
        contact.localContactsID = id
        contact.contentToken = saved.contentToken
        contact.fileName = try session.fileName(id: id)
        return session
    }

    private func openSession() throws -> ContactsSession {
        if let session { return session }
        guard let url = folderURL else { throw ContactsStoreError.noFolder }
        let opened = try ContactsSession.open(root: url.path, device: deviceId)
        session = opened
        return opened
    }

    private func refreshFromSession(_ session: ContactsSession) throws {
        let rows = try session.listRows()
        var loaded: [Contact] = []
        for row in rows {
            let draft = try session.contactEditDraft(id: row.id)
            let fileName = try session.fileName(id: row.id)
            let contact = contact(from: draft, fileName: fileName)
            if let existing = contacts.first(where: {
                $0.localContactsID == contact.localContactsID
            }) {
                contact.conflictState = existing.conflictState
            }
            loaded.append(contact)
        }
        contacts = loaded
        syncConflictGroups = try session.conflictRows()
    }

    private func editDraft(from contact: Contact) -> ContactEditDraft {
        ContactEditDraft(
            id: contact.contentToken == nil ? nil : contact.localContactsID,
            contentToken: contact.contentToken,
            fullName: contact.fullName,
            familyName: contact.familyName,
            givenName: contact.givenName,
            middleName: contact.middleName,
            namePrefix: contact.namePrefix,
            nameSuffix: contact.nameSuffix,
            organization: contact.organization,
            jobTitle: contact.jobTitle,
            nickname: contact.nickname,
            urls: contact.urls.map { LabeledValueDraft(label: $0.label, value: $0.value) },
            phones: contact.phoneNumbers.map { LabeledValueDraft(label: $0.label, value: $0.value) },
            emails: contact.emailAddresses.map { LabeledValueDraft(label: $0.label, value: $0.value) },
            addresses: contact.postalAddresses.map {
                LabeledAddressDraft(
                    label: $0.label,
                    street: $0.value.street,
                    city: $0.value.city,
                    state: $0.value.state,
                    postalCode: $0.value.postalCode,
                    country: $0.value.country
                )
            },
            birthday: contact.birthday.flatMap {
                guard let month = $0.month, let day = $0.day else { return nil }
                return BirthdayDraft(
                    year: $0.year.map(Int32.init),
                    month: UInt8(clamping: month),
                    day: UInt8(clamping: day)
                )
            },
            note: contact.note,
            categories: contact.categories,
            photo: contact.photoData
        )
    }

    private func contact(from draft: ContactEditDraft, fileName: String) -> Contact {
        Contact(
            localContactsID: draft.id ?? UUID().uuidString,
            fileName: fileName,
            contentToken: draft.contentToken,
            fullName: draft.fullName,
            familyName: draft.familyName,
            givenName: draft.givenName,
            middleName: draft.middleName,
            namePrefix: draft.namePrefix,
            nameSuffix: draft.nameSuffix,
            organization: draft.organization,
            jobTitle: draft.jobTitle,
            nickname: draft.nickname,
            urls: draft.urls.map { LabeledValue(label: $0.label, value: $0.value) },
            phoneNumbers: draft.phones.map { LabeledValue(label: $0.label, value: $0.value) },
            emailAddresses: draft.emails.map { LabeledValue(label: $0.label, value: $0.value) },
            postalAddresses: draft.addresses.map {
                LabeledValue(
                    label: $0.label,
                    value: PostalAddress(
                        street: $0.street,
                        city: $0.city,
                        state: $0.state,
                        postalCode: $0.postalCode,
                        country: $0.country
                    )
                )
            },
            birthday: draft.birthday.map {
                DateComponents(
                    year: $0.year.map(Int.init),
                    month: Int($0.month),
                    day: Int($0.day)
                )
            },
            note: draft.note,
            categories: draft.categories,
            photoData: draft.photo
        )
    }
}

enum FolderLayoutMode: Equatable, Sendable {
    case empty
    case oneFilePerContact
    case singleFile(fileName: String)
    case mixed

    var label: String {
        switch self {
        case .empty: "Empty folder"
        case .oneFilePerContact: "One file per contact"
        case .singleFile(let name): "Single file (\(name))"
        case .mixed: "Mixed layout"
        }
    }

    var detail: String {
        switch self {
        case .empty:
            "No contacts yet. The folder layout will be determined by your first vCard files."
        case .oneFilePerContact:
            "Each contact lives in its own .vcf file. New contacts will be saved the same way."
        case .singleFile(let name):
            "All contacts share \(name). New contacts will be appended to it."
        case .mixed:
            "Some .vcf files contain multiple contacts while others contain one. Edits still persist, but a uniform layout (one file per contact, or one shared file) is recommended."
        }
    }

    var isSupported: Bool {
        if case .mixed = self { return false }
        return true
    }
}

enum ContactsStoreError: LocalizedError, Equatable {
    case noFolder
    case missingSavedID

    var errorDescription: String? {
        switch self {
        case .noFolder: "No folder selected. Please select a contacts folder first."
        case .missingSavedID: "The saved contact did not return an identifier."
        }
    }
}
