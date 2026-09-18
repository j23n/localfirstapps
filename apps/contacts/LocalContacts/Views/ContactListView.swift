import SwiftUI
import ShellKitSwift

struct ContactListView: View {
    @Environment(ContactsStore.self) private var store
    @State private var showSettings = false
    @State private var showAddContact = false
    @State private var isSelecting = false
    @State private var selectedContactIDs: Set<String> = []
    @State private var showBulkTagPicker = false
    @State private var showBulkDeleteConfirmation = false
    @State private var showSyncConflicts = false

    var body: some View {
        @Bindable var store = store

        NavigationStack {
            Group {
                if store.isLoading {
                    VStack(spacing: 12) {
                        ProgressView()
                        Text("Loading contacts…")
                            .font(.callout)
                            .foregroundStyle(.secondary)
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if store.contacts.isEmpty {
                    emptyState
                } else {
                    contactList
                }
            }
            .navigationTitle("Contacts")
            .shellSearch(
                text: $store.searchText,
                data: .init(prompt: "Name, company, phone, or email")
            )
            .overlay(alignment: .top) {
                LinearGradient(
                    stops: [
                        .init(color: Color(.systemGroupedBackground), location: 0),
                        .init(color: Color(.systemGroupedBackground).opacity(0.7), location: 0.5),
                        .init(color: Color(.systemGroupedBackground).opacity(0), location: 1.0)
                    ],
                    startPoint: .top,
                    endPoint: .bottom
                )
                .frame(height: 130)
                .ignoresSafeArea(edges: .top)
                .allowsHitTesting(false)
            }
            .toolbar {
                ToolbarItem(placement: .principal) {
                    if let progress = store.chromeProgress {
                        ShellProgressChip(progress)
                    }
                }
                ToolbarItem(placement: .topBarLeading) {
                    HStack {
                        SettingsToolbarButton(isPresented: $showSettings)
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
                        .accessibilityLabel("Add Contact")
                    }
                }
            }
            .sheet(isPresented: $showSettings) {
                ContactsRouter.destination(SettingsView())
            }
            .sheet(isPresented: $showAddContact) {
                NavigationStack {
                    ContactsRouter.destination(
                        ContactEditView(draft: store.newContactDraft(), isNew: true)
                    )
                }
            }
            .sheet(isPresented: $showSyncConflicts) {
                ContactsRouter.destination(SyncConflictGroupSheet())
            }
            .safeAreaInset(edge: .top) {
                if store.hasSyncConflictGroups {
                    Button {
                        showSyncConflicts = true
                    } label: {
                        Label(
                            "\(store.syncConflictGroups.count) Sync Conflicts",
                            systemImage: "exclamationmark.triangle.fill"
                        )
                        .frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.borderedProminent)
                    .padding(.horizontal)
                    .padding(.bottom, 8)
                    .accessibilityIdentifier(ContactsScreen.syncConflictGroup.rawValue)
                }
            }
            .onChange(of: showAddContact) { _, isAdding in
                store.isSuppressingReload = isAdding
            }
            .refreshable {
                await store.loadContacts()
            }
            .toolbarBackgroundVisibility(.hidden, for: .navigationBar)
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

    private var isSearching: Bool {
        !store.searchText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
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
        let hits = store.searchHits
        let rowsByID = Dictionary(uniqueKeysWithValues: store.listRows.map { ($0.id, $0) })
        return VStack(spacing: 0) {
            List(selection: isSelecting ? $selectedContactIDs : nil) {
                if !store.allTags.isEmpty || store.hasConflicts {
                    TagFilterBar()
                        .listRowInsets(EdgeInsets())
                        .listRowBackground(Color.clear)
                        .listRowSeparator(.hidden)
                }

                if isSearching {
                    ForEach(hits, id: \.id) { hit in
                        if let contact = store.contacts.first(where: {
                            $0.localContactsID == hit.id
                        }) {
                            if isSelecting {
                                ContactCard(contact: contact, hit: hit, query: store.searchText)
                                    .tag(contact.localContactsID)
                            } else {
                                NavigationLink(value: contact.localContactsID) {
                                    ContactCard(contact: contact, hit: hit, query: store.searchText)
                                }
                            }
                        }
                    }
                } else {
                    ForEach(store.groupedContacts, id: \.letter) { group in
                        Section(group.letter) {
                            ForEach(group.contacts) { contact in
                                let row = rowsByID[contact.localContactsID]
                                if isSelecting {
                                    ContactCard(
                                        contact: contact,
                                        title: row?.title ?? contact.displayName,
                                        subtitle: row?.subtitle
                                    )
                                    .tag(contact.localContactsID)
                                } else {
                                    NavigationLink(value: contact.localContactsID) {
                                        ContactCard(
                                            contact: contact,
                                            title: row?.title ?? contact.displayName,
                                            subtitle: row?.subtitle
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }
            .listStyle(.insetGrouped)
            .scrollContentBackground(.hidden)
            .background(Color(.systemGroupedBackground))
            .environment(\.editMode, isSelecting ? .constant(.active) : .constant(.inactive))
            .navigationDestination(for: String.self) { contactID in
                if let contact = store.contacts.first(where: { $0.localContactsID == contactID }) {
                    ContactsRouter.destination(ContactDetailView(contact: contact))
                } else {
                    ContentUnavailableView("Contact Not Found",
                        systemImage: "person.crop.circle.badge.xmark",
                        description: Text("This contact may have been deleted."))
                }
            }
            .overlay {
                if isSearching && hits.isEmpty {
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
                    do {
                        try await store.deleteMultiple(selectedContactIDs)
                        withAnimation {
                            isSelecting = false
                            selectedContactIDs.removeAll()
                        }
                    } catch {
                        store.errorMessage = error.localizedDescription
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
    var title: String
    var subtitle: String? = nil
    var hit: SearchHit? = nil
    var query: String = ""

    private static let avatarSize: CGFloat = 36
    private static let avatarSpacing: CGFloat = 12

    init(
        contact: Contact,
        title: String? = nil,
        subtitle: String? = nil,
        hit: SearchHit? = nil,
        query: String = ""
    ) {
        self.contact = contact
        self.title = hit?.title ?? title ?? contact.displayName
        if let hit {
            if let kind = hit.trailing, !kind.isEmpty {
                self.subtitle = "\(kind): \(hit.subtitle)"
            } else {
                self.subtitle = hit.subtitle
            }
        } else {
            self.subtitle = subtitle
        }
        self.hit = hit
        self.query = query
    }

    var body: some View {
        HStack(spacing: Self.avatarSpacing) {
            if let hit {
                Image(systemName: Self.systemImage(for: hit.symbol))
                    .font(.body)
                    .foregroundStyle(.secondary)
                    .frame(width: Self.avatarSize, height: Self.avatarSize)
            } else {
                AvatarView(contact: contact, size: Self.avatarSize)
            }

            VStack(alignment: .leading, spacing: 1) {
                Text(title)
                    .font(.body.weight(.medium))

                if let hit {
                    subtitleText(for: hit)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                } else if let subtitle, !subtitle.isEmpty {
                    Text(subtitle)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
            .alignmentGuide(.listRowSeparatorLeading) { d in d[.leading] }

            Spacer()

            if contact.conflictState != nil {
                Image(systemName: "exclamationmark.circle.fill")
                    .foregroundStyle(.orange)
                    .font(.subheadline)
            }
        }
    }

    @ViewBuilder
    private func subtitleText(for hit: SearchHit) -> some View {
        let highlighted = Self.highlighted(hit.subtitle, query: query)
        if let kind = hit.trailing, !kind.isEmpty {
            Text("\(kind): ") + Text(highlighted)
        } else {
            Text(highlighted)
        }
    }

    /// Mark the first case-insensitive occurrence of `query` in `value`.
    nonisolated static func highlighted(_ value: String, query: String) -> AttributedString {
        var text = AttributedString(value)
        let needle = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !needle.isEmpty,
              let stringRange = value.range(of: needle, options: .caseInsensitive),
              let attrRange = Range(NSRange(stringRange, in: value), in: text) else {
            return text
        }
        text[attrRange].font = .subheadline.weight(.semibold)
        text[attrRange].backgroundColor = Color.accentColor.opacity(0.18)
        return text
    }

    static func systemImage(for symbol: String) -> String {
        switch symbol {
        case "phone-symbolic": "phone"
        case "mail-unread-symbolic": "envelope"
        case "mark-location-symbolic": "mappin"
        case "x-office-calendar-symbolic": "calendar"
        case "system-users-symbolic": "building.2"
        case "document-properties-symbolic": "briefcase"
        case "web-browser-symbolic": "globe"
        case "text-x-generic-symbolic": "note.text"
        case "user-bookmarks-symbolic": "tag"
        default: "person"
        }
    }
}

// MARK: - Avatar

struct AvatarView: View {
    let photoData: Data?
    let initials: String
    let size: CGFloat

    init(contact: Contact, size: CGFloat) {
        self.init(photoData: contact.photoData, initials: contact.initials, size: size)
    }

    init(photoData: Data?, initials: String, size: CGFloat) {
        self.photoData = photoData
        self.initials = initials
        self.size = size
    }

    var body: some View {
        if let photoData, let uiImage = UIImage(data: photoData) {
            Image(uiImage: uiImage)
                .resizable()
                .scaledToFill()
                .frame(width: size, height: size)
                .clipShape(Circle())
        } else {
            Text(initials)
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
        .background(Color.clear)
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
            do {
                try await store.assignTag(tag, to: selectedContactIDs)
                dismiss()
                onComplete()
            } catch {
                store.errorMessage = error.localizedDescription
            }
        }
    }
}
