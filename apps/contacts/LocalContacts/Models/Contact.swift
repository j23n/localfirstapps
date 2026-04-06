import Foundation
import Observation

@Observable
final class Contact: Identifiable, @unchecked Sendable {
    let id: UUID
    var localContactsID: String
    var fileName: String

    // Name
    var fullName: String
    var familyName: String
    var givenName: String
    var middleName: String
    var namePrefix: String
    var nameSuffix: String

    // Organization
    var organization: String
    var jobTitle: String
    var nickname: String
    var urls: [LabeledValue<String>]

    // Communication
    var phoneNumbers: [LabeledValue<String>]
    var emailAddresses: [LabeledValue<String>]
    var postalAddresses: [LabeledValue<PostalAddress>]

    // Other
    var birthday: DateComponents?
    var note: String
    var categories: [String]
    var photoData: Data?

    // Round-trip: unknown vCard lines preserved verbatim
    var unknownFields: [String]

    // Conflict tracking
    var conflictState: ConflictState?

    init(
        id: UUID = UUID(),
        localContactsID: String = UUID().uuidString,
        fileName: String = "",
        fullName: String = "",
        familyName: String = "",
        givenName: String = "",
        middleName: String = "",
        namePrefix: String = "",
        nameSuffix: String = "",
        organization: String = "",
        jobTitle: String = "",
        nickname: String = "",
        urls: [LabeledValue<String>] = [],
        phoneNumbers: [LabeledValue<String>] = [],
        emailAddresses: [LabeledValue<String>] = [],
        postalAddresses: [LabeledValue<PostalAddress>] = [],
        birthday: DateComponents? = nil,
        note: String = "",
        categories: [String] = [],
        photoData: Data? = nil,
        unknownFields: [String] = [],
        conflictState: ConflictState? = nil
    ) {
        self.id = id
        self.localContactsID = localContactsID
        self.fileName = fileName
        self.fullName = fullName
        self.familyName = familyName
        self.givenName = givenName
        self.middleName = middleName
        self.namePrefix = namePrefix
        self.nameSuffix = nameSuffix
        self.organization = organization
        self.jobTitle = jobTitle
        self.nickname = nickname
        self.urls = urls
        self.phoneNumbers = phoneNumbers
        self.emailAddresses = emailAddresses
        self.postalAddresses = postalAddresses
        self.birthday = birthday
        self.note = note
        self.categories = categories
        self.photoData = photoData
        self.unknownFields = unknownFields
        self.conflictState = conflictState
    }

    var displayName: String {
        if !fullName.isEmpty { return fullName }
        let parts = [givenName, middleName, familyName].filter { !$0.isEmpty }
        return parts.isEmpty ? "No Name" : parts.joined(separator: " ")
    }

    var initials: String {
        let parts = [givenName, familyName].filter { !$0.isEmpty }
        if parts.isEmpty { return "?" }
        return parts.map { String($0.prefix(1)).uppercased() }.joined()
    }

    var sortLetter: String {
        let name = familyName.isEmpty ? givenName : familyName
        guard let first = name.uppercased().first, first.isLetter else { return "#" }
        return String(first)
    }

    var age: Int? {
        guard let bday = birthday, let year = bday.year, let month = bday.month, let day = bday.day else {
            return nil
        }
        let calendar = Calendar.current
        let bdayDate = calendar.date(from: DateComponents(year: year, month: month, day: day))!
        let components = calendar.dateComponents([.year], from: bdayDate, to: Date())
        return components.year
    }

    func copy() -> Contact {
        Contact(
            id: self.id,
            localContactsID: self.localContactsID,
            fileName: self.fileName,
            fullName: self.fullName,
            familyName: self.familyName,
            givenName: self.givenName,
            middleName: self.middleName,
            namePrefix: self.namePrefix,
            nameSuffix: self.nameSuffix,
            organization: self.organization,
            jobTitle: self.jobTitle,
            nickname: self.nickname,
            urls: self.urls,
            phoneNumbers: self.phoneNumbers,
            emailAddresses: self.emailAddresses,
            postalAddresses: self.postalAddresses.map { LabeledValue(label: $0.label, value: $0.value.copy()) },
            birthday: self.birthday,
            note: self.note,
            categories: self.categories,
            photoData: self.photoData,
            unknownFields: self.unknownFields,
            conflictState: self.conflictState
        )
    }
}

struct LabeledValue<T: Sendable>: Identifiable, Sendable {
    let id = UUID()
    var label: String
    var value: T
}

struct PostalAddress: Sendable {
    var street: String = ""
    var city: String = ""
    var state: String = ""
    var postalCode: String = ""
    var country: String = ""

    var isEmpty: Bool {
        street.isEmpty && city.isEmpty && state.isEmpty && postalCode.isEmpty && country.isEmpty
    }

    var formatted: String {
        [street, city, [state, postalCode].filter { !$0.isEmpty }.joined(separator: " "), country]
            .filter { !$0.isEmpty }
            .joined(separator: "\n")
    }

    func copy() -> PostalAddress {
        PostalAddress(street: street, city: city, state: state, postalCode: postalCode, country: country)
    }
}

enum ConflictState: Sendable {
    case externalEdit
    case externalDelete
}
