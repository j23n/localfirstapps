import Foundation
import Observation

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

    private let parser = VCardParser()
    private let writer = VCardWriter()
    let bookmarkManager = BookmarkManager()
    let folderAccess = FolderAccessManager()
    let syncService = CNSyncService()

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
            await loadContacts()
        }
    }

    func restoreFolder() async {
        guard let url = bookmarkManager.loadBookmark() else { return }
        await folderAccess.startAccessing(url)
        folderURL = url
        await loadContacts()
    }

    // MARK: - Load

    func loadContacts() async {
        guard let url = folderURL else { return }
        guard !isSuppressingReload else { return }
        if contacts.isEmpty { isLoading = true }
        errorMessage = nil

        do {
            let contents = try FileManager.default.contentsOfDirectory(
                at: url,
                includingPropertiesForKeys: [.isRegularFileKey],
                options: [.skipsHiddenFiles]
            )

            let vcfFiles = contents.filter { $0.pathExtension.lowercased() == "vcf" }
            var loaded: [Contact] = []

            for file in vcfFiles {
                do {
                    let data = try Data(contentsOf: file)
                    let parsed = parser.parseMultiple(data: data, fileName: file.lastPathComponent)
                    var needsRewrite = false
                    for contact in parsed {
                        // Migration: generate ID if missing. We rewrite the whole
                        // file once below so multi-vCard files don't lose siblings.
                        if contact.localContactsID.isEmpty {
                            contact.localContactsID = UUID().uuidString
                            needsRewrite = true
                        }
                        // Preserve conflict state from existing contacts
                        if let existing = contacts.first(where: { $0.localContactsID == contact.localContactsID }) {
                            contact.conflictState = existing.conflictState
                        }
                        loaded.append(contact)
                    }
                    if needsRewrite {
                        let combined = parsed.map { writer.write($0) }.joined()
                        try? combined.data(using: .utf8)?.write(to: file, options: .atomic)
                    }
                } catch {
                    print("Warning: Could not parse \(file.lastPathComponent): \(error)")
                }
            }

            contacts = loaded
        } catch {
            errorMessage = "Failed to read folder: \(error.localizedDescription)"
        }

        lastSyncedAt = Date()
        isLoading = false
    }

    // MARK: - Save

    func save(_ contact: Contact) async throws {
        guard let url = folderURL else {
            throw ContactsStoreError.noFolder
        }

        if contact.fileName.isEmpty {
            contact.fileName = uniqueFileName(for: contact, in: url)
        }

        // Rewrite the whole file so siblings in a multi-vCard file are preserved.
        var fileContacts = contacts.filter {
            $0.fileName == contact.fileName && $0.localContactsID != contact.localContactsID
        }
        fileContacts.append(contact)

        let vcardString = fileContacts.map { writer.write($0) }.joined()
        guard let data = vcardString.data(using: .utf8) else {
            throw ContactsStoreError.encodingFailed
        }

        let fileURL = url.appendingPathComponent(contact.fileName)
        try data.write(to: fileURL, options: .atomic)

        // Update in-memory
        if let index = contacts.firstIndex(where: { $0.localContactsID == contact.localContactsID }) {
            contacts[index] = contact
        } else {
            contacts.append(contact)
        }
    }

    // MARK: - Delete

    func delete(_ contact: Contact) async throws {
        guard let url = folderURL else {
            throw ContactsStoreError.noFolder
        }

        if !contact.fileName.isEmpty {
            let fileURL = url.appendingPathComponent(contact.fileName)
            let remaining = contacts.filter {
                $0.fileName == contact.fileName && $0.localContactsID != contact.localContactsID
            }
            if remaining.isEmpty {
                if FileManager.default.fileExists(atPath: fileURL.path) {
                    try FileManager.default.removeItem(at: fileURL)
                }
            } else {
                let vcardString = remaining.map { writer.write($0) }.joined()
                guard let data = vcardString.data(using: .utf8) else {
                    throw ContactsStoreError.encodingFailed
                }
                try data.write(to: fileURL, options: .atomic)
            }
        }

        contacts.removeAll { $0.localContactsID == contact.localContactsID }

        // Also remove from CNContactStore
        try? await syncService.deleteContact(localContactsID: contact.localContactsID)
    }

    // MARK: - Tag Management

    func renameTag(_ oldName: String, to newName: String) async throws {
        let trimmed = newName.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty, trimmed != oldName else { return }

        for contact in contacts {
            if let index = contact.categories.firstIndex(of: oldName) {
                contact.categories[index] = trimmed
                // Deduplicate if new name already existed on this contact
                var seen = Set<String>()
                contact.categories = contact.categories.filter { seen.insert($0).inserted }
                try await save(contact)
            }
        }

        if selectedTag == oldName {
            selectedTag = trimmed
        }
    }

    func deleteTag(_ tagName: String) async throws {
        for contact in contacts {
            if contact.categories.contains(tagName) {
                contact.categories.removeAll { $0 == tagName }
                try await save(contact)
            }
        }

        if selectedTag == tagName {
            selectedTag = nil
        }
    }

    // MARK: - Bulk Operations

    func deleteMultiple(_ contactIDs: Set<String>) async throws {
        for id in contactIDs {
            if let contact = contacts.first(where: { $0.localContactsID == id }) {
                try await delete(contact)
            }
        }
    }

    func assignTag(_ tag: String, to contactIDs: Set<String>) async throws {
        for id in contactIDs {
            if let contact = contacts.first(where: { $0.localContactsID == id }),
               !contact.categories.contains(tag) {
                contact.categories.append(tag)
                try await save(contact)
            }
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

    // MARK: - Helpers

    private func uniqueFileName(for contact: Contact, in folder: URL) -> String {
        let base = writer.suggestedFileName(for: contact)
        let name = (base as NSString).deletingPathExtension
        let ext = (base as NSString).pathExtension

        var candidate = base
        var counter = 1
        while FileManager.default.fileExists(atPath: folder.appendingPathComponent(candidate).path) {
            candidate = "\(name)-\(counter).\(ext)"
            counter += 1
        }
        return candidate
    }
}

enum ContactsStoreError: LocalizedError {
    case noFolder
    case encodingFailed

    var errorDescription: String? {
        switch self {
        case .noFolder: "No folder selected. Please select a contacts folder first."
        case .encodingFailed: "Failed to encode vCard data."
        }
    }
}
