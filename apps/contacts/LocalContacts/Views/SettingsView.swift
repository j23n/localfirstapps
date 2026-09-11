import SwiftUI
import Contacts

struct SettingsView: View {
    @Environment(ContactsStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL
    @State private var showFolderPicker = false
    @State private var showOverwriteConfirmation = false
    @State private var contactsAuthStatus: CNAuthorizationStatus = CNContactStore.authorizationStatus(for: .contacts)
    @AppStorage("hasSeenSyncInfo") private var hasSeenSyncInfo = false
    @State private var syncInfoExpanded = false

    private static let githubURL = URL(string: "https://github.com/j23n/localcontacts")!

    private var appVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "Unknown"
    }

    var body: some View {
        NavigationStack {
            List {
                Section("Contacts Folder") {
                    Button {
                        showFolderPicker = true
                    } label: {
                        LabeledContent {
                            Text(store.folderURL?.lastPathComponent ?? "Not selected")
                                .foregroundStyle(.secondary)
                        } label: {
                            Label("Folder", systemImage: "folder")
                        }
                    }
                    .tint(.primary)

                    Button {
                        Task { await store.loadContacts() }
                    } label: {
                        Label("Reload Contacts", systemImage: "arrow.clockwise")
                    }
                    .disabled(store.isLoading)

                    if let lastSync = store.lastSyncedAt {
                        LabeledContent("Last Synced", value: lastSync, format: .dateTime)
                    }
                }

                Section {
                    switch contactsAuthStatus {
                    case .authorized:
                        Label("Contacts access granted", systemImage: "checkmark.circle.fill")
                            .foregroundStyle(.green)

                        DisclosureGroup("About Contacts Sync", isExpanded: $syncInfoExpanded) {
                            VStack(alignment: .leading, spacing: 12) {
                                Text("Local .vcf files are the source of truth. Changes made in Apple Contacts are detected as conflicts for you to review.")
                                Text("When creating contacts in Apple Contacts, you must manually add them to the \"LocalContacts\" list at the bottom of the new contact creation or edit screen.")
                                Text("Photos may not round-trip perfectly due to re-encoding by Apple Contacts.")
                            }
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        }

                        Button("Force Overwrite LocalContacts List in Contacts") {
                            showOverwriteConfirmation = true
                        }

                    case .limited:
                        Label("Limited Contacts access is not enough", systemImage: "exclamationmark.triangle.fill")
                            .foregroundStyle(.orange)

                        Text("LocalContacts needs **full** Contacts access to maintain the LocalContacts group and caller ID. Limited access cannot sync.")
                            .font(.caption)
                            .foregroundStyle(.secondary)

                        Button("Open Settings") {
                            if let url = URL(string: UIApplication.openSettingsURLString) {
                                UIApplication.shared.open(url)
                            }
                        }

                    case .denied, .restricted:
                        Label("Contacts access denied", systemImage: "xmark.circle.fill")
                            .foregroundStyle(.red)

                        Text("LocalContacts works as a standalone vCard manager. To sync contacts with the system, grant access in Settings.")
                            .font(.caption)
                            .foregroundStyle(.secondary)

                        Button("Open Settings") {
                            if let url = URL(string: UIApplication.openSettingsURLString) {
                                UIApplication.shared.open(url)
                            }
                        }

                    case .notDetermined:
                        Button("Enable Contacts Sync") {
                            Task {
                                let granted = await store.syncService.requestAccess()
                                contactsAuthStatus = CNContactStore.authorizationStatus(for: .contacts)
                                if granted {
                                    do {
                                        try await store.syncService.fullReconciliation(contacts: store.contacts)
                                    } catch {
                                        store.errorMessage = error.localizedDescription
                                    }
                                }
                            }
                        }

                    @unknown default:
                        Text("Unknown authorization status")
                    }
                } header: {
                    Text("Contacts Sync")
                } footer: {
                    Text("Synced contacts appear under a \"LocalContacts\" group in the Apple Contacts app, enabling caller ID and QuickType suggestions.")
                }

                Section("Tags") {
                    NavigationLink {
                        TagManagementView()
                    } label: {
                        LabeledContent {
                            Text("\(store.allTags.count)")
                                .foregroundStyle(.secondary)
                        } label: {
                            Label("Manage Tags", systemImage: "tag")
                        }
                    }
                }

                statsSection

                diagnosticsSection

                Section("About") {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("LocalContacts manages your contacts as .vcf files in a folder of your choice — no import, no cloud account.")
                            .font(.callout)

                        Text("Found a bug or have feedback? Open an issue or get in touch:")
                            .font(.callout)
                            .foregroundStyle(.secondary)
                    }
                    .padding(.vertical, 4)

                    Button {
                        openURL(Self.githubURL)
                    } label: {
                        LabeledContent {
                            Image(systemName: "arrow.up.right")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        } label: {
                            Label("GitHub", systemImage: "chevron.left.forwardslash.chevron.right")
                        }
                    }
                    .tint(.primary)
                }
            }
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .onAppear {
                if !hasSeenSyncInfo {
                    syncInfoExpanded = true
                    hasSeenSyncInfo = true
                }
            }
            .alert("Error", isPresented: .init(
                get: { store.errorMessage != nil },
                set: { if !$0 { store.errorMessage = nil } }
            )) {
                Button("OK") { store.errorMessage = nil }
            } message: {
                Text(store.errorMessage ?? "")
            }
            .confirmationDialog("Force Overwrite LocalContacts List", isPresented: $showOverwriteConfirmation, titleVisibility: .visible) {
                Button("Overwrite", role: .destructive) {
                    Task {
                        do {
                            try await store.syncService.fullReconciliation(contacts: store.contacts)
                        } catch {
                            store.errorMessage = error.localizedDescription
                        }
                    }
                }
            } message: {
                if store.hasConflicts {
                    Text("This will delete all contacts in the LocalContacts list in Apple Contacts and replace them with the local .vcf versions. \(store.contacts.filter { $0.conflictState != nil }.count) unresolved conflict(s) will be lost.")
                } else {
                    Text("This will delete all contacts in the LocalContacts list in Apple Contacts and replace them with the local .vcf versions.")
                }
            }
            .sheet(isPresented: $showFolderPicker) {
                DocumentPickerView { url in
                    Task {
                        await store.setFolder(url)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var statsSection: some View {
        Section("Stats") {
            LabeledContent("Total Contacts", value: "\(store.contacts.count)")
            LabeledContent("Tags", value: "\(store.allTags.count)")

            let conflicts = store.contacts.filter { $0.conflictState != nil }.count
            if conflicts > 0 {
                LabeledContent("Conflicts") {
                    Text("\(conflicts)")
                        .foregroundStyle(.orange)
                }
            }

            let layoutColor: Color = store.layoutMode.isSupported ? .secondary : .orange
            LabeledContent("Storage Layout") {
                Text(store.layoutMode.label)
                    .foregroundStyle(layoutColor)
            }
            if !store.layoutMode.isSupported {
                Text(store.layoutMode.detail)
                    .font(.caption)
                    .foregroundStyle(layoutColor)
            }
        }
    }

    @ViewBuilder
    private var diagnosticsSection: some View {
        Section {
            NavigationLink {
                LogsView()
            } label: {
                Label("Logs", systemImage: "doc.text.magnifyingglass")
            }
            LabeledContent("Version", value: appVersion)
        } header: {
            Text("Diagnostics")
        }
    }
}

// MARK: - Tag Management

struct TagManagementView: View {
    @Environment(ContactsStore.self) private var store
    @State private var tagToRename: String?
    @State private var editedName = ""
    @State private var tagToDelete: String?

    var body: some View {
        List {
            if store.allTags.isEmpty {
                ContentUnavailableView("No Tags",
                    systemImage: "tag.slash",
                    description: Text("Tags are created when you assign them to contacts."))
            } else {
                ForEach(store.allTags, id: \.tag) { tagInfo in
                    Button {
                        tagToRename = tagInfo.tag
                        editedName = tagInfo.tag
                    } label: {
                        LabeledContent {
                            Text("\(tagInfo.count) contacts")
                                .foregroundStyle(.secondary)
                                .font(.subheadline)
                        } label: {
                            Text(tagInfo.tag)
                                .foregroundStyle(.primary)
                        }
                    }
                    .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                        Button(role: .destructive) {
                            tagToDelete = tagInfo.tag
                        } label: {
                            Label("Delete", systemImage: "trash")
                        }
                        Button {
                            tagToRename = tagInfo.tag
                            editedName = tagInfo.tag
                        } label: {
                            Label("Rename", systemImage: "pencil")
                        }
                        .tint(.orange)
                    }
                }
                .onDelete { indexSet in
                    if let index = indexSet.first {
                        tagToDelete = store.allTags[index].tag
                    }
                }
            }
        }
        .navigationTitle("Manage Tags")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            if !store.allTags.isEmpty {
                ToolbarItem(placement: .topBarTrailing) {
                    EditButton()
                }
            }
        }
        .alert("Rename Tag", isPresented: Binding(
            get: { tagToRename != nil },
            set: { if !$0 { tagToRename = nil } }
        )) {
            TextField("Tag name", text: $editedName)
            Button("Rename") {
                if let old = tagToRename {
                    commitRename(from: old)
                }
            }
            Button("Cancel", role: .cancel) { }
        } message: {
            if let tag = tagToRename {
                Text("Enter a new name for \"\(tag)\".")
            }
        }
        .confirmationDialog("Delete Tag", isPresented: Binding(
            get: { tagToDelete != nil },
            set: { if !$0 { tagToDelete = nil } }
        ), titleVisibility: .visible) {
            Button("Delete", role: .destructive) {
                if let tag = tagToDelete {
                    Task {
                        do {
                            try await store.deleteTag(tag)
                        } catch {
                            store.errorMessage = error.localizedDescription
                        }
                    }
                }
            }
        } message: {
            if let tag = tagToDelete {
                let count: Int = store.allTags.first(where: { $0.tag == tag })?.count ?? 0
                Text("This will remove \"\(tag)\" from \(count) contact(s).")
            }
        }
    }

    private func commitRename(from oldName: String) {
        let newName = editedName.trimmingCharacters(in: .whitespaces)
        guard !newName.isEmpty else { return }
        tagToRename = nil
        Task {
            do {
                try await store.renameTag(oldName, to: newName)
            } catch {
                store.errorMessage = error.localizedDescription
            }
        }
    }
}
