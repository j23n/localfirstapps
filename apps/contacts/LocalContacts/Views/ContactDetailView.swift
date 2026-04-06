import SwiftUI

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

                    if !contact.organization.isEmpty || !contact.jobTitle.isEmpty {
                        Text([contact.jobTitle, contact.organization].filter { !$0.isEmpty }.joined(separator: " — "))
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }

                    if !contact.nickname.isEmpty {
                        Text("\"\(contact.nickname)\"")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .italic()
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
                            Image(systemName: contact.conflictState == .externalEdit
                                  ? "pencil.circle.fill" : "trash.circle.fill")
                                .font(.title3)
                                .foregroundStyle(.orange)

                            VStack(alignment: .leading, spacing: 2) {
                                Text(contact.conflictState == .externalEdit
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
                        Link(destination: URL(string: "tel:\(phone.value)")!) {
                            LabeledContent {
                                Text(phone.value)
                                    .foregroundStyle(Color.accentColor)
                            } label: {
                                Text(phone.label.capitalized)
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                }
            }

            // Emails
            if !contact.emailAddresses.isEmpty {
                Section("Email") {
                    ForEach(contact.emailAddresses) { email in
                        Link(destination: URL(string: "mailto:\(email.value)")!) {
                            LabeledContent {
                                Text(email.value)
                                    .foregroundStyle(Color.accentColor)
                            } label: {
                                Text(email.label.capitalized)
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                }
            }

            // URLs
            if !contact.urls.isEmpty {
                Section("Website") {
                    ForEach(contact.urls) { url in
                        if let linkURL = URL(string: url.value.hasPrefix("http") ? url.value : "https://\(url.value)") {
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
                    }
                }
            }

            // Birthday
            if let bday = contact.birthday, let month = bday.month, let day = bday.day {
                Section("Birthday") {
                    HStack {
                        if let year = bday.year {
                            Text(birthdayString(year: year, month: month, day: day))
                        } else {
                            Text(birthdayString(month: month, day: day))
                        }
                        if let age = contact.age {
                            Spacer()
                            Text("Age \(age)")
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }

            // Notes
            if !contact.note.isEmpty {
                Section("Notes") {
                    Text(contact.note)
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
        .sheet(isPresented: $showConflictSheet) {
            ConflictResolutionSheet(contact: contact)
        }
        .confirmationDialog("Delete Contact", isPresented: $showDeleteConfirmation, titleVisibility: .visible) {
            Button("Delete", role: .destructive) {
                Task {
                    try? await store.delete(contact)
                    dismiss()
                }
            }
        } message: {
            Text("This will permanently delete \(contact.displayName) and remove the .vcf file.")
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
