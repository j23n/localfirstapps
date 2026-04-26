import Foundation
import Testing
@testable import LocalContacts

@Suite("VCardParser")
struct VCardParserTests {
    let parser = VCardParser()

    // MARK: - Basic structure

    @Test("parses minimal vCard with FN")
    func minimal() throws {
        let vcard = """
        BEGIN:VCARD\r
        VERSION:3.0\r
        FN:Alice\r
        END:VCARD\r
        """
        let contact = try #require(parser.parse(string: vcard, fileName: "alice.vcf"))
        #expect(contact.fullName == "Alice")
        #expect(contact.fileName == "alice.vcf")
    }

    @Test("returns nil when BEGIN:VCARD is absent")
    func missingBeginIsNil() {
        let bad = "FN:Alice\r\nEND:VCARD\r\n"
        #expect(parser.parse(string: bad, fileName: "x.vcf") == nil)
    }

    @Test("returns nil for non-UTF8 data")
    func nonUTF8Data() {
        // Lone continuation byte = invalid UTF-8
        let bad = Data([0xC3, 0x28])
        #expect(parser.parse(data: bad, fileName: "x.vcf") == nil)
    }

    @Test("BEGIN/END/VERSION lines are ignored as fields")
    func headersIgnored() throws {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Bob\r\nEND:VCARD\r\n"
        let contact = try #require(parser.parse(string: vcard, fileName: "b.vcf"))
        #expect(contact.unknownFields.isEmpty)
    }

    // MARK: - Name fields

    @Test("parses N into family/given/middle/prefix/suffix")
    func nameComponents() throws {
        let vcard = """
        BEGIN:VCARD\r
        VERSION:3.0\r
        N:Smith;John;Quincy;Dr.;Jr.\r
        FN:Dr. John Quincy Smith Jr.\r
        END:VCARD\r
        """
        let c = try #require(parser.parse(string: vcard, fileName: "n.vcf"))
        #expect(c.familyName == "Smith")
        #expect(c.givenName == "John")
        #expect(c.middleName == "Quincy")
        #expect(c.namePrefix == "Dr.")
        #expect(c.nameSuffix == "Jr.")
    }

    @Test("missing N components default to empty")
    func nameMissingComponents() throws {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nN:Smith\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "n.vcf"))
        #expect(c.familyName == "Smith")
        #expect(c.givenName.isEmpty)
        #expect(c.middleName.isEmpty)
    }

    // MARK: - Type label extraction

    @Test("TEL TYPE=CELL,VOICE,PREF picks cell, skips voice/pref")
    func telTypeFiltering() throws {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nTEL;TYPE=CELL,VOICE,PREF:+15551234567\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "t.vcf"))
        #expect(c.phoneNumbers.count == 1)
        #expect(c.phoneNumbers.first?.label == "cell")
        #expect(c.phoneNumbers.first?.value == "+15551234567")
    }

    @Test("vCard 2.1 bare TEL;HOME parses with home label")
    func telBareType() throws {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nTEL;HOME:555-1212\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "t.vcf"))
        #expect(c.phoneNumbers.first?.label == "home")
    }

    @Test("EMAIL TYPE=INTERNET,WORK skips internet, picks work")
    func emailSkipsInternet() throws {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nEMAIL;TYPE=INTERNET,WORK:a@b.com\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "e.vcf"))
        #expect(c.emailAddresses.first?.label == "work")
    }

    @Test("TEL with no TYPE defaults to mobile")
    func telDefaultLabel() throws {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nTEL:555-0000\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "t.vcf"))
        #expect(c.phoneNumbers.first?.label == "mobile")
    }

    @Test("URL with no TYPE defaults to homepage")
    func urlDefaultLabel() throws {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nURL:https://example.com\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "u.vcf"))
        #expect(c.urls.first?.label == "homepage")
        #expect(c.urls.first?.value == "https://example.com")
    }

    @Test("EMAIL with no TYPE defaults to home")
    func emailDefaultLabel() throws {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nEMAIL:a@b.com\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "e.vcf"))
        #expect(c.emailAddresses.first?.label == "home")
    }

    // MARK: - Group prefix

    @Test("group-prefixed line (item1.TEL) parses as TEL")
    func groupPrefix() throws {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nitem1.TEL;TYPE=cell:555-1234\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "g.vcf"))
        #expect(c.phoneNumbers.first?.value == "555-1234")
        #expect(c.phoneNumbers.first?.label == "cell")
    }

    @Test("semicolon-before-dot is not treated as group prefix")
    func notAGroupPrefix() throws {
        // The dot here lives in the parameter section (after a ;), so the parser
        // must NOT strip everything before it as a group prefix.
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nTEL;TYPE=home.work:555\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "g.vcf"))
        // Field is still TEL — group-prefix detection should not have fired.
        #expect(c.phoneNumbers.count == 1)
    }

    // MARK: - Addresses

    @Test("ADR maps fields 2..6 into street/city/state/zip/country")
    func addressFields() throws {
        let vcard = """
        BEGIN:VCARD\r
        VERSION:3.0\r
        FN:X\r
        ADR;TYPE=home:;;123 Main St;Springfield;IL;62701;USA\r
        END:VCARD\r
        """
        let c = try #require(parser.parse(string: vcard, fileName: "a.vcf"))
        let addr = try #require(c.postalAddresses.first)
        #expect(addr.label == "home")
        #expect(addr.value.street == "123 Main St")
        #expect(addr.value.city == "Springfield")
        #expect(addr.value.state == "IL")
        #expect(addr.value.postalCode == "62701")
        #expect(addr.value.country == "USA")
    }

    // MARK: - Birthday

    @Test("BDAY parses YYYYMMDD")
    func bdayBasic() throws {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nBDAY:19850314\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "b.vcf"))
        #expect(c.birthday?.year == 1985)
        #expect(c.birthday?.month == 3)
        #expect(c.birthday?.day == 14)
    }

    @Test("BDAY parses YYYY-MM-DD")
    func bdayDashed() throws {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nBDAY:1985-03-14\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "b.vcf"))
        #expect(c.birthday?.year == 1985)
        #expect(c.birthday?.month == 3)
    }

    @Test("BDAY parses --MM-DD with no year")
    func bdayNoYear() throws {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nBDAY:--03-14\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "b.vcf"))
        #expect(c.birthday?.year == nil)
        #expect(c.birthday?.month == 3)
        #expect(c.birthday?.day == 14)
    }

    @Test("malformed BDAY does not crash and returns nil")
    func bdayMalformed() throws {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nBDAY:not-a-date\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "b.vcf"))
        #expect(c.birthday == nil)
    }

    // MARK: - Photo

    @Test("base64 PHOTO decodes into photoData")
    func photoBase64() throws {
        let payload = Data([0xFF, 0xD8, 0xFF, 0xE0]) // JPEG magic bytes
        let b64 = payload.base64EncodedString()
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nPHOTO;ENCODING=b;TYPE=JPEG:\(b64)\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "p.vcf"))
        #expect(c.photoData == payload)
    }

    @Test("non-base64 PHOTO is preserved as unknown field")
    func photoURI() throws {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nPHOTO;VALUE=URI:https://example.com/p.jpg\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "p.vcf"))
        #expect(c.photoData == nil)
        #expect(c.unknownFields.contains { $0.contains("PHOTO") })
    }

    // MARK: - Categories, X-LOCALCONTACTS-ID

    @Test("CATEGORIES splits on commas and trims whitespace")
    func categories() throws {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nCATEGORIES:friends, family ,work\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "c.vcf"))
        #expect(c.categories == ["friends", "family", "work"])
    }

    @Test("X-LOCALCONTACTS-ID is preserved verbatim")
    func localContactsID() throws {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nX-LOCALCONTACTS-ID:abc-123\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "id.vcf"))
        #expect(c.localContactsID == "abc-123")
    }

    // MARK: - Unknown fields

    @Test("unrecognized fields are kept in unknownFields")
    func unknownFields() throws {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nIMPP:xmpp:foo@bar\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "u.vcf"))
        #expect(c.unknownFields.contains { $0.hasPrefix("IMPP:") })
    }

    // MARK: - Escaping

    @Test("escaped commas, semicolons, and newlines round-trip")
    func unescaping() throws {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Smith\\, John\r\nNOTE:line1\\nline2\\Nline3\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "esc.vcf"))
        #expect(c.fullName == "Smith, John")
        #expect(c.note == "line1\nline2\nline3")
    }

    @Test("escaped backslash unescapes to single backslash")
    func unescapeBackslash() throws {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nNOTE:a\\\\b\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: vcard, fileName: "esc.vcf"))
        #expect(c.note == "a\\b")
    }

    // MARK: - Line folding

    @Test("CRLF + space continuation reassembles to single line")
    func lineFoldingCRLF() throws {
        // Per RFC 6350, the leading space on a folded line is the fold
        // marker — it is consumed, so the two lines concatenate.
        let folded = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nNOTE:abcd\r\n efgh\r\nEND:VCARD\r\n"
        let c = try #require(parser.parse(string: folded, fileName: "f.vcf"))
        #expect(c.note == "abcdefgh")
    }

    @Test("LF + tab continuation reassembles to single line")
    func lineFoldingLFTab() throws {
        let folded = "BEGIN:VCARD\nVERSION:3.0\nFN:X\nNOTE:abcd\n\tefgh\nEND:VCARD\n"
        let c = try #require(parser.parse(string: folded, fileName: "f.vcf"))
        #expect(c.note == "abcdefgh")
    }

    // MARK: - Multi-vCard

    @Test("parseMultiple returns all contacts in order")
    func parseMultipleSplits() {
        let vcard = """
        BEGIN:VCARD\r
        VERSION:3.0\r
        FN:Alice\r
        END:VCARD\r
        BEGIN:VCARD\r
        VERSION:3.0\r
        FN:Bob\r
        END:VCARD\r
        BEGIN:VCARD\r
        VERSION:3.0\r
        FN:Carol\r
        END:VCARD\r
        """
        let data = vcard.data(using: .utf8)!
        let contacts = parser.parseMultiple(data: data, fileName: "multi.vcf")
        #expect(contacts.count == 3)
        #expect(contacts.map(\.fullName) == ["Alice", "Bob", "Carol"])
    }

    @Test("parseMultiple handles blank lines between cards")
    func parseMultipleWithBlanks() {
        let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:A\r\nEND:VCARD\r\n\r\nBEGIN:VCARD\r\nVERSION:3.0\r\nFN:B\r\nEND:VCARD\r\n"
        let contacts = parser.parseMultiple(data: vcard.data(using: .utf8)!, fileName: "x.vcf")
        #expect(contacts.count == 2)
    }
}
