import SwiftUI
import PhotosUI

struct ContactEditView: View {
    @Environment(ContactsStore.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State var contact: Contact
    let isNew: Bool

    @State private var selectedPhoto: PhotosPickerItem?
    @State private var isSaving = false
    @State private var saveError: String?
    @State private var newTag = ""

    // Birthday editing
    @State private var hasBirthday: Bool
    @State private var birthdayDate: Date

    init(contact: Contact, isNew: Bool) {
        self._contact = State(initialValue: contact)
        self.isNew = isNew
        let hasBday = contact.birthday != nil
        self._hasBirthday = State(initialValue: hasBday)
        if let bday = contact.birthday,
           let date = Calendar.current.date(from: bday) {
            self._birthdayDate = State(initialValue: date)
        } else {
            self._birthdayDate = State(initialValue: Date())
        }
    }

    var body: some View {
        Form {
            // Photo
            Section {
                HStack {
                    Spacer()
                    VStack(spacing: 8) {
                        AvatarView(contact: contact, size: 80)
                        PhotosPicker(selection: $selectedPhoto, matching: .images) {
                            Text(contact.photoData != nil ? "Change Photo" : "Add Photo")
                                .font(.subheadline)
                        }
                        if contact.photoData != nil {
                            Button("Remove Photo", role: .destructive) {
                                contact.photoData = nil
                            }
                            .font(.subheadline)
                        }
                    }
                    Spacer()
                }
                .listRowBackground(Color.clear)
            }

            // Name
            Section("Name") {
                TextField("First Name", text: $contact.givenName)
                TextField("Middle Name", text: $contact.middleName)
                TextField("Last Name", text: $contact.familyName)
                TextField("Name Prefix", text: $contact.namePrefix)
                TextField("Name Suffix", text: $contact.nameSuffix)
            }

            // Organization
            Section("Organization") {
                TextField("Company", text: $contact.organization)
                TextField("Job Title", text: $contact.jobTitle)
                TextField("Nickname", text: $contact.nickname)
            }

            // Websites
            Section("Websites") {
                ForEach($contact.urls) { $url in
                    HStack {
                        TextField("URL", text: $url.value)
                            .keyboardType(.URL)
                            .textInputAutocapitalization(.never)

                        Button {
                            contact.urls.removeAll { $0.id == url.id }
                        } label: {
                            Image(systemName: "minus.circle.fill")
                                .foregroundStyle(.red)
                        }
                        .buttonStyle(.plain)
                    }
                }

                Button {
                    contact.urls.append(LabeledValue(label: "homepage", value: ""))
                } label: {
                    Label("Add Website", systemImage: "plus.circle.fill")
                }
            }

            // Phone Numbers
            Section("Phone Numbers") {
                ForEach($contact.phoneNumbers) { $phone in
                    HStack {
                        Picker("", selection: $phone.label) {
                            ForEach(["mobile", "home", "work", "main", "iphone", "other"], id: \.self) {
                                Text($0.capitalized).tag($0)
                            }
                        }
                        .labelsHidden()
                        .frame(width: 100)

                        TextField("Phone", text: $phone.value)
                            .keyboardType(.phonePad)

                        Button {
                            contact.phoneNumbers.removeAll { $0.id == phone.id }
                        } label: {
                            Image(systemName: "minus.circle.fill")
                                .foregroundStyle(.red)
                        }
                        .buttonStyle(.plain)
                    }
                }

                Button {
                    contact.phoneNumbers.append(LabeledValue(label: "mobile", value: ""))
                } label: {
                    Label("Add Phone", systemImage: "plus.circle.fill")
                }
            }

            // Email
            Section("Email Addresses") {
                ForEach($contact.emailAddresses) { $email in
                    HStack {
                        Picker("", selection: $email.label) {
                            ForEach(["home", "work", "other"], id: \.self) {
                                Text($0.capitalized).tag($0)
                            }
                        }
                        .labelsHidden()
                        .frame(width: 100)

                        TextField("Email", text: $email.value)
                            .keyboardType(.emailAddress)
                            .textInputAutocapitalization(.never)

                        Button {
                            contact.emailAddresses.removeAll { $0.id == email.id }
                        } label: {
                            Image(systemName: "minus.circle.fill")
                                .foregroundStyle(.red)
                        }
                        .buttonStyle(.plain)
                    }
                }

                Button {
                    contact.emailAddresses.append(LabeledValue(label: "home", value: ""))
                } label: {
                    Label("Add Email", systemImage: "plus.circle.fill")
                }
            }

            // Addresses
            Section("Addresses") {
                ForEach($contact.postalAddresses) { $addr in
                    VStack(alignment: .leading, spacing: 8) {
                        HStack {
                            Picker("", selection: $addr.label) {
                                ForEach(["home", "work", "other"], id: \.self) {
                                    Text($0.capitalized).tag($0)
                                }
                            }
                            .labelsHidden()

                            Spacer()

                            Button {
                                contact.postalAddresses.removeAll { $0.id == addr.id }
                            } label: {
                                Image(systemName: "minus.circle.fill")
                                    .foregroundStyle(.red)
                            }
                            .buttonStyle(.plain)
                        }

                        TextField("Street", text: $addr.value.street)
                        TextField("City", text: $addr.value.city)
                        HStack {
                            TextField("State", text: $addr.value.state)
                            TextField("ZIP", text: $addr.value.postalCode)
                                .frame(width: 100)
                        }
                        TextField("Country", text: $addr.value.country)
                    }
                    .padding(.vertical, 4)
                }

                Button {
                    contact.postalAddresses.append(LabeledValue(label: "home", value: PostalAddress()))
                } label: {
                    Label("Add Address", systemImage: "plus.circle.fill")
                }
            }

            // Birthday
            Section("Birthday") {
                Toggle("Birthday", isOn: $hasBirthday)
                if hasBirthday {
                    DatePicker("Date", selection: $birthdayDate, displayedComponents: .date)
                }
            }

            // Notes
            Section("Notes") {
                TextEditor(text: $contact.note)
                    .frame(minHeight: 80)
            }

            // Tags
            Section("Tags") {
                ForEach(contact.categories, id: \.self) { tag in
                    HStack {
                        Text(tag)
                        Spacer()
                        Button {
                            contact.categories.removeAll { $0 == tag }
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

                // Existing tags suggestion
                let existingTags = store.allTags.map(\.tag).filter { !contact.categories.contains($0) }
                if !existingTags.isEmpty {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack {
                            ForEach(existingTags, id: \.self) { tag in
                                Button(tag) {
                                    contact.categories.append(tag)
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
                    saveContact()
                }
                .disabled(isSaving)
            }
        }
        .onChange(of: selectedPhoto) { _, newItem in
            Task {
                if let data = try? await newItem?.loadTransferable(type: Data.self) {
                    // Compress to JPEG
                    if let uiImage = UIImage(data: data),
                       let jpeg = uiImage.jpegData(compressionQuality: 0.8) {
                        contact.photoData = jpeg
                    }
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

    private func addTag() {
        let tag = newTag.trimmingCharacters(in: .whitespaces)
        guard !tag.isEmpty, !contact.categories.contains(tag) else { return }
        contact.categories.append(tag)
        newTag = ""
    }

    private func saveContact() {
        isSaving = true

        // Update computed full name
        let parts = [contact.givenName, contact.middleName, contact.familyName].filter { !$0.isEmpty }
        contact.fullName = parts.joined(separator: " ")

        // Update birthday
        if hasBirthday {
            contact.birthday = Calendar.current.dateComponents([.year, .month, .day], from: birthdayDate)
        } else {
            contact.birthday = nil
        }

        Task {
            do {
                try await store.save(contact)

                // Push to CNContactStore
                try? await store.syncService.pushContact(contact)

                await MainActor.run {
                    dismiss()
                }
            } catch {
                saveError = error.localizedDescription
                isSaving = false
            }
        }
    }
}
