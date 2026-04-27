import Foundation
import Testing
@testable import LocalContacts

@Suite("Contact")
struct ContactTests {

    // MARK: - displayName

    @Test("displayName prefers fullName when set")
    func displayNameUsesFullName() {
        let c = Contact(fullName: "Alice Wonder", givenName: "Alice")
        #expect(c.displayName == "Alice Wonder")
    }

    @Test("displayName joins given/middle/family when fullName empty")
    func displayNameJoinsParts() {
        let c = Contact(familyName: "Wonder", givenName: "Alice", middleName: "Q")
        #expect(c.displayName == "Alice Q Wonder")
    }

    @Test("displayName falls back to 'No Name' when nothing is set")
    func displayNameFallback() {
        let c = Contact()
        #expect(c.displayName == "No Name")
    }

    @Test("displayName skips empty parts when joining")
    func displayNameSkipsEmpty() {
        let c = Contact(familyName: "Wonder", givenName: "Alice")
        #expect(c.displayName == "Alice Wonder")
    }

    // MARK: - initials

    @Test("initials = uppercased first letter of given + family")
    func initials() {
        let c = Contact(familyName: "wonder", givenName: "alice")
        #expect(c.initials == "AW")
    }

    @Test("initials returns single letter when only one name part")
    func initialsSingle() {
        let c = Contact(givenName: "Alice")
        #expect(c.initials == "A")
    }

    @Test("initials returns ? when both parts empty")
    func initialsEmpty() {
        let c = Contact()
        #expect(c.initials == "?")
    }

    // MARK: - sortLetter

    @Test("sortLetter uses family name first letter uppercased")
    func sortLetterFamily() {
        let c = Contact(familyName: "wonder", givenName: "Alice")
        #expect(c.sortLetter == "W")
    }

    @Test("sortLetter falls back to given name when family is empty")
    func sortLetterGivenFallback() {
        let c = Contact(givenName: "Alice")
        #expect(c.sortLetter == "A")
    }

    @Test("sortLetter returns # for non-letter starts")
    func sortLetterNonLetter() {
        #expect(Contact(givenName: "123").sortLetter == "#")
        #expect(Contact(givenName: "!!!").sortLetter == "#")
        #expect(Contact().sortLetter == "#")
    }

    // MARK: - age

    @Test("age returns nil when birthday components are incomplete")
    func ageRequiresFullDate() {
        #expect(Contact(birthday: nil).age == nil)
        #expect(Contact(birthday: DateComponents(month: 3, day: 14)).age == nil)
        #expect(Contact(birthday: DateComponents(year: 1990)).age == nil)
    }

    @Test("age is computed from a full birthday")
    func ageFromBirthday() throws {
        let cal = Calendar.current
        let oneYearAgo = cal.date(byAdding: .year, value: -1, to: Date())!
        let bday = cal.dateComponents([.year, .month, .day], from: oneYearAgo)
        let c = Contact(birthday: bday)
        #expect(c.age == 1)
    }

    // MARK: - copy()

    @Test("copy() preserves all scalar fields")
    func copyPreservesFields() {
        let original = Contact(
            localContactsID: "lcid",
            fileName: "f.vcf",
            fullName: "Alice Wonder",
            familyName: "Wonder",
            givenName: "Alice",
            organization: "Acme",
            jobTitle: "CTO",
            nickname: "Ali",
            note: "n",
            categories: ["a"]
        )
        let copy = original.copy()
        #expect(copy.localContactsID == original.localContactsID)
        #expect(copy.fileName == original.fileName)
        #expect(copy.fullName == original.fullName)
        #expect(copy.organization == original.organization)
        #expect(copy.note == original.note)
        #expect(copy.categories == original.categories)
    }

    @Test("mutating a copy does not mutate the original")
    func copyIsIndependent() {
        let original = Contact(givenName: "Alice")
        let copy = original.copy()
        copy.givenName = "Bob"
        #expect(original.givenName == "Alice")
        #expect(copy.givenName == "Bob")
    }

    @Test("copy() deep-copies postal addresses (not shared references)")
    func copyDeepCopiesAddresses() {
        let addr = PostalAddress(street: "1 Main St", city: "Springfield")
        let original = Contact(
            givenName: "Alice",
            postalAddresses: [LabeledValue(label: "home", value: addr)]
        )
        let copy = original.copy()

        copy.postalAddresses[0].value.street = "2 Other St"

        // Original must be untouched.
        #expect(original.postalAddresses[0].value.street == "1 Main St")
        #expect(copy.postalAddresses[0].value.street == "2 Other St")
    }
}

@Suite("PostalAddress")
struct PostalAddressTests {

    @Test("isEmpty is true for default-init")
    func isEmptyDefault() {
        #expect(PostalAddress().isEmpty)
    }

    @Test("isEmpty is false when any field is set")
    func isEmptyWithStreet() {
        #expect(!PostalAddress(street: "x").isEmpty)
        #expect(!PostalAddress(city: "x").isEmpty)
        #expect(!PostalAddress(country: "x").isEmpty)
    }

    @Test("formatted joins city/state/zip/country with newlines and spaces")
    func formattedFull() {
        let a = PostalAddress(
            street: "1 Main St", city: "Springfield",
            state: "IL", postalCode: "62701", country: "USA"
        )
        #expect(a.formatted == "1 Main St\nSpringfield\nIL 62701\nUSA")
    }

    @Test("formatted omits empty lines entirely")
    func formattedOmitsEmpty() {
        let a = PostalAddress(street: "1 Main St", city: "Springfield", country: "USA")
        #expect(a.formatted == "1 Main St\nSpringfield\nUSA")
    }

    @Test("formatted is empty when all fields are empty")
    func formattedEmpty() {
        #expect(PostalAddress().formatted.isEmpty)
    }

    @Test("copy() returns an independent value")
    func copyIsIndependent() {
        let a = PostalAddress(street: "1 Main St")
        var b = a.copy()
        b.street = "2 Other St"
        #expect(a.street == "1 Main St")
        #expect(b.street == "2 Other St")
    }
}
