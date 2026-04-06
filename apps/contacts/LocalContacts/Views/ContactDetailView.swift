import SwiftUI

struct ContactDetailView: View {
    @Environment(ContactsStore.self) private var store
    let contact: Contact
    @State private var showEdit = false
    @State private var showDeleteConfirmation = false
    @State private var showConflictSheet = false

    var body: some View {
        List {
            // Header
            Section {
                VStack(spacing: 12) {
                    AvatarView(contact: contact, size: 100)
                    Text(contact.displayName)
                        .font(.title2.bold())

                    if !contact.categories.isEmpty {
                        HStack(spacing: 6) {
                            ForEach(contact.categories, id: \.self) { tag in
                                Text(tag)
                                    .font(.caption)
                                    .padding(.horizontal, 8)
                                    .padding(.vertical, 4)
                                    .background(Color.accentColor.opacity(0.15), in: Capsule())
                                    .foregroundStyle(Color.accentColor)
                            }
                        }
                    }
                }
                .frame(maxWidth: .infinity)
                .listRowBackground(Color.clear)
            }

            // Quick Actions
            if !contact.phoneNumbers.isEmpty || !contact.emailAddresses.isEmpty {
                Section {
                    HStack(spacing: 16) {
                        if let phone = contact.phoneNumbers.first {
                            actionButton(icon: "phone.fill", label: "Call") {
                                openURL("tel:\(phone.value)")
                            }
                            actionButton(icon: "message.fill", label: "Message") {
                                openURL("sms:\(phone.value)")
                            }
                            actionButton(icon: "video.fill", label: "FaceTime") {
                                openURL("facetime:\(phone.value)")
                            }
                        }
                        if let email = contact.emailAddresses.first {
                            actionButton(icon: "envelope.fill", label: "Mail") {
                                openURL("mailto:\(email.value)")
                            }
                        }
                    }
                    .frame(maxWidth: .infinity)
                    .listRowBackground(Color.clear)
                }
            }

            // Phone Numbers
            if !contact.phoneNumbers.isEmpty {
                Section("Phone") {
                    ForEach(contact.phoneNumbers) { phone in
                        Button {
                            openURL("tel:\(phone.value)")
                        } label: {
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
                        Button {
                            openURL("mailto:\(email.value)")
                        } label: {
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
                }
            }
        } message: {
            Text("This will permanently delete \(contact.displayName) and remove the .vcf file.")
        }
        .onAppear {
            if contact.conflictState != nil {
                showConflictSheet = true
            }
        }
    }

    private func actionButton(icon: String, label: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            VStack(spacing: 4) {
                Image(systemName: icon)
                    .font(.title3)
                Text(label)
                    .font(.caption2)
            }
            .frame(width: 60, height: 50)
            .background(Color(.systemGray6), in: RoundedRectangle(cornerRadius: 10))
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

    private func openURL(_ urlString: String) {
        guard let url = URL(string: urlString) else { return }
        UIApplication.shared.open(url)
    }
}
