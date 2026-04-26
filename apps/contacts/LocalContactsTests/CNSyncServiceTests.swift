import Contacts
import Foundation
import Testing
@testable import LocalContacts

/// Tests for CNSyncService logic that doesn't require the system contacts DB.
/// `CNContact`/`CNMutableContact` are plain data types — they don't need
/// authorization to instantiate or compare. Anything that hits CNContactStore
/// (push, fetchChanges, fullReconciliation) is intentionally excluded; per
/// the test plan those need a CNContactStoreProtocol shim, which is a
/// separate refactor.
@Suite("CNSyncService — pure logic")
struct CNSyncServiceTests {

    // MARK: - cnLabel mapping

    @Test("cnLabel maps known phone labels to the documented constants")
    func cnLabelPhones() {
        let svc = CNSyncService()
        #expect(svc.cnLabel(from: "home", isPhone: true) == CNLabelHome)
        #expect(svc.cnLabel(from: "work", isPhone: true) == CNLabelWork)
        #expect(svc.cnLabel(from: "mobile", isPhone: true) == CNLabelPhoneNumberMobile)
        #expect(svc.cnLabel(from: "cell", isPhone: true) == CNLabelPhoneNumberMobile)
        #expect(svc.cnLabel(from: "main", isPhone: true) == CNLabelPhoneNumberMain)
        #expect(svc.cnLabel(from: "iphone", isPhone: true) == CNLabelPhoneNumberiPhone)
        #expect(svc.cnLabel(from: "fax", isPhone: true) == CNLabelPhoneNumberWorkFax)
        #expect(svc.cnLabel(from: "pager", isPhone: true) == CNLabelPhoneNumberPager)
        #expect(svc.cnLabel(from: "other", isPhone: true) == CNLabelOther)
        #expect(svc.cnLabel(from: "unknown-label", isPhone: true) == CNLabelOther)
    }

    @Test("cnLabel: fax for non-phone falls back to work")
    func cnLabelFaxNonPhone() {
        let svc = CNSyncService()
        #expect(svc.cnLabel(from: "fax", isPhone: false) == CNLabelWork)
    }

    @Test("cnLabel is case-insensitive on input")
    func cnLabelCaseInsensitive() {
        let svc = CNSyncService()
        #expect(svc.cnLabel(from: "HOME", isPhone: true) == CNLabelHome)
        #expect(svc.cnLabel(from: "MoBiLe", isPhone: true) == CNLabelPhoneNumberMobile)
    }

    // MARK: - contactDiffers

    private func makeCN(
        given: String = "",
        family: String = "",
        middle: String = "",
        prefix: String = "",
        suffix: String = "",
        org: String = "",
        title: String = "",
        nickname: String = "",
        phones: [String] = [],
        emails: [String] = [],
        urls: [String] = [],
        birthday: DateComponents? = nil,
        addresses: [(street: String, city: String, state: String, zip: String, country: String)] = [],
        imageData: Data? = nil
    ) -> CNContact {
        let cn = CNMutableContact()
        cn.givenName = given
        cn.familyName = family
        cn.middleName = middle
        cn.namePrefix = prefix
        cn.nameSuffix = suffix
        cn.organizationName = org
        cn.jobTitle = title
        cn.nickname = nickname
        cn.phoneNumbers = phones.map {
            CNLabeledValue(label: CNLabelPhoneNumberMobile, value: CNPhoneNumber(stringValue: $0))
        }
        cn.emailAddresses = emails.map {
            CNLabeledValue(label: CNLabelHome, value: $0 as NSString)
        }
        cn.urlAddresses = urls.map {
            CNLabeledValue(label: CNLabelHome, value: $0 as NSString)
        }
        if let bday = birthday { cn.birthday = bday }
        cn.postalAddresses = addresses.map { addr in
            let p = CNMutablePostalAddress()
            p.street = addr.street
            p.city = addr.city
            p.state = addr.state
            p.postalCode = addr.zip
            p.country = addr.country
            return CNLabeledValue(label: CNLabelHome, value: p)
        }
        if let imageData { cn.imageData = imageData }
        return cn
    }

    private func makeLocal(
        given: String = "",
        family: String = "",
        middle: String = "",
        prefix: String = "",
        suffix: String = "",
        org: String = "",
        title: String = "",
        nickname: String = "",
        phones: [String] = [],
        emails: [String] = [],
        urls: [String] = [],
        birthday: DateComponents? = nil,
        addresses: [(street: String, city: String, state: String, zip: String, country: String)] = [],
        photoData: Data? = nil
    ) -> Contact {
        Contact(
            familyName: family,
            givenName: given,
            middleName: middle,
            namePrefix: prefix,
            nameSuffix: suffix,
            organization: org,
            jobTitle: title,
            nickname: nickname,
            urls: urls.map { LabeledValue(label: "home", value: $0) },
            phoneNumbers: phones.map { LabeledValue(label: "mobile", value: $0) },
            emailAddresses: emails.map { LabeledValue(label: "home", value: $0) },
            postalAddresses: addresses.map {
                LabeledValue(label: "home", value: PostalAddress(
                    street: $0.street, city: $0.city, state: $0.state,
                    postalCode: $0.zip, country: $0.country
                ))
            },
            birthday: birthday,
            photoData: photoData
        )
    }

    @Test("identical contacts do not differ")
    func notDifferent() {
        let svc = CNSyncService()
        let local = makeLocal(
            given: "Alice", family: "Wonder",
            org: "Acme", title: "CTO", nickname: "Ali",
            phones: ["+15551234"], emails: ["a@b.com"], urls: ["https://x"],
            birthday: DateComponents(year: 1985, month: 3, day: 14),
            addresses: [("1 Main", "Springfield", "IL", "62701", "USA")]
        )
        let cn = makeCN(
            given: "Alice", family: "Wonder",
            org: "Acme", title: "CTO", nickname: "Ali",
            phones: ["+15551234"], emails: ["a@b.com"], urls: ["https://x"],
            birthday: DateComponents(year: 1985, month: 3, day: 14),
            addresses: [("1 Main", "Springfield", "IL", "62701", "USA")]
        )
        #expect(svc.contactDiffers(local: local, cn: cn) == false)
    }

    @Test("givenName change is detected")
    func diffGivenName() {
        let svc = CNSyncService()
        let local = makeLocal(given: "Alice")
        let cn = makeCN(given: "Alicia")
        #expect(svc.contactDiffers(local: local, cn: cn))
    }

    @Test("phone set change is detected")
    func diffPhones() {
        let svc = CNSyncService()
        let local = makeLocal(phones: ["+15551234"])
        let cn = makeCN(phones: ["+15559999"])
        #expect(svc.contactDiffers(local: local, cn: cn))
    }

    @Test("email comparison is case-insensitive")
    func emailCaseInsensitive() {
        let svc = CNSyncService()
        let local = makeLocal(emails: ["alice@Example.COM"])
        let cn = makeCN(emails: ["alice@example.com"])
        #expect(svc.contactDiffers(local: local, cn: cn) == false)
    }

    @Test("phone label-only change is NOT a diff (values compared)")
    func phoneLabelOnly() {
        let svc = CNSyncService()
        let local = Contact(
            phoneNumbers: [LabeledValue(label: "home", value: "+15551234")]
        )
        let cn = makeCN(phones: ["+15551234"]) // mobile label
        #expect(svc.contactDiffers(local: local, cn: cn) == false)
    }

    @Test("address change is detected")
    func diffAddress() {
        let svc = CNSyncService()
        let local = makeLocal(addresses: [("1 Main", "Springfield", "IL", "62701", "USA")])
        let cn = makeCN(addresses: [("2 Oak", "Springfield", "IL", "62701", "USA")])
        #expect(svc.contactDiffers(local: local, cn: cn))
    }

    @Test("URL change is detected")
    func diffURL() {
        let svc = CNSyncService()
        let local = makeLocal(urls: ["https://a"])
        let cn = makeCN(urls: ["https://b"])
        #expect(svc.contactDiffers(local: local, cn: cn))
    }

    @Test("birthday change is detected")
    func diffBirthday() {
        let svc = CNSyncService()
        let local = makeLocal(birthday: DateComponents(year: 1985, month: 3, day: 14))
        let cn = makeCN(birthday: DateComponents(year: 1990, month: 1, day: 1))
        #expect(svc.contactDiffers(local: local, cn: cn))
    }

    @Test("photo bytes differ but contactDiffers returns false (intentional)")
    func photoIgnored() {
        // CNContactStore re-encodes photo data, which would otherwise trigger
        // false-positive conflicts. Verify the documented behavior.
        let svc = CNSyncService()
        let local = makeLocal(given: "Alice", photoData: Data([0xFF, 0xD8, 0xFF, 0xE0]))
        let cn = makeCN(given: "Alice", imageData: Data([0xDE, 0xAD, 0xBE, 0xEF]))
        #expect(svc.contactDiffers(local: local, cn: cn) == false)
    }
}
