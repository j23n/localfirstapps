import SwiftUI

struct ContactListView: View {
    @Environment(ContactsStore.self) private var store
    @State private var showSettings = false
    @State private var showAddContact = false

    var body: some View {
        @Bindable var store = store

        NavigationStack {
            Group {
                if store.isLoading {
                    ProgressView("Loading contacts...")
                } else if store.contacts.isEmpty {
                    emptyState
                } else {
                    contactList
                }
            }
            .navigationTitle("Contacts")
            .searchable(text: $store.searchText, prompt: "Name, phone, or email")
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button {
                        showSettings = true
                    } label: {
                        Image(systemName: "gear")
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        showAddContact = true
                    } label: {
                        Image(systemName: "plus")
                    }
                }
            }
            .sheet(isPresented: $showSettings) {
                SettingsView()
            }
            .sheet(isPresented: $showAddContact) {
                NavigationStack {
                    ContactEditView(contact: Contact(), isNew: true)
                }
            }
            .refreshable {
                await store.loadContacts()
            }
            .alert("Error", isPresented: .init(
                get: { store.errorMessage != nil },
                set: { if !$0 { store.errorMessage = nil } }
            )) {
                Button("OK") { store.errorMessage = nil }
            } message: {
                Text(store.errorMessage ?? "")
            }
        }
    }

    private var emptyState: some View {
        ContentUnavailableView {
            Label("No Contacts", systemImage: "person.crop.circle.badge.questionmark")
        } description: {
            Text("Add a contact or place .vcf files in your selected folder.")
        } actions: {
            Button("Add Contact") {
                showAddContact = true
            }
            .buttonStyle(.borderedProminent)
        }
    }

    private var contactList: some View {
        VStack(spacing: 0) {
            if !store.allTags.isEmpty || store.hasConflicts {
                TagFilterBar()
            }

            List {
                ForEach(store.groupedContacts, id: \.letter) { group in
                    Section(group.letter) {
                        ForEach(group.contacts) { contact in
                            NavigationLink(value: contact.id) {
                                ContactCard(contact: contact)
                            }
                        }
                    }
                }
            }
            .listStyle(.insetGrouped)
            .navigationDestination(for: UUID.self) { contactID in
                if let contact = store.contacts.first(where: { $0.id == contactID }) {
                    ContactDetailView(contact: contact)
                }
            }
            .overlay {
                if store.filteredContacts.isEmpty && !store.searchText.isEmpty {
                    ContentUnavailableView.search(text: store.searchText)
                }
            }
        }
    }
}

// MARK: - Contact Card

struct ContactCard: View {
    let contact: Contact

    var body: some View {
        HStack(spacing: 14) {
            AvatarView(contact: contact, size: 48)

            VStack(alignment: .leading, spacing: 3) {
                Text(contact.displayName)
                    .font(.body.weight(.medium))

                if !contact.organization.isEmpty || !contact.jobTitle.isEmpty {
                    Text([contact.jobTitle, contact.organization].filter { !$0.isEmpty }.joined(separator: " — "))
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }

                if let detail = contactDetail {
                    Text(detail)
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)
                }
            }

            Spacer()

            if contact.conflictState != nil {
                Image(systemName: "exclamationmark.circle.fill")
                    .foregroundStyle(.orange)
                    .font(.subheadline)
            }
        }
        .padding(.vertical, 4)
    }

    private var contactDetail: String? {
        if let phone = contact.phoneNumbers.first {
            return phone.value
        }
        if let email = contact.emailAddresses.first {
            return email.value
        }
        return nil
    }
}

// MARK: - Avatar

struct AvatarView: View {
    let contact: Contact
    let size: CGFloat

    var body: some View {
        if let photoData = contact.photoData,
           let uiImage = UIImage(data: photoData) {
            Image(uiImage: uiImage)
                .resizable()
                .scaledToFill()
                .frame(width: size, height: size)
                .clipShape(Circle())
        } else {
            Text(contact.initials)
                .font(.system(size: size * 0.36, weight: .medium, design: .rounded))
                .foregroundStyle(.white)
                .frame(width: size, height: size)
                .background(Color.accentColor.opacity(0.8).gradient, in: Circle())
        }
    }
}

// MARK: - Tag Filter Bar

struct TagFilterBar: View {
    @Environment(ContactsStore.self) private var store

    var body: some View {
        @Bindable var store = store

        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                TagChip(title: "All", isSelected: store.selectedTag == nil && !store.showConflictsOnly) {
                    store.selectedTag = nil
                    store.showConflictsOnly = false
                }

                if store.hasConflicts {
                    let count = store.contacts.filter { $0.conflictState != nil }.count
                    TagChip(
                        title: "Conflicts (\(count))",
                        isSelected: store.showConflictsOnly,
                        tint: .orange
                    ) {
                        store.showConflictsOnly.toggle()
                        if store.showConflictsOnly { store.selectedTag = nil }
                    }
                }

                ForEach(store.allTags, id: \.tag) { tagInfo in
                    TagChip(
                        title: "\(tagInfo.tag) (\(tagInfo.count))",
                        isSelected: store.selectedTag == tagInfo.tag
                    ) {
                        store.selectedTag = store.selectedTag == tagInfo.tag ? nil : tagInfo.tag
                        store.showConflictsOnly = false
                    }
                }
            }
            .padding(.horizontal)
            .padding(.vertical, 8)
        }
    }
}

struct TagChip: View {
    let title: String
    let isSelected: Bool
    var tint: Color = .accentColor
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(.subheadline)
                .padding(.horizontal, 12)
                .padding(.vertical, 6)
                .background(isSelected ? tint : Color(.systemGray5), in: Capsule())
                .foregroundStyle(isSelected ? .white : .primary)
        }
        .buttonStyle(.plain)
    }
}
