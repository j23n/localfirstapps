import SwiftUI
import UIKit

struct ContactDetailView: View {
    @Environment(ContactsStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    let contact: Contact
    @State private var showEdit = false
    @State private var editDraft: ContactEditDraft?
    @State private var showDeleteConfirmation = false
    @State private var showConflictSheet = false
    @State private var fields: [FieldRow] = []

    private var displayed: Contact {
        store.contacts.first { $0.localContactsID == contact.localContactsID } ?? contact
    }

    var body: some View {
        List {
            heroSection

            if displayed.conflictState != nil {
                conflictBanner
            }

            ForEach(Self.sections(from: fields), id: \.title) { section in
                Section(section.title) {
                    ForEach(section.rows, id: \.offset) { item in
                        fieldRowView(item.row)
                    }
                }
            }

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
                HStack {
                    Button {
                        exportContact()
                    } label: {
                        Image(systemName: "square.and.arrow.up")
                    }
                    .accessibilityLabel("Export")

                    Button("Edit") {
                        openEditor()
                    }
                }
            }
        }
        .sheet(isPresented: $showEdit) {
            if let editDraft {
                NavigationStack {
                    ContactsRouter.destination(
                        ContactEditView(draft: editDraft, isNew: false)
                    )
                }
            }
        }
        .onChange(of: showEdit) { _, isEditing in
            store.isSuppressingReload = isEditing
            if !isEditing {
                editDraft = nil
                reloadFields()
            }
        }
        .sheet(isPresented: $showConflictSheet) {
            ContactsRouter.destination(ConflictResolutionSheet(contact: displayed))
        }
        .confirmationDialog("Delete Contact", isPresented: $showDeleteConfirmation, titleVisibility: .visible) {
            Button("Delete", role: .destructive) {
                Task {
                    do {
                        try await store.delete(displayed)
                        dismiss()
                    } catch {
                        store.errorMessage = error.localizedDescription
                    }
                }
            }
        } message: {
            Text("This will permanently delete \(displayed.displayName) and remove the .vcf file.")
        }
        .onAppear { reloadFields() }
        .onChange(of: displayed.contentToken) { _, _ in
            reloadFields()
        }
    }

    @ViewBuilder
    private var heroSection: some View {
        Section {
            VStack(spacing: 12) {
                AvatarView(contact: displayed, size: 120)

                Text(displayed.displayName)
                    .font(.title2.bold())
                    .contextMenu { copyButton(displayed.displayName) }

                if !displayed.organization.isEmpty || !displayed.jobTitle.isEmpty {
                    let orgLine = [displayed.jobTitle, displayed.organization]
                        .filter { !$0.isEmpty }
                        .joined(separator: " — ")
                    Text(orgLine)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .contextMenu { copyButton(orgLine) }
                }

                if !displayed.nickname.isEmpty {
                    Text("\"\(displayed.nickname)\"")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .italic()
                        .contextMenu { copyButton(displayed.nickname) }
                }

                if !displayed.categories.isEmpty {
                    HStack(spacing: 6) {
                        ForEach(displayed.categories, id: \.self) { tag in
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
    }

    @ViewBuilder
    private var conflictBanner: some View {
        Section {
            Button {
                showConflictSheet = true
            } label: {
                HStack(spacing: 12) {
                    Image(systemName: displayed.conflictState?.isExternalEdit == true
                          ? "pencil.circle.fill" : "trash.circle.fill")
                        .font(.title3)
                        .foregroundStyle(.orange)

                    VStack(alignment: .leading, spacing: 2) {
                        Text(displayed.conflictState?.isExternalEdit == true
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

    @ViewBuilder
    private func fieldRowView(_ row: FieldRow) -> some View {
        Group {
            if let id = row.id, id.hasPrefix("tel:"), let url = Self.dialURL(row.value) {
                Link(destination: url) {
                    valueRow(label: row.label, value: row.value)
                }
            } else if let id = row.id, id.hasPrefix("email:"), let url = Self.mailURL(row.value) {
                Link(destination: url) {
                    valueRow(label: row.label, value: row.value)
                }
            } else if let id = row.id, id.hasPrefix("url:"), let url = Self.websiteURL(row.value) {
                Link(destination: url) {
                    LabeledContent {
                        Text(row.value)
                            .foregroundStyle(Color.accentColor)
                            .lineLimit(1)
                    } label: {
                        Text(row.label)
                            .foregroundStyle(.secondary)
                    }
                }
            } else if row.id == "bday" {
                HStack {
                    Text(row.value)
                    if let age = displayed.age {
                        Spacer()
                        Text("Age \(age)")
                            .foregroundStyle(.secondary)
                    }
                }
            } else {
                LabeledContent {
                    Text(row.value)
                } label: {
                    Text(row.label)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .contextMenu { copyButton(row.value) }
    }

    @ViewBuilder
    private func valueRow(label: String, value: String) -> some View {
        LabeledContent {
            Text(value)
                .foregroundStyle(Color.accentColor)
        } label: {
            Text(label)
                .foregroundStyle(.secondary)
        }
    }

    private func reloadFields() {
        fields = store.fieldRows(id: displayed.localContactsID)
    }

    private func openEditor() {
        do {
            editDraft = try store.contactEditDraft(id: displayed.localContactsID)
            showEdit = true
        } catch {
            store.errorMessage = error.localizedDescription
        }
    }

    private func exportContact() {
        do {
            let text = try store.exportVCardText(id: displayed.localContactsID)
            let name = displayed.fileName.isEmpty ? "contact.vcf" : displayed.fileName
            let url = FileManager.default.temporaryDirectory.appendingPathComponent(name)
            try text.write(to: url, atomically: true, encoding: .utf8)
            ShareSheet.present(items: [url])
        } catch {
            store.errorMessage = error.localizedDescription
        }
    }

    /// Build a `tel:` URL from a phone number, keeping only characters the URL
    /// scheme accepts. Returns `nil` (so the caller renders a plain, non-tappable
    /// row instead of crashing) when nothing dialable remains. Formatted numbers
    /// like "+1 (555) 123-4567" otherwise produce a nil URL that force-unwrapping
    /// would trap on.
    nonisolated static func dialURL(_ phone: String) -> URL? {
        let allowed = CharacterSet(charactersIn: "+0123456789*#,;")
        let filtered = String(phone.unicodeScalars.filter { allowed.contains($0) })
        guard !filtered.isEmpty else { return nil }
        return URL(string: "tel:\(filtered)")
    }

    /// Build a `mailto:` URL, percent-encoding as needed. Returns `nil` for an
    /// empty or unencodable address so the caller can fall back to a plain row.
    nonisolated static func mailURL(_ email: String) -> URL? {
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
    nonisolated static func websiteURL(_ raw: String) -> URL? {
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

    static func isHeroField(_ row: FieldRow) -> Bool {
        switch row.id {
        case "fn", "org", "title", "nickname", "photo":
            return true
        case let id? where id.hasPrefix("category:"):
            return true
        default:
            return false
        }
    }

    static func sectionTitle(for row: FieldRow) -> String {
        guard let id = row.id else { return row.label }
        if id.hasPrefix("tel:") { return "Phone" }
        if id.hasPrefix("email:") { return "Email" }
        if id.hasPrefix("url:") { return "Website" }
        if id.hasPrefix("adr:") { return "Address" }
        if id == "bday" { return "Birthday" }
        if id == "note" { return "Notes" }
        return row.label
    }

    static func sections(from rows: [FieldRow]) -> [(title: String, rows: [(offset: Int, row: FieldRow)])] {
        let order = ["Phone", "Email", "Website", "Address", "Birthday", "Notes"]
        var grouped: [String: [(offset: Int, row: FieldRow)]] = [:]
        for (offset, row) in rows.enumerated() where !isHeroField(row) {
            grouped[sectionTitle(for: row), default: []].append((offset, row))
        }
        var result: [(title: String, rows: [(offset: Int, row: FieldRow)])] = []
        for title in order {
            if let rows = grouped.removeValue(forKey: title) {
                result.append((title, rows))
            }
        }
        for title in grouped.keys.sorted() {
            if let rows = grouped[title] {
                result.append((title, rows))
            }
        }
        return result
    }
}
