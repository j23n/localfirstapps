import SwiftUI
import UIKit

struct ContactDetailView: View {
    @Environment(ContactsStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    let contact: Contact
    @State private var showEdit = false
    @State private var showDeleteConfirmation = false
    @State private var showConflictSheet = false

    var body: some View {
        List {
            // Hero Header
            Section {
                VStack(spacing: 12) {
                    AvatarView(contact: contact, size: 120)

                    Text(contact.displayName)
                        .font(.title2.bold())
                        .contextMenu { copyButton(contact.displayName) }

                    if !contact.organization.isEmpty || !contact.jobTitle.isEmpty {
                        let orgLine = [contact.jobTitle, contact.organization].filter { !$0.isEmpty }.joined(separator: " — ")
                        Text(orgLine)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .contextMenu { copyButton(orgLine) }
                    }

                    if !contact.nickname.isEmpty {
                        Text("\"\(contact.nickname)\"")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .italic()
                            .contextMenu { copyButton(contact.nickname) }
                    }

                    if !contact.categories.isEmpty {
                        HStack(spacing: 6) {
                            ForEach(contact.categories, id: \.self) { tag in
                                Text(tag)
                                    .font(.caption)
                                    .padding(.horizontal, 8)
                                    .padding(.vertical, 4)
                                    .background(Color.accentColor.opacity(0.12), in: Capsule())
                                    .foregroundStyle(Color.accentColor)
                            }
                        }
                    }
                }
                .frame(maxWidth: .infinity)
                .listRowBackground(Color.clear)
            }

            // Conflict Banner
            if contact.conflictState != nil {
                Section {
                    Button {
                        showConflictSheet = true
                    } label: {
                        HStack(spacing: 12) {
                            Image(systemName: contact.conflictState?.isExternalEdit == true
                                  ? "pencil.circle.fill" : "trash.circle.fill")
                                .font(.title3)
                                .foregroundStyle(.orange)

                            VStack(alignment: .leading, spacing: 2) {
                                Text(contact.conflictState?.isExternalEdit == true
                                     ? "External Edit Detected"
                                     : "External Deletion Detected")
                                    .font(.subheadline.weight(.medium))
                                Text("Tap to resolve")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }

                            Spacer()

                            Image(systemName: "chevron.right")
                                .font(.caption)
                                .foregroundStyle(.tertiary)
                        }
                    }
                    .tint(.primary)
                }
            }

            // Phone Numbers
            if !contact.phoneNumbers.isEmpty {
                Section("Phone") {
                    ForEach(contact.phoneNumbers) { phone in
                        Group {
                            if let url = Self.dialURL(phone.value) {
                                Link(destination: url) {
                                    valueRow(label: phone.label, value: phone.value)
                                }
                            } else {
                                valueRow(label: phone.label, value: phone.value)
                            }
                        }
                        .contextMenu { copyButton(phone.value) }
                    }
                }
            }

            // Emails
            if !contact.emailAddresses.isEmpty {
                Section("Email") {
                    ForEach(contact.emailAddresses) { email in
                        Group {
                            if let url = Self.mailURL(email.value) {
                                Link(destination: url) {
                                    valueRow(label: email.label, value: email.value)
                                }
                            } else {
                                valueRow(label: email.label, value: email.value)
                            }
                        }
                        .contextMenu { copyButton(email.value) }
                    }
                }
            }

            // URLs
            if !contact.urls.isEmpty {
                Section("Website") {
                    ForEach(contact.urls) { url in
                        if let linkURL = Self.websiteURL(url.value) {
                            Link(destination: linkURL) {
                                LabeledContent {
                                    Text(url.value)
                                        .foregroundStyle(Color.accentColor)
                                        .lineLimit(1)
                                } label: {
                                    Text(url.label.capitalized)
                                        .foregroundStyle(.secondary)
                                }
                            }
                            .contextMenu { copyButton(url.value) }
                        }
                    }
                }
            }

            // Addresses
            if !contact.postalAddresses.isEmpty {
                Section("Address") {
                    ForEach(contact.postalAddresses) { addr in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(addr.label.capitalized)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            Text(addr.value.formatted)
                                .font(.body)
                        }
                        .contextMenu { copyButton(addr.value.formatted) }
                    }
                }
            }

            // Birthday
            if let bday = contact.birthday, let month = bday.month, let day = bday.day {
                Section("Birthday") {
                    let bdayText: String = {
                        if let year = bday.year {
                            return birthdayString(year: year, month: month, day: day)
                        } else {
                            return birthdayString(month: month, day: day)
                        }
                    }()
                    HStack {
                        Text(bdayText)
                        if let age = contact.age {
                            Spacer()
                            Text("Age \(age)")
                                .foregroundStyle(.secondary)
                        }
                    }
                    .contextMenu { copyButton(bdayText) }
                }
            }

            // Notes
            if !contact.note.isEmpty {
                Section("Notes") {
                    Text(contact.note)
                        .contextMenu { copyButton(contact.note) }
                }
            }

            // Delete
            Section {
                Button(role: .destructive) {
                    showDeleteConfirmation = true
                } label: {
                    HStack {
                        Spacer()
                        Text("Delete Contact")
                        Spacer()
                    }
                }
            }
        }
        .listStyle(.insetGrouped)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button("Edit") {
                    showEdit = true
                }
            }
        }
        .sheet(isPresented: $showEdit) {
            NavigationStack {
                ContactEditView(contact: contact.copy(), isNew: false)
            }
        }
        .onChange(of: showEdit) { _, isEditing in
            store.isSuppressingReload = isEditing
        }
        .sheet(isPresented: $showConflictSheet) {
            ConflictResolutionSheet(contact: contact)
        }
        .confirmationDialog("Delete Contact", isPresented: $showDeleteConfirmation, titleVisibility: .visible) {
            Button("Delete", role: .destructive) {
                Task {
                    do {
                        try await store.delete(contact)
                        dismiss()
                    } catch {
                        store.errorMessage = error.localizedDescription
                    }
                }
            }
        } message: {
            Text("This will permanently delete \(contact.displayName) and remove the .vcf file.")
        }
    }

    @ViewBuilder
    private func valueRow(label: String, value: String) -> some View {
        LabeledContent {
            Text(value)
                .foregroundStyle(Color.accentColor)
        } label: {
            Text(label.capitalized)
                .foregroundStyle(.secondary)
        }
    }

    /// Build a `tel:` URL from a phone number, keeping only characters the URL
    /// scheme accepts. Returns `nil` (so the caller renders a plain, non-tappable
    /// row instead of crashing) when nothing dialable remains. Formatted numbers
    /// like "+1 (555) 123-4567" otherwise produce a nil URL that force-unwrapping
    /// would trap on.
    static func dialURL(_ phone: String) -> URL? {
        let allowed = CharacterSet(charactersIn: "+0123456789*#,;")
        let filtered = String(phone.unicodeScalars.filter { allowed.contains($0) })
        guard !filtered.isEmpty else { return nil }
        return URL(string: "tel:\(filtered)")
    }

    /// Build a `mailto:` URL, percent-encoding as needed. Returns `nil` for an
    /// empty or unencodable address so the caller can fall back to a plain row.
    static func mailURL(_ email: String) -> URL? {
        let trimmed = email.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }
        if let url = URL(string: "mailto:\(trimmed)") {
            return url
        }
        guard let encoded = trimmed.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) else {
            return nil
        }
        return URL(string: "mailto:\(encoded)")
    }

    /// Accept `http(s)://` case-insensitively; otherwise prepend `https://`.
    static func websiteURL(_ raw: String) -> URL? {
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }
        let lower = trimmed.lowercased()
        if lower.hasPrefix("http://") || lower.hasPrefix("https://") {
            return URL(string: trimmed)
        }
        return URL(string: "https://\(trimmed)")
    }

    @ViewBuilder
    private func copyButton(_ value: String) -> some View {
        Button {
            UIPasteboard.general.string = value
            UINotificationFeedbackGenerator().notificationOccurred(.success)
        } label: {
            Label("Copy", systemImage: "doc.on.doc")
        }
    }

    private func birthdayString(year: Int, month: Int, day: Int) -> String {
        let formatter = DateFormatter()
        formatter.dateStyle = .long
        let date = Calendar.current.date(from: DateComponents(year: year, month: month, day: day))!
        return formatter.string(from: date)
    }

    private func birthdayString(month: Int, day: Int) -> String {
        let formatter = DateFormatter()
        formatter.dateFormat = "MMMM d"
        let date = Calendar.current.date(from: DateComponents(month: month, day: day))!
        return formatter.string(from: date)
    }
}
