import SwiftUI
import Contacts

struct SettingsView: View {
    @Environment(ContactsStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var showFolderPicker = false
    @State private var showOverwriteConfirmation = false
    @State private var contactsAuthStatus: CNAuthorizationStatus = CNContactStore.authorizationStatus(for: .contacts)
    @AppStorage("hasSeenSyncInfo") private var hasSeenSyncInfo = false
    @State private var syncInfoExpanded = false

    var body: some View {
        NavigationStack {
            List {
                // Folder
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

                    Button("Reload Contacts") {
                        Task { await store.loadContacts() }
                    }

                    if let lastSync = store.lastSyncedAt {
                        LabeledContent("Last Synced") {
                            Text(lastSync, style: .relative)
                                .foregroundStyle(.secondary)
                        }
                    }
                }

                // Storage Layout
                Section {
                    LabeledContent("Layout") {
                        Text(store.layoutMode.label)
                            .foregroundStyle(store.layoutMode.isSupported ? .secondary : .orange)
                    }
                    Text(store.layoutMode.detail)
                        .font(.caption)
                        .foregroundStyle(store.layoutMode.isSupported ? .secondary : .orange)
                } header: {
                    Text("Storage Layout")
                }

                // Contacts Sync
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
                                    try? await store.syncService.fullReconciliation(contacts: store.contacts)
                                }
                            }
                        }

                    @unknown default:
                        Text("Unknown authorization status")
                    }
                } header: {
                    Text("Contacts Integration")
                } footer: {
                    Text("Synced contacts appear under a \"LocalContacts\" group in the Apple Contacts app, enabling caller ID and QuickType suggestions.")
                }

                // Tag Management
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

                // Stats
                Section("Info") {
                    LabeledContent("Total Contacts", value: "\(store.contacts.count)")
                    LabeledContent("Tags", value: "\(store.allTags.count)")

                    let conflicts = store.contacts.filter { $0.conflictState != nil }.count
                    if conflicts > 0 {
                        LabeledContent("Conflicts") {
                            Text("\(conflicts)")
                                .foregroundStyle(.orange)
                        }
                    }
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
            .confirmationDialog("Force Overwrite LocalContacts List", isPresented: $showOverwriteConfirmation, titleVisibility: .visible) {
                Button("Overwrite", role: .destructive) {
                    Task {
                        try? await store.syncService.fullReconciliation(contacts: store.contacts)
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
}

// MARK: - Tag Management

struct TagManagementView: View {
    @Environment(ContactsStore.self) private var store
    @State private var editingTag: String?
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
                    HStack {
                        if editingTag == tagInfo.tag {
                            TextField("Tag name", text: $editedName)
                                .onSubmit { commitRename(from: tagInfo.tag) }
                            Button("Save") { commitRename(from: tagInfo.tag) }
                                .buttonStyle(.borderedProminent)
                                .controlSize(.small)
                            Button("Cancel") { editingTag = nil }
                                .controlSize(.small)
                        } else {
                            Text(tagInfo.tag)
                            Spacer()
                            Text("\(tagInfo.count) contacts")
                                .foregroundStyle(.secondary)
                                .font(.subheadline)
                        }
                    }
                    .swipeActions(edge: .trailing) {
                        Button(role: .destructive) {
                            tagToDelete = tagInfo.tag
                        } label: {
                            Label("Delete", systemImage: "trash")
                        }
                        Button {
                            editingTag = tagInfo.tag
                            editedName = tagInfo.tag
                        } label: {
                            Label("Rename", systemImage: "pencil")
                        }
                        .tint(.orange)
                    }
                }
            }
        }
        .navigationTitle("Manage Tags")
        .navigationBarTitleDisplayMode(.inline)
        .confirmationDialog("Delete Tag", isPresented: .init(
            get: { tagToDelete != nil },
            set: { if !$0 { tagToDelete = nil } }
        ), titleVisibility: .visible) {
            Button("Delete", role: .destructive) {
                if let tag = tagToDelete {
                    Task { try? await store.deleteTag(tag) }
                }
            }
        } message: {
            if let tag = tagToDelete {
                let count = store.allTags.first(where: { $0.tag == tag })?.count ?? 0
                Text("This will remove \"\(tag)\" from \(count) contact(s).")
            }
        }
    }

    private func commitRename(from oldName: String) {
        let newName = editedName.trimmingCharacters(in: .whitespaces)
        guard !newName.isEmpty else { return }
        editingTag = nil
        Task { try? await store.renameTag(oldName, to: newName) }
    }
}
