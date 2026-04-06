import Foundation
import Observation

@Observable
@MainActor
final class ContactsStore {
    var contacts: [Contact] = []
    var searchText: String = ""
    var selectedTag: String?
    var folderURL: URL?
    var isLoading = false
    var errorMessage: String?

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

        if let tag = selectedTag {
            result = result.filter { $0.categories.contains(tag) }
        }

        if !searchText.isEmpty {
            let query = searchText.lowercased()
            result = result.filter { contact in
                contact.displayName.lowercased().contains(query)
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
                    for contact in parsed {
                        // Migration: generate ID if missing
                        if contact.localContactsID.isEmpty {
                            contact.localContactsID = UUID().uuidString
                            // Write back with the new ID
                            let vcardString = writer.write(contact)
                            try? vcardString.data(using: .utf8)?.write(to: file)
                        }
                        // Preserve conflict state from existing contacts
                        if let existing = contacts.first(where: { $0.localContactsID == contact.localContactsID }) {
                            contact.conflictState = existing.conflictState
                        }
                        loaded.append(contact)
                    }
                } catch {
                    print("Warning: Could not parse \(file.lastPathComponent): \(error)")
                }
            }

            contacts = loaded
        } catch {
            errorMessage = "Failed to read folder: \(error.localizedDescription)"
        }

        isLoading = false
    }

    // MARK: - Save

    func save(_ contact: Contact) async throws {
        guard let url = folderURL else {
            throw ContactsStoreError.noFolder
        }

        let vcardString = writer.write(contact)
        guard let data = vcardString.data(using: .utf8) else {
            throw ContactsStoreError.encodingFailed
        }

        // Determine file name
        if contact.fileName.isEmpty {
            contact.fileName = uniqueFileName(for: contact, in: url)
        }

        let fileURL = url.appendingPathComponent(contact.fileName)
        try data.write(to: fileURL)

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
            if FileManager.default.fileExists(atPath: fileURL.path) {
                try FileManager.default.removeItem(at: fileURL)
            }
        }

        contacts.removeAll { $0.localContactsID == contact.localContactsID }

        // Also remove from CNContactStore
        try? await syncService.deleteContact(localContactsID: contact.localContactsID)
    }

    // MARK: - Import External Changes

    func applyExternalData(_ data: CNSyncService.CNContactData, to contact: Contact) async throws {
        contact.givenName = data.givenName
        contact.familyName = data.familyName
        contact.middleName = data.middleName
        contact.namePrefix = data.namePrefix
        contact.nameSuffix = data.nameSuffix

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
