import SwiftUI

struct ConflictResolutionSheet: View {
    @Environment(ContactsStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    let contact: Contact

    var body: some View {
        NavigationStack {
            VStack(spacing: 24) {
                Image(systemName: conflictIcon)
                    .font(.system(size: 48))
                    .foregroundStyle(.orange)

                Text(conflictTitle)
                    .font(.title3.bold())
                    .multilineTextAlignment(.center)

                Text(conflictMessage)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 24)

                VStack(spacing: 12) {
                    Button {
                        keepLocalVersion()
                    } label: {
                        Label("Keep LocalContacts Version", systemImage: "doc.badge.arrow.up")
                            .frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.borderedProminent)

                    if contact.conflictState == .externalEdit {
                        Button {
                            importExternalChanges()
                        } label: {
                            Label("Import External Changes", systemImage: "arrow.down.doc")
                                .frame(maxWidth: .infinity)
                        }
                        .buttonStyle(.bordered)
                    }
                }
                .controlSize(.large)
                .padding(.horizontal, 24)

                Spacer()
            }
            .padding(.top, 32)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Dismiss") { dismiss() }
                }
            }
        }
        .presentationDetents([.medium])
    }

    private var conflictIcon: String {
        switch contact.conflictState {
        case .externalEdit: "pencil.circle.fill"
        case .externalDelete: "trash.circle.fill"
        case nil: "questionmark.circle"
        }
    }

    private var conflictTitle: String {
        switch contact.conflictState {
        case .externalEdit: "External Edit Detected"
        case .externalDelete: "External Deletion Detected"
        case nil: "No Conflict"
        }
    }

    private var conflictMessage: String {
        switch contact.conflictState {
        case .externalEdit:
            "\(contact.displayName) was edited outside LocalContacts. Choose which version to keep."
        case .externalDelete:
            "\(contact.displayName) was deleted outside LocalContacts. You can re-push the local version or accept the deletion."
        case nil:
            ""
        }
    }

    private func keepLocalVersion() {
        contact.conflictState = nil
        Task {
            try? await store.syncService.pushContact(contact)
            dismiss()
        }
    }

    private func importExternalChanges() {
        Task {
            if let data = await store.syncService.fetchCNContactData(localContactsID: contact.localContactsID) {
                try? await store.applyExternalData(data, to: contact)
                // Re-push to CN so both sides are consistent
                try? await store.syncService.pushContact(contact)
            } else {
                // Couldn't fetch — just clear the flag
                contact.conflictState = nil
            }
            dismiss()
        }
    }
}
