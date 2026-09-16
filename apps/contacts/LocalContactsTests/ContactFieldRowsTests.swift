import Testing
@testable import LocalContacts

@Suite("Contact detail field-row grouping")
struct ContactFieldRowsTests {
    @Test("hero fields are skipped")
    func skipsHero() {
        #expect(ContactDetailView.isHeroField(FieldRow(id: "fn", label: "Name", value: "Ada", editable: true)))
        #expect(ContactDetailView.isHeroField(FieldRow(id: "org", label: "Organization", value: "Acme", editable: true)))
        #expect(ContactDetailView.isHeroField(FieldRow(id: "category:0", label: "Tag", value: "work", editable: true)))
        #expect(!ContactDetailView.isHeroField(FieldRow(id: "tel:0", label: "Mobile", value: "1", editable: true)))
    }

    @Test("id prefixes become section titles")
    func sectionTitles() {
        #expect(ContactDetailView.sectionTitle(for: FieldRow(id: "tel:0", label: "Mobile", value: "1", editable: true)) == "Phone")
        #expect(ContactDetailView.sectionTitle(for: FieldRow(id: "email:0", label: "Home", value: "a@b", editable: true)) == "Email")
        #expect(ContactDetailView.sectionTitle(for: FieldRow(id: "url:0", label: "Work", value: "ex.com", editable: true)) == "Website")
        #expect(ContactDetailView.sectionTitle(for: FieldRow(id: "adr:0", label: "Home", value: "1 Main", editable: true)) == "Address")
        #expect(ContactDetailView.sectionTitle(for: FieldRow(id: "bday", label: "Birthday", value: "1990-01-01", editable: true)) == "Birthday")
        #expect(ContactDetailView.sectionTitle(for: FieldRow(id: "note", label: "Note", value: "hi", editable: true)) == "Notes")
    }

    @Test("sections keep Phone→Notes order and drop hero rows")
    func sectionOrder() {
        let rows = [
            FieldRow(id: "fn", label: "Name", value: "Ada", editable: true),
            FieldRow(id: "note", label: "Note", value: "hi", editable: true),
            FieldRow(id: "tel:0", label: "Mobile", value: "1", editable: true),
            FieldRow(id: "email:0", label: "Home", value: "a@b", editable: true),
        ]
        let titles = ContactDetailView.sections(from: rows).map(\.title)
        #expect(titles == ["Phone", "Email", "Notes"])
    }
}

@Suite("Contact edit draft")
struct ContactEditDraftViewTests {
    @Test("editor initials come from the typed draft")
    func initialsFromDraft() {
        let draft = ContactEditDraft(
            id: nil,
            contentToken: nil,
            fullName: "",
            familyName: "Lovelace",
            givenName: "Ada",
            middleName: "",
            namePrefix: "",
            nameSuffix: "",
            organization: "",
            jobTitle: "",
            nickname: "",
            urls: [],
            phones: [],
            emails: [],
            addresses: [],
            birthday: nil,
            note: "",
            categories: [],
            photo: nil
        )
        #expect(ContactEditView.initials(from: draft) == "AL")
        #expect(ContactEditView.structuredName(given: "Ada", middle: "", family: "Lovelace") == "Ada Lovelace")
    }

    @Test("search highlight marks the first case-insensitive match")
    func searchHighlight() {
        let marked = ContactCard.highlighted("a@b.com", query: "B.COM")
        let plain = String(marked.characters)
        #expect(plain == "a@b.com")
        #expect(ContactCard.highlighted("Ada", query: "").characters.count == 3)
    }
}
