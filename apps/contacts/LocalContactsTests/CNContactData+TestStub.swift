import Foundation
@testable import LocalContacts

extension CNSyncService.CNContactData {
    static func stub(
        cnIdentifier: String = "cn-1",
        localContactsID: String = "lcid-1",
        givenName: String = "Alicia",
        familyName: String = "Wonder",
        middleName: String = "",
        namePrefix: String = "",
        nameSuffix: String = "",
        organization: String = "Acme",
        jobTitle: String = "CTO",
        nickname: String = "Ali",
        urls: [(label: String, value: String)] = [("homepage", "https://apple.example")],
        phoneNumbers: [(label: String, value: String)] = [("mobile", "+15559999")],
        emailAddresses: [(label: String, value: String)] = [("home", "ali@apple.example")],
        postalAddresses: [(label: String, street: String, city: String, state: String, postalCode: String, country: String)] = [
            ("home", "2 Oak", "Springfield", "IL", "62701", "USA")
        ],
        birthday: DateComponents? = DateComponents(year: 1990, month: 1, day: 1),
        imageData: Data? = nil
    ) -> CNSyncService.CNContactData {
        CNSyncService.CNContactData(
            cnIdentifier: cnIdentifier,
            localContactsID: localContactsID,
            givenName: givenName,
            familyName: familyName,
            middleName: middleName,
            namePrefix: namePrefix,
            nameSuffix: nameSuffix,
            organization: organization,
            jobTitle: jobTitle,
            nickname: nickname,
            urls: urls,
            phoneNumbers: phoneNumbers,
            emailAddresses: emailAddresses,
            postalAddresses: postalAddresses,
            birthday: birthday,
            imageData: imageData
        )
    }
}
