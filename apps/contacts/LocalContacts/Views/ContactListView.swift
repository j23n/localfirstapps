import SwiftUI

struct ContactListView: View {
    @Environment(ContactsStore.self) private var store
    @State private var showSettings = false
    @State private var showAddContact = false
    @State private var isSelecting = false
    @State private var selectedContactIDs: Set<String> = []
    @State private var showBulkTagPicker = false
    @State private var showBulkDeleteConfirmation = false

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
            .searchable(text: $store.searchText, prompt: "Name, company, phone, or email")
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    HStack {
                        Button {
                            showSettings = true
                        } label: {
                            Image(systemName: "gear")
                        }
                        if !store.contacts.isEmpty {
                            Button(isSelecting ? "Done" : "Select") {
                                withAnimation {
                                    isSelecting.toggle()
                                    if !isSelecting {
                                        selectedContactIDs.removeAll()
                                    }
                                }
                            }
                        }
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    if !isSelecting {
                        Button {
                            showAddContact = true
                        } label: {
                            Image(systemName: "plus")
                        }
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
            .onChange(of: showAddContact) { _, isAdding in
                store.isSuppressingReload = isAdding
            }
            .refreshable {
                await store.loadContacts()
            }
            .toolbarBackground(.ultraThinMaterial, for: .navigationBar)
            .toolbarBackground(.visible, for: .navigationBar)
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

            List(selection: isSelecting ? $selectedContactIDs : nil) {
                ForEach(store.groupedContacts, id: \.letter) { group in
                    Section(group.letter) {
                        ForEach(group.contacts) { contact in
                            if isSelecting {
                                ContactCard(contact: contact)
                                    .tag(contact.localContactsID)
                            } else {
                                NavigationLink(value: contact.localContactsID) {
                                    ContactCard(contact: contact)
                                }
                            }
                        }
                    }
                }
            }
            .listStyle(.insetGrouped)
            .environment(\.editMode, isSelecting ? .constant(.active) : .constant(.inactive))
            .navigationDestination(for: String.self) { contactID in
                if let contact = store.contacts.first(where: { $0.localContactsID == contactID }) {
                    ContactDetailView(contact: contact)
                } else {
                    ContentUnavailableView("Contact Not Found",
                        systemImage: "person.crop.circle.badge.xmark",
                        description: Text("This contact may have been deleted."))
                }
            }
            .overlay {
                if store.filteredContacts.isEmpty && !store.searchText.isEmpty {
                    ContentUnavailableView.search(text: store.searchText)
                }
            }

            if isSelecting && !selectedContactIDs.isEmpty {
                bulkActionBar
            }
        }
        .sheet(isPresented: $showBulkTagPicker) {
            BulkTagPickerView(selectedContactIDs: selectedContactIDs) {
                withAnimation {
                    isSelecting = false
                    selectedContactIDs.removeAll()
                }
            }
        }
        .confirmationDialog(
            "Delete \(selectedContactIDs.count) Contacts",
            isPresented: $showBulkDeleteConfirmation,
            titleVisibility: .visible
        ) {
            Button("Delete \(selectedContactIDs.count) Contacts", role: .destructive) {
                Task {
                    try? await store.deleteMultiple(selectedContactIDs)
                    withAnimation {
                        isSelecting = false
                        selectedContactIDs.removeAll()
                    }
                }
            }
        } message: {
            Text("This will permanently delete the selected contacts and their .vcf files.")
        }
    }

    private var bulkActionBar: some View {
        VStack(spacing: 0) {
            Divider()
            HStack(spacing: 24) {
                Button {
                    showBulkTagPicker = true
                } label: {
                    Label("Tag", systemImage: "tag")
                }

                Spacer()

                Text("\(selectedContactIDs.count) selected")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)

                Spacer()

                Button(role: .destructive) {
                    showBulkDeleteConfirmation = true
                } label: {
                    Label("Delete", systemImage: "trash")
                }
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
            .background(.bar)
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
        .background(.ultraThinMaterial)
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

// MARK: - Bulk Tag Picker

struct BulkTagPickerView: View {
    @Environment(ContactsStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    let selectedContactIDs: Set<String>
    let onComplete: () -> Void
    @State private var newTagName = ""

    var body: some View {
        NavigationStack {
            List {
                if !store.allTags.isEmpty {
                    Section("Existing Tags") {
                        ForEach(store.allTags, id: \.tag) { tagInfo in
                            Button {
                                applyTag(tagInfo.tag)
                            } label: {
                                HStack {
                                    Text(tagInfo.tag)
                                        .foregroundStyle(.primary)
                                    Spacer()
                                    Text("\(tagInfo.count)")
                                        .foregroundStyle(.secondary)
                                }
                            }
                        }
                    }
                }

                Section("New Tag") {
                    HStack {
                        TextField("Tag name", text: $newTagName)
                        Button("Apply") {
                            applyTag(newTagName.trimmingCharacters(in: .whitespaces))
                        }
                        .disabled(newTagName.trimmingCharacters(in: .whitespaces).isEmpty)
                    }
                }
            }
            .navigationTitle("Assign Tag")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
        .presentationDetents([.medium, .large])
    }

    private func applyTag(_ tag: String) {
        guard !tag.isEmpty else { return }
        Task {
            try? await store.assignTag(tag, to: selectedContactIDs)
            dismiss()
            onComplete()
        }
    }
}
