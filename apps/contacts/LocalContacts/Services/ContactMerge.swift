import Foundation

/// Pure field-by-field merge of Apple Contacts data onto a local `Contact`.
/// Does not save, push, or clear `conflictState` — callers do that after I/O succeeds.
enum ContactMerge {
    enum FieldSource: Equatable, Sendable {
        case local
        case apple
    }

    static func apply(
        selections: [String: FieldSource],
        external: CNSyncService.CNContactData,
        to contact: Contact
    ) {
        for (key, source) in selections {
            guard source == .apple else { continue }
            switch key {
            case "name":
                contact.givenName = external.givenName
                contact.familyName = external.familyName
                contact.middleName = external.middleName
                contact.namePrefix = external.namePrefix
                contact.nameSuffix = external.nameSuffix
                contact.fullName = [external.givenName, external.middleName, external.familyName]
                    .filter { !$0.isEmpty }.joined(separator: " ")
            case "organization":
                contact.organization = external.organization
            case "jobTitle":
                contact.jobTitle = external.jobTitle
            case "nickname":
                contact.nickname = external.nickname
            case "phones":
                contact.phoneNumbers = external.phoneNumbers.map {
                    LabeledValue(label: $0.label, value: $0.value)
                }
            case "emails":
                contact.emailAddresses = external.emailAddresses.map {
                    LabeledValue(label: $0.label, value: $0.value)
                }
            case "urls":
                contact.urls = external.urls.map {
                    LabeledValue(label: $0.label, value: $0.value)
                }
            case "addresses":
                contact.postalAddresses = external.postalAddresses.map {
                    LabeledValue(label: $0.label, value: PostalAddress(
                        street: $0.street, city: $0.city, state: $0.state,
                        postalCode: $0.postalCode, country: $0.country
                    ))
                }
            case "birthday":
                contact.birthday = external.birthday
            default:
                break
            }
        }

        // Photos aren't diffed. Keep local unless it's nil and Apple has one.
        if contact.photoData == nil, let applePhoto = external.imageData {
            contact.photoData = applePhoto
        }
    }
}
