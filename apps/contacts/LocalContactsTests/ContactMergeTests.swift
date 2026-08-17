import Foundation
import Testing
@testable import LocalContacts

@Suite("ContactMerge")
struct ContactMergeTests {

    private func localContact() -> Contact {
        Contact(
            localContactsID: "lcid-1",
            fullName: "Alice Wonder",
            familyName: "Wonder",
            givenName: "Alice",
            organization: "LocalCo",
            jobTitle: "Dev",
            nickname: "Al",
            urls: [LabeledValue(label: "homepage", value: "https://local.example")],
            phoneNumbers: [LabeledValue(label: "mobile", value: "+15551111")],
            emailAddresses: [LabeledValue(label: "home", value: "alice@local.example")],
            postalAddresses: [LabeledValue(label: "home", value: PostalAddress(street: "1 Main", city: "Springfield"))],
            birthday: DateComponents(year: 1985, month: 3, day: 14),
            photoData: Data([0xFF]),
            conflictState: .externalDelete
        )
    }

    @Test("name-only Apple selection updates the five name parts and fullName")
    func nameApple() {
        let contact = localContact()
        let external = CNSyncService.CNContactData.stub(
            givenName: "Alicia", familyName: "Wonder", middleName: "Q",
            namePrefix: "Dr.", nameSuffix: "Jr."
        )
        ContactMerge.apply(selections: ["name": .apple], external: external, to: contact)
        #expect(contact.givenName == "Alicia")
        #expect(contact.familyName == "Wonder")
        #expect(contact.middleName == "Q")
        #expect(contact.namePrefix == "Dr.")
        #expect(contact.nameSuffix == "Jr.")
        #expect(contact.fullName == "Alicia Q Wonder")
        #expect(contact.organization == "LocalCo")
    }

    @Test("local name selection leaves name fields unchanged")
    func nameLocal() {
        let contact = localContact()
        ContactMerge.apply(
            selections: ["name": .local],
            external: .stub(givenName: "Alicia"),
            to: contact
        )
        #expect(contact.givenName == "Alice")
        #expect(contact.fullName == "Alice Wonder")
    }

    @Test("Apple phone/email/URL/address selection replaces the whole list")
    func listsReplacedNotMerged() {
        let contact = localContact()
        let external = CNSyncService.CNContactData.stub(
            urls: [("work", "https://new.example")],
            phoneNumbers: [("work", "+15552222"), ("home", "+15553333")],
            emailAddresses: [("work", "new@example.com")],
            postalAddresses: [("work", "9 Pine", "Chicago", "IL", "60601", "USA")]
        )
        ContactMerge.apply(
            selections: ["phones": .apple, "emails": .apple, "urls": .apple, "addresses": .apple],
            external: external,
            to: contact
        )
        #expect(contact.phoneNumbers.map(\.value) == ["+15552222", "+15553333"])
        #expect(contact.emailAddresses.map(\.value) == ["new@example.com"])
        #expect(contact.urls.map(\.value) == ["https://new.example"])
        #expect(contact.postalAddresses.first?.value.street == "9 Pine")
        #expect(contact.phoneNumbers.count == 2)
    }

    @Test("birthday Apple selection; local keeps the original")
    func birthday() {
        let apple = localContact()
        ContactMerge.apply(
            selections: ["birthday": .apple],
            external: .stub(birthday: DateComponents(year: 1990, month: 1, day: 1)),
            to: apple
        )
        #expect(apple.birthday?.year == 1990)

        let local = localContact()
        ContactMerge.apply(
            selections: ["birthday": .local],
            external: .stub(birthday: DateComponents(year: 1990, month: 1, day: 1)),
            to: local
        )
        #expect(local.birthday?.year == 1985)
    }

    @Test("photo is copied from Apple only when local photoData is nil")
    func photoOnlyIfLocalNil() {
        let withLocal = localContact()
        ContactMerge.apply(selections: [:], external: .stub(imageData: Data([0xAA])), to: withLocal)
        #expect(withLocal.photoData == Data([0xFF]))

        let withoutLocal = localContact()
        withoutLocal.photoData = nil
        ContactMerge.apply(selections: [:], external: .stub(imageData: Data([0xAA])), to: withoutLocal)
        #expect(withoutLocal.photoData == Data([0xAA]))
    }

    @Test("apply does not clear conflictState")
    func conflictPreserved() {
        let contact = localContact()
        ContactMerge.apply(selections: ["organization": .apple], external: .stub(), to: contact)
        #expect(contact.conflictState != nil)
    }
}
