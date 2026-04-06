import SwiftUI
import Contacts

struct SettingsView: View {
    @Environment(ContactsStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var showFolderPicker = false
    @State private var showOverwriteConfirmation = false
    @State private var contactsAuthStatus: CNAuthorizationStatus = CNContactStore.authorizationStatus(for: .contacts)

    var body: some View {
        NavigationStack {
            List {
                // Folder
                Section("Contacts Folder") {
                    if let url = store.folderURL {
                        LabeledContent("Current Folder") {
                            Text(url.lastPathComponent)
                                .foregroundStyle(.secondary)
                        }
                    }

                    Button("Change Folder") {
                        showFolderPicker = true
                    }

                    Button("Reload Contacts") {
                        Task { await store.loadContacts() }
                    }
                }

                // Contacts Sync
                Section {
                    switch contactsAuthStatus {
                    case .authorized:
                        Label("Contacts access granted", systemImage: "checkmark.circle.fill")
                            .foregroundStyle(.green)

                        Button("Force Overwrite Contacts App") {
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
            .confirmationDialog("Force Overwrite", isPresented: $showOverwriteConfirmation, titleVisibility: .visible) {
                Button("Overwrite", role: .destructive) {
                    Task {
                        try? await store.syncService.fullReconciliation(contacts: store.contacts)
                    }
                }
            } message: {
                if store.hasConflicts {
                    Text("This will delete all contacts in the LocalContacts group and replace them with the local .vcf versions. \(store.contacts.filter { $0.conflictState != nil }.count) unresolved conflict(s) will be lost.")
                } else {
                    Text("This will delete all contacts in the LocalContacts group and replace them with the local .vcf versions.")
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
