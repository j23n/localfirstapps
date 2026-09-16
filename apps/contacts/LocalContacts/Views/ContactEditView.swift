import SwiftUI
import PhotosUI

struct ContactEditView: View {
    @Environment(ContactsStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var draft: ContactEditDraft
    let isNew: Bool

    @State private var urls: [IdentifiedLabeled]
    @State private var phones: [IdentifiedLabeled]
    @State private var emails: [IdentifiedLabeled]
    @State private var addresses: [IdentifiedAddress]
    @State private var selectedPhoto: PhotosPickerItem?
    @State private var isSaving = false
    @State private var saveError: String?
    @State private var newTag = ""
    @State private var hasBirthday: Bool
    @State private var birthdayDate: Date
    @State private var lastComposedName: String

    init(draft: ContactEditDraft, isNew: Bool) {
        self._draft = State(initialValue: draft)
        self.isNew = isNew
        self._urls = State(initialValue: draft.urls.map(IdentifiedLabeled.init))
        self._phones = State(initialValue: draft.phones.map(IdentifiedLabeled.init))
        self._emails = State(initialValue: draft.emails.map(IdentifiedLabeled.init))
        self._addresses = State(initialValue: draft.addresses.map(IdentifiedAddress.init))
        self._hasBirthday = State(initialValue: draft.birthday != nil)
        self._lastComposedName = State(initialValue: Self.structuredName(
            given: draft.givenName,
            middle: draft.middleName,
            family: draft.familyName
        ))
        if let birthday = draft.birthday,
           let date = Calendar.current.date(from: DateComponents(
            year: birthday.year.map(Int.init),
            month: Int(birthday.month),
            day: Int(birthday.day)
           )) {
            self._birthdayDate = State(initialValue: date)
        } else {
            self._birthdayDate = State(initialValue: Date())
        }
    }

    var body: some View {
        Form {
            Section {
                HStack {
                    Spacer()
                    VStack(spacing: 8) {
                        AvatarView(
                            photoData: draft.photo,
                            initials: Self.initials(from: draft),
                            size: 80
                        )
                        let hasPhoto = draft.photo != nil
                        PhotosPicker(selection: $selectedPhoto, matching: .images) {
                            Text(hasPhoto ? "Change Photo" : "Add Photo")
                                .font(.subheadline)
                        }
                        if draft.photo != nil {
                            Button("Remove Photo", role: .destructive) {
                                draft.photo = nil
                            }
                            .font(.subheadline)
                        }
                    }
                    Spacer()
                }
                .listRowBackground(Color.clear)
            }

            Section("Name") {
                TextField("Display Name", text: $draft.fullName)
                TextField("First Name", text: $draft.givenName)
                    .onChange(of: draft.givenName) { _, _ in syncDerivedFullName() }
                TextField("Middle Name", text: $draft.middleName)
                    .onChange(of: draft.middleName) { _, _ in syncDerivedFullName() }
                TextField("Last Name", text: $draft.familyName)
                    .onChange(of: draft.familyName) { _, _ in syncDerivedFullName() }
                TextField("Name Prefix", text: $draft.namePrefix)
                TextField("Name Suffix", text: $draft.nameSuffix)
            }

            Section("Organization") {
                TextField("Company", text: $draft.organization)
                TextField("Job Title", text: $draft.jobTitle)
                TextField("Nickname", text: $draft.nickname)
            }

            Section("Websites") {
                ForEach(urls) { row in
                    HStack {
                        TextField("URL", text: labeledBinding($urls, id: row.id, to: \.value))
                            .keyboardType(.URL)
                            .textInputAutocapitalization(.never)

                        Button {
                            urls.removeAll { $0.id == row.id }
                        } label: {
                            Image(systemName: "minus.circle.fill")
                                .foregroundStyle(.red)
                        }
                        .buttonStyle(.plain)
                    }
                }

                Button {
                    urls.append(IdentifiedLabeled(label: "homepage", value: ""))
                } label: {
                    Label("Add Website", systemImage: "plus.circle.fill")
                }
            }

            Section("Phone Numbers") {
                ForEach(phones) { row in
                    HStack {
                        Picker("", selection: labeledBinding($phones, id: row.id, to: \.label)) {
                            ForEach(["mobile", "home", "work", "main", "iphone", "other"], id: \.self) {
                                Text($0.capitalized).tag($0)
                            }
                        }
                        .labelsHidden()
                        .frame(width: 100)

                        TextField("Phone", text: labeledBinding($phones, id: row.id, to: \.value))
                            .keyboardType(.phonePad)

                        Button {
                            phones.removeAll { $0.id == row.id }
                        } label: {
                            Image(systemName: "minus.circle.fill")
                                .foregroundStyle(.red)
                        }
                        .buttonStyle(.plain)
                    }
                }

                Button {
                    phones.append(IdentifiedLabeled(label: "mobile", value: ""))
                } label: {
                    Label("Add Phone", systemImage: "plus.circle.fill")
                }
            }

            Section("Email Addresses") {
                ForEach(emails) { row in
                    HStack {
                        Picker("", selection: labeledBinding($emails, id: row.id, to: \.label)) {
                            ForEach(["home", "work", "other"], id: \.self) {
                                Text($0.capitalized).tag($0)
                            }
                        }
                        .labelsHidden()
                        .frame(width: 100)

                        TextField("Email", text: labeledBinding($emails, id: row.id, to: \.value))
                            .keyboardType(.emailAddress)
                            .textInputAutocapitalization(.never)

                        Button {
                            emails.removeAll { $0.id == row.id }
                        } label: {
                            Image(systemName: "minus.circle.fill")
                                .foregroundStyle(.red)
                        }
                        .buttonStyle(.plain)
                    }
                }

                Button {
                    emails.append(IdentifiedLabeled(label: "home", value: ""))
                } label: {
                    Label("Add Email", systemImage: "plus.circle.fill")
                }
            }

            Section("Addresses") {
                ForEach(addresses) { row in
                    VStack(alignment: .leading, spacing: 8) {
                        HStack {
                            Picker("", selection: addressBinding($addresses, id: row.id, to: \.label)) {
                                ForEach(["home", "work", "other"], id: \.self) {
                                    Text($0.capitalized).tag($0)
                                }
                            }
                            .labelsHidden()

                            Spacer()

                            Button {
                                addresses.removeAll { $0.id == row.id }
                            } label: {
                                Image(systemName: "minus.circle.fill")
                                    .foregroundStyle(.red)
                            }
                            .buttonStyle(.plain)
                        }

                        TextField("Street", text: addressBinding($addresses, id: row.id, to: \.street))
                        TextField("City", text: addressBinding($addresses, id: row.id, to: \.city))
                        HStack {
                            TextField("State", text: addressBinding($addresses, id: row.id, to: \.state))
                            TextField("ZIP", text: addressBinding($addresses, id: row.id, to: \.postalCode))
                                .frame(width: 100)
                        }
                        TextField("Country", text: addressBinding($addresses, id: row.id, to: \.country))
                    }
                    .padding(.vertical, 4)
                }

                Button {
                    addresses.append(IdentifiedAddress(label: "home"))
                } label: {
                    Label("Add Address", systemImage: "plus.circle.fill")
                }
            }

            Section("Birthday") {
                Toggle("Birthday", isOn: $hasBirthday)
                if hasBirthday {
                    DatePicker("Date", selection: $birthdayDate, displayedComponents: .date)
                }
            }

            Section("Notes") {
                TextEditor(text: $draft.note)
                    .frame(minHeight: 80)
            }

            Section("Tags") {
                ForEach(draft.categories, id: \.self) { tag in
                    HStack {
                        Text(tag)
                        Spacer()
                        Button {
                            draft.categories.removeAll { $0 == tag }
                        } label: {
                            Image(systemName: "xmark.circle.fill")
                                .foregroundStyle(.secondary)
                        }
                        .buttonStyle(.plain)
                    }
                }

                HStack {
                    TextField("New tag", text: $newTag)
                        .onSubmit {
                            addTag()
                        }
                    Button("Add") {
                        addTag()
                    }
                    .disabled(newTag.trimmingCharacters(in: .whitespaces).isEmpty)
                }

                let existingTags = store.allTags.map(\.tag).filter { !draft.categories.contains($0) }
                if !existingTags.isEmpty {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack {
                            ForEach(existingTags, id: \.self) { tag in
                                Button(tag) {
                                    draft.categories.append(tag)
                                }
                                .buttonStyle(.bordered)
                                .controlSize(.small)
                            }
                        }
                    }
                }
            }
        }
        .navigationTitle(isNew ? "New Contact" : "Edit Contact")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button("Cancel") { dismiss() }
            }
            ToolbarItem(placement: .confirmationAction) {
                Button("Save") {
                    saveDraft()
                }
                .disabled(isSaving)
            }
        }
        .onChange(of: selectedPhoto) { _, newItem in
            Task {
                if let data = try? await newItem?.loadTransferable(type: Data.self),
                   let uiImage = UIImage(data: data),
                   let jpeg = uiImage.jpegData(compressionQuality: 0.8) {
                    draft.photo = jpeg
                }
            }
        }
        .alert("Save Error", isPresented: .init(
            get: { saveError != nil },
            set: { if !$0 { saveError = nil } }
        )) {
            Button("OK") { saveError = nil }
        } message: {
            Text(saveError ?? "")
        }
    }

    private func labeledBinding(
        _ rows: Binding<[IdentifiedLabeled]>,
        id: UUID,
        to field: WritableKeyPath<IdentifiedLabeled, String>
    ) -> Binding<String> {
        Binding(
            get: { rows.wrappedValue.first { $0.id == id }?[keyPath: field] ?? "" },
            set: { newValue in
                if let index = rows.wrappedValue.firstIndex(where: { $0.id == id }) {
                    rows.wrappedValue[index][keyPath: field] = newValue
                }
            }
        )
    }

    private func addressBinding(
        _ rows: Binding<[IdentifiedAddress]>,
        id: UUID,
        to field: WritableKeyPath<IdentifiedAddress, String>
    ) -> Binding<String> {
        Binding(
            get: { rows.wrappedValue.first { $0.id == id }?[keyPath: field] ?? "" },
            set: { newValue in
                if let index = rows.wrappedValue.firstIndex(where: { $0.id == id }) {
                    rows.wrappedValue[index][keyPath: field] = newValue
                }
            }
        )
    }

    private func syncDerivedFullName() {
        let composed = Self.structuredName(
            given: draft.givenName,
            middle: draft.middleName,
            family: draft.familyName
        )
        if draft.fullName.isEmpty || draft.fullName == lastComposedName {
            draft.fullName = composed
        }
        lastComposedName = composed
    }

    static func structuredName(given: String, middle: String, family: String) -> String {
        [given, middle, family].filter { !$0.isEmpty }.joined(separator: " ")
    }

    private func addTag() {
        let tag = newTag.trimmingCharacters(in: .whitespaces)
        guard !tag.isEmpty, !draft.categories.contains(tag) else { return }
        draft.categories.append(tag)
        newTag = ""
    }

    private func saveDraft() {
        isSaving = true
        draft.urls = urls.map(\.draft)
        draft.phones = phones.map(\.draft)
        draft.emails = emails.map(\.draft)
        draft.addresses = addresses.map(\.draft)
        if hasBirthday {
            let parts = Calendar.current.dateComponents([.year, .month, .day], from: birthdayDate)
            if let month = parts.month, let day = parts.day {
                draft.birthday = BirthdayDraft(
                    year: parts.year.map(Int32.init),
                    month: UInt8(clamping: month),
                    day: UInt8(clamping: day)
                )
            }
        } else {
            draft.birthday = nil
        }

        Task {
            do {
                let saved = try await store.save(draft)
                if let id = saved.id,
                   let contact = store.contacts.first(where: { $0.localContactsID == id }) {
                    do {
                        try await store.syncService.pushContact(contact)
                    } catch {
                        store.errorMessage = "Saved locally, but Apple Contacts sync failed: \(error.localizedDescription)"
                    }
                }

                await MainActor.run {
                    dismiss()
                }
            } catch {
                saveError = error.localizedDescription
                isSaving = false
            }
        }
    }

    static func initials(from draft: ContactEditDraft) -> String {
        let parts = [draft.givenName, draft.familyName].filter { !$0.isEmpty }
        if parts.isEmpty { return "?" }
        return parts.map { String($0.prefix(1)).uppercased() }.joined()
    }
}

private struct IdentifiedLabeled: Identifiable {
    let id: UUID
    var label: String
    var value: String

    init(id: UUID = UUID(), label: String, value: String) {
        self.id = id
        self.label = label
        self.value = value
    }

    init(_ draft: LabeledValueDraft) {
        self.init(label: draft.label, value: draft.value)
    }

    var draft: LabeledValueDraft {
        LabeledValueDraft(label: label, value: value)
    }
}

private struct IdentifiedAddress: Identifiable {
    let id: UUID
    var label: String
    var street: String
    var city: String
    var state: String
    var postalCode: String
    var country: String

    init(
        id: UUID = UUID(),
        label: String,
        street: String = "",
        city: String = "",
        state: String = "",
        postalCode: String = "",
        country: String = ""
    ) {
        self.id = id
        self.label = label
        self.street = street
        self.city = city
        self.state = state
        self.postalCode = postalCode
        self.country = country
    }

    init(_ draft: LabeledAddressDraft) {
        self.init(
            label: draft.label,
            street: draft.street,
            city: draft.city,
            state: draft.state,
            postalCode: draft.postalCode,
            country: draft.country
        )
    }

    var draft: LabeledAddressDraft {
        LabeledAddressDraft(
            label: label,
            street: street,
            city: city,
            state: state,
            postalCode: postalCode,
            country: country
        )
    }
}
