import Foundation
import Observation
import os

@Observable
@MainActor
final class ContactsStore {
    var contacts: [Contact] = []
    var searchText: String = ""
    var selectedTag: String?
    var showConflictsOnly = false
    var folderURL: URL?
    var isLoading = false
    var isSuppressingReload = false
    var errorMessage: String?
    var lastSyncedAt: Date?
    /// Syncthing `.vcf` groups (ADR 0005 R8). Not Apple CN conflicts.
    var syncConflictGroups: [TextRow] = []

    private let parser = VCardParser()
    private let writer = VCardWriter()
    private var session: ContactsSession?
    let bookmarkManager = BookmarkManager()
    let folderAccess = FolderAccessManager()
    let syncService: CNSyncService

    init(syncService: CNSyncService = CNSyncService()) {
        self.syncService = syncService
    }

    // MARK: - Computed

    var allTags: [(tag: String, count: Int)] {
        var tagCounts: [String: Int] = [:]
        for contact in contacts {
            for tag in contact.categories {
                tagCounts[tag, default: 0] += 1
            }
        }
        return tagCounts.map { (tag: $0.key, count: $0.value) }.sorted { $0.tag < $1.tag }
    }

    var filteredContacts: [Contact] {
        var result = contacts

        if showConflictsOnly {
            result = result.filter { $0.conflictState != nil }
        } else if let tag = selectedTag {
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

        return result.sorted { $0.displayName.localizedCaseInsensitiveCompare($1.displayName) == .orderedAscending }
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
        if contacts.isEmpty { isLoading = true }
        errorMessage = nil

        do {
            let opened = try ContactsSession.open(root: url.path)
            session = opened
            try opened.reload()
            try refreshFromSession(opened)
        } catch {
            errorMessage = "Failed to read folder: \(error.localizedDescription)"
        }

        lastSyncedAt = Date()
        isLoading = false
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

        var touchedFiles = Set<String>()
        for contact in contacts {
            if let index = contact.categories.firstIndex(of: oldName) {
                contact.categories[index] = trimmed
                // Deduplicate if new name already existed on this contact
                var seen = Set<String>()
                contact.categories = contact.categories.filter { seen.insert($0).inserted }
                touchedFiles.insert(contact.fileName)
            }
        }

        let session = try openSession()
        for contact in contacts where touchedFiles.contains(contact.fileName) {
            _ = try persist(contact)
        }
        try refreshFromSession(session)

        if selectedTag == oldName {
            selectedTag = trimmed
        }
    }

    func deleteTag(_ tagName: String) async throws {
        var touchedFiles = Set<String>()
        for contact in contacts {
            if contact.categories.contains(tagName) {
                contact.categories.removeAll { $0 == tagName }
                touchedFiles.insert(contact.fileName)
            }
        }

        let session = try openSession()
        for contact in contacts where touchedFiles.contains(contact.fileName) {
            _ = try persist(contact)
        }
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
        for contact in toDelete {
            try session.delete(id: contact.localContactsID)
        }
        contacts.removeAll { contactIDs.contains($0.localContactsID) }

        for contact in toDelete {
            try? await syncService.deleteContact(localContactsID: contact.localContactsID)
        }
        syncConflictGroups = (try? session.conflictRows()) ?? []
    }

    func assignTag(_ tag: String, to contactIDs: Set<String>) async throws {
        var touchedFiles = Set<String>()
        for id in contactIDs {
            if let contact = contacts.first(where: { $0.localContactsID == id }),
               !contact.categories.contains(tag) {
                contact.categories.append(tag)
                touchedFiles.insert(contact.fileName)
            }
        }

        let session = try openSession()
        for contact in contacts where touchedFiles.contains(contact.fileName) {
            _ = try persist(contact)
        }
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

    // MARK: - Session

    @discardableResult
    private func persist(_ contact: Contact) throws -> ContactsSession {
        let session = try openSession()
        let id = try session.saveVcard(text: writer.write(contact), fileName: contact.fileName)
        contact.localContactsID = id
        contact.fileName = try session.fileName(id: id)
        return session
    }

    private func openSession() throws -> ContactsSession {
        if let session { return session }
        guard let url = folderURL else { throw ContactsStoreError.noFolder }
        let opened = try ContactsSession.open(root: url.path)
        session = opened
        return opened
    }

    private func refreshFromSession(_ session: ContactsSession) throws {
        let rows = try session.listRows()
        var loaded: [Contact] = []
        for row in rows {
            let text = try session.vcardText(id: row.id)
            let fileName = try session.fileName(id: row.id)
            guard let data = text.data(using: .utf8) else { continue }
            let parsed = parser.parseMultiple(
                data: data,
                fileName: fileName,
                assignDefaultID: false
            )
            for contact in parsed {
                if let existing = contacts.first(where: {
                    $0.localContactsID == contact.localContactsID
                }) {
                    contact.conflictState = existing.conflictState
                }
                loaded.append(contact)
            }
        }
        contacts = loaded
        syncConflictGroups = try session.conflictRows()
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
    case encodingFailed

    var errorDescription: String? {
        switch self {
        case .noFolder: "No folder selected. Please select a contacts folder first."
        case .encodingFailed: "Failed to encode vCard data."
        }
    }
}
