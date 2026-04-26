import Foundation
import Testing
@testable import LocalContacts

@Suite("VCardWriter")
struct VCardWriterTests {
    let writer = VCardWriter()
    let parser = VCardParser()

    // MARK: - Required headers / line endings

    @Test("writes BEGIN/VERSION/X-LOCALCONTACTS-ID/END in order with CRLF")
    func headerOrder() {
        let c = Contact(localContactsID: "lcid-1", givenName: "Alice")
        let out = writer.write(c)
        #expect(out.hasSuffix("\r\n"))
        let lines = out.components(separatedBy: "\r\n")
        #expect(lines.first == "BEGIN:VCARD")
        #expect(lines.contains("VERSION:3.0"))
        #expect(lines.contains("X-LOCALCONTACTS-ID:lcid-1"))
        // END:VCARD is the last non-empty line
        #expect(lines.dropLast().last == "END:VCARD")
    }

    // MARK: - Empty optional fields are omitted

    @Test("ORG/TITLE/NICKNAME/NOTE/CATEGORIES omitted when empty")
    func optionalFieldsOmitted() {
        let c = Contact(givenName: "Alice")
        let out = writer.write(c)
        #expect(!out.contains("ORG:"))
        #expect(!out.contains("TITLE:"))
        #expect(!out.contains("NICKNAME:"))
        #expect(!out.contains("NOTE:"))
        #expect(!out.contains("CATEGORIES:"))
    }

    @Test("populated optional fields appear in output")
    func optionalFieldsPresent() {
        let c = Contact(
            givenName: "Alice",
            organization: "Acme",
            jobTitle: "CTO",
            nickname: "Al",
            note: "hi",
            categories: ["a", "b"]
        )
        let out = writer.write(c)
        #expect(out.contains("ORG:Acme"))
        #expect(out.contains("TITLE:CTO"))
        #expect(out.contains("NICKNAME:Al"))
        #expect(out.contains("NOTE:hi"))
        #expect(out.contains("CATEGORIES:a,b"))
    }

    // MARK: - Escaping

    @Test("escapes commas, semicolons, backslashes, and newlines in NOTE")
    func escapingNote() throws {
        let c = Contact(givenName: "X", note: "line1\nline2,with;chars\\back")
        let out = writer.write(c)
        // The rendered NOTE line must escape every meta-char so the parser can recover it.
        #expect(out.contains("NOTE:line1\\nline2\\,with\\;chars\\\\back"))

        // Round-trip: parser should restore the original string.
        let parsed = try #require(parser.parse(string: out, fileName: "x.vcf"))
        #expect(parsed.note == "line1\nline2,with;chars\\back")
    }

    @Test("escapes commas inside FN")
    func escapingFN() throws {
        let c = Contact(fullName: "Smith, John")
        let out = writer.write(c)
        #expect(out.contains("FN:Smith\\, John"))
        let parsed = try #require(parser.parse(string: out, fileName: "x.vcf"))
        #expect(parsed.fullName == "Smith, John")
    }

    // MARK: - Birthday

    @Test("BDAY with year writes YYYY-MM-DD")
    func bdayYear() {
        let c = Contact(givenName: "X", birthday: DateComponents(year: 1990, month: 1, day: 5))
        let out = writer.write(c)
        #expect(out.contains("BDAY:1990-01-05"))
    }

    @Test("BDAY without year writes --MM-DD")
    func bdayNoYear() {
        let c = Contact(givenName: "X", birthday: DateComponents(month: 7, day: 4))
        let out = writer.write(c)
        #expect(out.contains("BDAY:--07-04"))
    }

    @Test("missing BDAY produces no BDAY line")
    func bdayMissing() {
        let c = Contact(givenName: "X", birthday: nil)
        let out = writer.write(c)
        #expect(!out.contains("BDAY:"))
    }

    // MARK: - Photo

    @Test("PHOTO is emitted as base64 with TYPE=JPEG")
    func photoEncoding() {
        let payload = Data([0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10])
        let c = Contact(givenName: "X", photoData: payload)
        let out = writer.write(c)
        #expect(out.contains("PHOTO;ENCODING=b;TYPE=JPEG:"))
        #expect(out.contains(payload.base64EncodedString(options: .lineLength76Characters)))
    }

    // MARK: - Unknown fields preserved

    @Test("unknownFields are written verbatim")
    func unknownFieldsRoundTrip() throws {
        let c = Contact(givenName: "X", unknownFields: ["IMPP:xmpp:foo@bar"])
        let out = writer.write(c)
        #expect(out.contains("IMPP:xmpp:foo@bar"))
        let parsed = try #require(parser.parse(string: out, fileName: "x.vcf"))
        #expect(parsed.unknownFields.contains { $0.contains("IMPP:") })
    }

    // MARK: - Filename suggestion

    @Test("suggests lowercased given-family with .vcf")
    func filenameBasic() {
        let c = Contact(familyName: "Doe", givenName: "John")
        #expect(writer.suggestedFileName(for: c) == "john-doe.vcf")
    }

    @Test("strips non [a-z0-9-] from suggested name")
    func filenameSanitization() {
        let c = Contact(familyName: "Müller", givenName: "Anna")
        // 'ü' is stripped; "Anna-Mller" lowercased.
        #expect(writer.suggestedFileName(for: c) == "anna-mller.vcf")
    }

    @Test("falls back to <localContactsID>.vcf when no name parts")
    func filenameFallback() {
        let c = Contact(localContactsID: "lcid-xyz")
        #expect(writer.suggestedFileName(for: c) == "lcid-xyz.vcf")
    }

    @Test("a single punctuation-only name falls back to localContactsID")
    func filenamePunctuationFallback() {
        // Only one name part is set, and it has no [a-z0-9-] characters,
        // so sanitization produces an empty string and the writer falls
        // back to the local contact ID. (When both given and family are
        // punctuation, the joiner's `-` survives and the result is `-.vcf`.)
        let c = Contact(localContactsID: "fallback", familyName: "!!!")
        #expect(writer.suggestedFileName(for: c) == "fallback.vcf")
    }

    // MARK: - End-to-end round trip

    @Test("fully-populated contact survives write → parse")
    func endToEndRoundTrip() throws {
        let original = Contact(
            localContactsID: "rt-1",
            fileName: "rt.vcf",
            fullName: "Alice Wonder",
            familyName: "Wonder",
            givenName: "Alice",
            organization: "Acme",
            jobTitle: "CTO",
            nickname: "Ali",
            urls: [LabeledValue(label: "homepage", value: "https://alice.example")],
            phoneNumbers: [
                LabeledValue(label: "mobile", value: "+15551234567"),
                LabeledValue(label: "work", value: "+15557654321"),
            ],
            emailAddresses: [
                LabeledValue(label: "home", value: "alice@example.com"),
            ],
            postalAddresses: [
                LabeledValue(label: "home", value: PostalAddress(
                    street: "1 Main St", city: "Springfield",
                    state: "IL", postalCode: "62701", country: "USA"
                )),
            ],
            birthday: DateComponents(year: 1985, month: 3, day: 14),
            note: "friend\nfrom college",
            categories: ["friends", "college"]
        )

        let out = writer.write(original)
        let parsed = try #require(parser.parse(string: out, fileName: "rt.vcf"))

        #expect(parsed.localContactsID == original.localContactsID)
        #expect(parsed.fullName == original.fullName)
        #expect(parsed.familyName == original.familyName)
        #expect(parsed.givenName == original.givenName)
        #expect(parsed.organization == original.organization)
        #expect(parsed.jobTitle == original.jobTitle)
        #expect(parsed.nickname == original.nickname)
        #expect(parsed.urls.map(\.value) == original.urls.map(\.value))
        #expect(parsed.phoneNumbers.map(\.value) == original.phoneNumbers.map(\.value))
        #expect(parsed.phoneNumbers.map(\.label) == original.phoneNumbers.map(\.label))
        #expect(parsed.emailAddresses.map(\.value) == original.emailAddresses.map(\.value))
        #expect(parsed.postalAddresses.first?.value.street == "1 Main St")
        #expect(parsed.postalAddresses.first?.value.country == "USA")
        #expect(parsed.birthday?.year == 1985)
        #expect(parsed.birthday?.month == 3)
        #expect(parsed.birthday?.day == 14)
        #expect(parsed.note == "friend\nfrom college")
        #expect(parsed.categories == ["friends", "college"])
    }

    // MARK: - displayName fallback for FN

    @Test("FN falls back to displayName when fullName is empty")
    func fnFallback() throws {
        let c = Contact(familyName: "Wonder", givenName: "Alice")
        let out = writer.write(c)
        #expect(out.contains("FN:Alice Wonder"))
    }
}
