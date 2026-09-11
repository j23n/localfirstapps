import Foundation
import Testing
@testable import LocalContacts

@Suite("ContactDetailView URL builders")
struct ContactDetailURLTests {

    // MARK: - dialURL

    @Test("dialURL keeps a plain numeric string")
    func dialURLPlain() {
        #expect(ContactDetailView.dialURL("5551234567") == URL(string: "tel:5551234567"))
    }

    @Test("dialURL strips spaces and parentheses instead of returning nil")
    func dialURLFormatted() {
        // "+1 (555) 123-4567" would make URL(string:) return nil and crash a
        // force-unwrap; the helper must filter it down to a dialable URL.
        #expect(ContactDetailView.dialURL("+1 (555) 123-4567") == URL(string: "tel:+15551234567"))
    }

    @Test("dialURL preserves DTMF separators")
    func dialURLSeparators() {
        #expect(ContactDetailView.dialURL("555-1234,*89#") == URL(string: "tel:5551234,*89#"))
    }

    @Test("dialURL returns nil when nothing dialable remains")
    func dialURLEmpty() {
        #expect(ContactDetailView.dialURL("call me") == nil)
        #expect(ContactDetailView.dialURL("") == nil)
    }

    // MARK: - mailURL

    @Test("mailURL builds a mailto for a normal address")
    func mailURLPlain() {
        #expect(ContactDetailView.mailURL("alice@example.com") == URL(string: "mailto:alice@example.com"))
    }

    @Test("mailURL trims surrounding whitespace")
    func mailURLTrims() {
        #expect(ContactDetailView.mailURL("  alice@example.com \n") == URL(string: "mailto:alice@example.com"))
    }

    @Test("mailURL returns nil for an empty address")
    func mailURLEmpty() {
        #expect(ContactDetailView.mailURL("   ") == nil)
    }

    @Test("mailURL percent-encodes a space in the address")
    func mailURLEncodesSpace() {
        let url = ContactDetailView.mailURL("alice smith@example.com")
        #expect(url?.absoluteString == "mailto:alice%20smith@example.com")
    }

    // MARK: - websiteURL

    @Test("websiteURL prepends https when no scheme")
    func websiteURLBareHost() {
        #expect(ContactDetailView.websiteURL("example.com") == URL(string: "https://example.com"))
    }

    @Test("websiteURL accepts HTTPS case-insensitively")
    func websiteURLHTTPS() {
        #expect(ContactDetailView.websiteURL("HTTPS://example.com") == URL(string: "HTTPS://example.com"))
    }

    @Test("websiteURL returns nil for empty input")
    func websiteURLEmpty() {
        #expect(ContactDetailView.websiteURL("   ") == nil)
    }
}
