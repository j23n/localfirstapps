import SwiftUI
import Contacts
import ShellKitSwift

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
        ShellSettings(
            title: "Settings",
            tokens: ShellTokens(cardRadius: ContactsTokens.cardRadius),
            dismissLabel: "Done",
            onDismiss: { dismiss() }
        ) {
                Section("Contacts Folder") {
                    Button {
                        showFolderPicker = true
                    } label: {
                        ShellTextRow(
                            .init(
                                title: "Folder",
                                trailingValue: store.folderURL?.lastPathComponent
                                    ?? "Not selected",
                                leadingSymbol: "folder"
                            )
                        )
                    }
                    .tint(.primary)

                    ShellActionRow(
                        .init(
                            actionID: "reload-contacts",
                            label: "Reload Contacts",
                            isEnabled: !store.isLoading,
                            leadingSymbol: "arrow.clockwise"
                        )
                    ) { _ in
                        Task { await store.loadContacts() }
                    }

                    if let lastSync = store.lastSyncedAt {
                        LabeledContent("Last Synced", value: lastSync, format: .dateTime)
                    }
                }

                Section {
                    switch contactsAuthStatus {
                    case .authorized:
                        ShellStatusRow(
                            .init(
                                message: "Contacts access granted",
                                severity: .info
                            )
                        )

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
                        ShellStatusRow(
                            .init(
                                message: "Limited Contacts access is not enough",
                                severity: .warning
                            )
                        )

                        Text("LocalContacts needs **full** Contacts access to maintain the LocalContacts group and caller ID. Limited access cannot sync.")
                            .font(.caption)
                            .foregroundStyle(.secondary)

                        Button("Open Settings") {
                            if let url = URL(string: UIApplication.openSettingsURLString) {
                                UIApplication.shared.open(url)
                            }
                        }

                    case .denied, .restricted:
                        ShellStatusRow(
                            .init(
                                message: "Contacts access denied",
                                severity: .error
                            )
                        )

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
                    ShellNavRow(
                        .init(
                            destinationID: "tag-management",
                            label: "Manage Tags",
                            trailingValue: "\(store.allTags.count)",
                            leadingSymbol: "tag"
                        )
                    ) {
                        ContactsRouter.destination(TagManagementView())
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
            .shellConfirmation(
                data: .init(
                    actionID: "overwrite-system-contacts",
                    question: "Force Overwrite LocalContacts List",
                    destructiveLabel: "Overwrite",
                    message: store.hasConflicts
                        ? "This will delete all contacts in the LocalContacts list in Apple Contacts and replace them with the local .vcf versions. \(store.contacts.filter { $0.conflictState != nil }.count) unresolved conflict(s) will be lost."
                        : "This will delete all contacts in the LocalContacts list in Apple Contacts and replace them with the local .vcf versions."
                ),
                isPresented: $showOverwriteConfirmation
            ) { _ in
                Task {
                    do {
                        try await store.syncService.fullReconciliation(contacts: store.contacts)
                    } catch {
                        store.errorMessage = error.localizedDescription
                    }
                }
            }
            .sheet(isPresented: $showFolderPicker) {
                ContactsRouter.destination(
                    DocumentPickerView { url in
                        Task {
                            await store.setFolder(url)
                        }
                    }
                )
            }
    }

    @ViewBuilder
    private var statsSection: some View {
        Section("Stats") {
            ShellTextRow(
                .init(
                    title: "Total Contacts",
                    trailingValue: "\(store.contacts.count)"
                )
            )
            ShellTextRow(
                .init(title: "Tags", trailingValue: "\(store.allTags.count)")
            )

            let conflicts = store.contacts.filter { $0.conflictState != nil }.count
            if conflicts > 0 {
                ShellTextRow(
                    .init(title: "Conflicts", trailingValue: "\(conflicts)")
                )
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
            ShellNavRow(
                .init(
                    destinationID: "logs",
                    label: "Logs",
                    leadingSymbol: "doc.text.magnifyingglass"
                )
            ) {
                ContactsRouter.destination(LogsView())
            }
            ShellTextRow(
                .init(title: "Version", trailingValue: appVersion)
            )
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
