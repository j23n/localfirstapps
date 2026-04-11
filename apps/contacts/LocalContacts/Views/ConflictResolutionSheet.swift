import SwiftUI

struct ConflictResolutionSheet: View {
    @Environment(ContactsStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    let contact: Contact

    var body: some View {
        NavigationStack {
            if let data = contact.conflictState?.externalData {
                ConflictDiffView(contact: contact, externalData: data, store: store, dismiss: dismiss)
            } else {
                // External delete — simple UI
                deletionView
            }
        }
    }

    private var deletionView: some View {
        VStack(spacing: 24) {
            Image(systemName: "trash.circle.fill")
                .font(.system(size: 48))
                .foregroundStyle(.orange)

            Text("External Deletion Detected")
                .font(.title3.bold())
                .multilineTextAlignment(.center)

            Text("\(contact.displayName) was deleted outside LocalContacts. You can re-push the local version or accept the deletion.")
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 24)

            VStack(spacing: 12) {
                Button {
                    contact.conflictState = nil
                    Task {
                        try? await store.syncService.pushContact(contact)
                        dismiss()
                    }
                } label: {
                    Label("Re-Push to Contacts", systemImage: "arrow.up.doc")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)

                Button(role: .destructive) {
                    Task {
                        try? await store.delete(contact)
                        dismiss()
                    }
                } label: {
                    Label("Accept Deletion", systemImage: "trash")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered)
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
}

// MARK: - Diff View

private struct ConflictDiffView: View {
    let contact: Contact
    let externalData: CNSyncService.CNContactData
    let store: ContactsStore
    let dismiss: DismissAction

    @State private var selections: [String: FieldSource] = [:]

    enum FieldSource {
        case local, apple
    }

    var body: some View {
        List {
            Section {
                Text("\(contact.displayName) was edited in Apple Contacts. Choose which version to keep for each field.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .listRowBackground(Color.clear)
            }

            ForEach(diffs) { diff in
                Section(diff.label) {
                    FieldRow(
                        key: diff.key,
                        localValue: diff.localValue,
                        appleValue: diff.appleValue,
                        selection: selections[diff.key],
                        onSelect: { source in
                            selections[diff.key] = source
                        }
                    )
                }
            }

            if !identicalFields.isEmpty {
                Section {
                    DisclosureGroup("Identical Fields") {
                        ForEach(identicalFields, id: \.self) { name in
                            Text(name)
                                .foregroundStyle(.secondary)
                        }
                    }
                    .foregroundStyle(.secondary)
                }
            }
        }
        .navigationTitle("Resolve Conflict")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button("Dismiss") { dismiss() }
            }
        }
        .safeAreaInset(edge: .bottom) {
            bottomBar
        }
        .onAppear {
            // Default all selections to local
            for diff in diffs {
                selections[diff.key] = .local
            }
        }
    }

    private var bottomBar: some View {
        VStack(spacing: 8) {
            HStack(spacing: 12) {
                Button("Keep All Local") {
                    for diff in diffs { selections[diff.key] = .local }
                }
                .buttonStyle(.bordered)
                .tint(.accentColor)

                Button("Keep All Apple") {
                    for diff in diffs { selections[diff.key] = .apple }
                }
                .buttonStyle(.bordered)
                .tint(.orange)
            }

            Button {
                applyMerge()
            } label: {
                Text("Apply")
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
        }
        .padding()
        .background(.ultraThinMaterial)
    }

    // MARK: - Diff Computation

    struct FieldDiff: Identifiable {
        let key: String
        let label: String
        let localValue: String
        let appleValue: String
        var id: String { key }
    }

    private var diffs: [FieldDiff] {
        var result: [FieldDiff] = []

        func addIfDifferent(_ key: String, _ label: String, _ local: String, _ apple: String) {
            let l = local.trimmingCharacters(in: .whitespacesAndNewlines)
            let a = apple.trimmingCharacters(in: .whitespacesAndNewlines)
            if l != a {
                result.append(FieldDiff(key: key, label: label, localValue: l.isEmpty ? "(empty)" : l, appleValue: a.isEmpty ? "(empty)" : a))
            }
        }

        // Name fields
        let localFullName = [contact.namePrefix, contact.givenName, contact.middleName, contact.familyName, contact.nameSuffix]
            .filter { !$0.isEmpty }.joined(separator: " ")
        let appleFullName = [externalData.namePrefix, externalData.givenName, externalData.middleName, externalData.familyName, externalData.nameSuffix]
            .filter { !$0.isEmpty }.joined(separator: " ")
        addIfDifferent("name", "Name", localFullName, appleFullName)

        addIfDifferent("organization", "Organization", contact.organization, externalData.organization)
        addIfDifferent("jobTitle", "Job Title", contact.jobTitle, externalData.jobTitle)
        addIfDifferent("nickname", "Nickname", contact.nickname, externalData.nickname)

        // Phone numbers
        let localPhones = contact.phoneNumbers.map { "\($0.label): \($0.value)" }.sorted().joined(separator: "\n")
        let applePhones = externalData.phoneNumbers.map { "\($0.label): \($0.value)" }.sorted().joined(separator: "\n")
        addIfDifferent("phones", "Phone Numbers", localPhones, applePhones)

        // Emails
        let localEmails = contact.emailAddresses.map { "\($0.label): \($0.value)" }.sorted().joined(separator: "\n")
        let appleEmails = externalData.emailAddresses.map { "\($0.label): \($0.value)" }.sorted().joined(separator: "\n")
        addIfDifferent("emails", "Email Addresses", localEmails, appleEmails)

        // URLs
        let localURLs = contact.urls.map { "\($0.label): \($0.value)" }.sorted().joined(separator: "\n")
        let appleURLs = externalData.urls.map { "\($0.label): \($0.value)" }.sorted().joined(separator: "\n")
        addIfDifferent("urls", "URLs", localURLs, appleURLs)

        // Postal addresses
        let localAddrs = contact.postalAddresses.map { "\($0.label): \($0.value.formatted)" }.sorted().joined(separator: "\n\n")
        let appleAddrs = externalData.postalAddresses.map { addr in
            let formatted = [addr.street, addr.city, [addr.state, addr.postalCode].filter { !$0.isEmpty }.joined(separator: " "), addr.country]
                .filter { !$0.isEmpty }.joined(separator: "\n")
            return "\(addr.label): \(formatted)"
        }.sorted().joined(separator: "\n\n")
        addIfDifferent("addresses", "Addresses", localAddrs, appleAddrs)

        // Birthday
        let localBday = contact.birthday.map { formatBirthday($0) } ?? ""
        let appleBday = externalData.birthday.map { formatBirthday($0) } ?? ""
        addIfDifferent("birthday", "Birthday", localBday, appleBday)

        return result
    }

    private var identicalFields: [String] {
        let allFieldKeys: [(String, String)] = [
            ("name", "Name"), ("organization", "Organization"), ("jobTitle", "Job Title"),
            ("nickname", "Nickname"), ("phones", "Phone Numbers"), ("emails", "Email Addresses"),
            ("urls", "URLs"), ("addresses", "Addresses"), ("birthday", "Birthday"),
        ]
        let diffKeys = Set(diffs.map(\.key))
        return allFieldKeys.filter { !diffKeys.contains($0.0) }.map(\.1)
    }

    private func formatBirthday(_ dc: DateComponents) -> String {
        var parts: [String] = []
        if let year = dc.year { parts.append("\(year)") }
        if let month = dc.month { parts.append(String(format: "%02d", month)) }
        if let day = dc.day { parts.append(String(format: "%02d", day)) }
        return parts.joined(separator: "-")
    }

    // MARK: - Apply Merge

    private func applyMerge() {
        for diff in diffs {
            let useApple = selections[diff.key] == .apple
            switch diff.key {
            case "name":
                if useApple {
                    contact.givenName = externalData.givenName
                    contact.familyName = externalData.familyName
                    contact.middleName = externalData.middleName
                    contact.namePrefix = externalData.namePrefix
                    contact.nameSuffix = externalData.nameSuffix
                    contact.fullName = [externalData.givenName, externalData.middleName, externalData.familyName]
                        .filter { !$0.isEmpty }.joined(separator: " ")
                }
            case "organization":
                if useApple { contact.organization = externalData.organization }
            case "jobTitle":
                if useApple { contact.jobTitle = externalData.jobTitle }
            case "nickname":
                if useApple { contact.nickname = externalData.nickname }
            case "phones":
                if useApple {
                    contact.phoneNumbers = externalData.phoneNumbers.map {
                        LabeledValue(label: $0.label, value: $0.value)
                    }
                }
            case "emails":
                if useApple {
                    contact.emailAddresses = externalData.emailAddresses.map {
                        LabeledValue(label: $0.label, value: $0.value)
                    }
                }
            case "urls":
                if useApple {
                    contact.urls = externalData.urls.map {
                        LabeledValue(label: $0.label, value: $0.value)
                    }
                }
            case "addresses":
                if useApple {
                    contact.postalAddresses = externalData.postalAddresses.map {
                        LabeledValue(label: $0.label, value: PostalAddress(
                            street: $0.street, city: $0.city, state: $0.state,
                            postalCode: $0.postalCode, country: $0.country
                        ))
                    }
                }
            case "birthday":
                if useApple { contact.birthday = externalData.birthday }
            default:
                break
            }
        }

        // Import photo from Apple if available (photos aren't diffed)
        // Keep local photo unless Apple version exists and local is nil
        if contact.photoData == nil, let applePhoto = externalData.imageData {
            contact.photoData = applePhoto
        }

        contact.conflictState = nil

        Task {
            try? await store.save(contact)
            try? await store.syncService.pushContact(contact)
            dismiss()
        }
    }
}

// MARK: - Field Row

private struct FieldRow: View {
    let key: String
    let localValue: String
    let appleValue: String
    let selection: ConflictDiffView.FieldSource?
    let onSelect: (ConflictDiffView.FieldSource) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Button { onSelect(.local) } label: {
                HStack(alignment: .top, spacing: 10) {
                    Image(systemName: selection == .local ? "checkmark.circle.fill" : "circle")
                        .foregroundStyle(selection == .local ? Color.accentColor : .secondary)
                        .font(.title3)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("LocalContacts")
                            .font(.caption.weight(.medium))
                            .foregroundStyle(.secondary)
                        Text(localValue)
                            .font(.body)
                            .foregroundStyle(.primary)
                    }
                    Spacer()
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            Divider()

            Button { onSelect(.apple) } label: {
                HStack(alignment: .top, spacing: 10) {
                    Image(systemName: selection == .apple ? "checkmark.circle.fill" : "circle")
                        .foregroundStyle(selection == .apple ? .orange : .secondary)
                        .font(.title3)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Apple Contacts")
                            .font(.caption.weight(.medium))
                            .foregroundStyle(.orange)
                        Text(appleValue)
                            .font(.body)
                            .foregroundStyle(.primary)
                    }
                    Spacer()
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
        .padding(.vertical, 4)
    }
}
