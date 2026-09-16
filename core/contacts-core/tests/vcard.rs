//! Parse / write vectors for the contacts-core vCard layer.

use contacts_core::{parse, parse_multiple, suggested_file_name, write, Card, Labeled};

fn card(text: &str) -> Card {
    parse(text.as_bytes(), "x.vcf", false).expect("parse")
}

#[test]
fn minimal_fn() {
    let c = card("BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Alice\r\nEND:VCARD\r\n");
    assert_eq!(c.full_name, "Alice");
    assert_eq!(c.file_name, "x.vcf");
}

#[test]
fn missing_begin_is_none() {
    assert!(parse(b"FN:Alice\r\nEND:VCARD\r\n", "x.vcf", true).is_none());
}

#[test]
fn non_utf8_is_none() {
    assert!(parse(&[0xC3, 0x28], "x.vcf", true).is_none());
}

#[test]
fn name_components() {
    let c = card(
        "BEGIN:VCARD\r\nVERSION:3.0\r\nN:Smith;John;Quincy;Dr.;Jr.\r\nFN:Dr. John Quincy Smith Jr.\r\nEND:VCARD\r\n",
    );
    assert_eq!(c.family_name, "Smith");
    assert_eq!(c.given_name, "John");
    assert_eq!(c.middle_name, "Quincy");
    assert_eq!(c.name_prefix, "Dr.");
    assert_eq!(c.name_suffix, "Jr.");
}

#[test]
fn tel_type_filtering() {
    let c = card("BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nTEL;TYPE=CELL,VOICE,PREF:+15551234567\r\nEND:VCARD\r\n");
    assert_eq!(c.phones[0].label, "cell");
    assert_eq!(c.phones[0].value, "+15551234567");
}

#[test]
fn tel_bare_type() {
    let c = card("BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nTEL;HOME:555-1212\r\nEND:VCARD\r\n");
    assert_eq!(c.phones[0].label, "home");
}

#[test]
fn email_skips_internet() {
    let c = card(
        "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nEMAIL;TYPE=INTERNET,WORK:a@b.com\r\nEND:VCARD\r\n",
    );
    assert_eq!(c.emails[0].label, "work");
}

#[test]
fn defaults() {
    let c = card("BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nTEL:555\r\nEMAIL:a@b.com\r\nURL:https://e.com\r\nEND:VCARD\r\n");
    assert_eq!(c.phones[0].label, "mobile");
    assert_eq!(c.emails[0].label, "home");
    assert_eq!(c.urls[0].label, "homepage");
}

#[test]
fn group_prefix() {
    let c =
        card("BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nitem1.TEL;TYPE=cell:555-1234\r\nEND:VCARD\r\n");
    assert_eq!(c.phones[0].value, "555-1234");
}

#[test]
fn apple_grouped_url_uses_x_ablabel() {
    let c = card(
        "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Ajay\r\nitem1.URL:http://www.facebook.com/profile.php?id=1\r\nitem1.X-ABLabel:Facebook\r\nEND:VCARD\r\n",
    );
    assert_eq!(c.urls.len(), 1);
    assert_eq!(c.urls[0].label, "Facebook");
    assert!(c.note.is_empty());
}

#[test]
fn item_dump_in_note_becomes_a_url() {
    let c = card(
        "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Ajay\r\nNOTE:ITEM1.URL:http://www.facebook.com/a\\nITEM1.X-ABLABEL:Facebook\r\nEND:VCARD\r\n",
    );
    assert_eq!(c.urls.len(), 1);
    assert_eq!(c.urls[0].label, "Facebook");
    assert!(c.note.is_empty());
}

#[test]
fn not_a_group_prefix() {
    let c = card("BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nTEL;TYPE=home.work:555\r\nEND:VCARD\r\n");
    assert_eq!(c.phones.len(), 1);
}

#[test]
fn address_fields() {
    let c = card(
        "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nADR;TYPE=home:;;123 Main St;Springfield;IL;62701;USA\r\nEND:VCARD\r\n",
    );
    let a = &c.addresses[0].value;
    assert_eq!(a.street, "123 Main St");
    assert_eq!(a.city, "Springfield");
    assert_eq!(a.country, "USA");
}

#[test]
fn bday_formats() {
    let a = card("BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nBDAY:19850314\r\nEND:VCARD\r\n");
    let b = a.birthday.unwrap();
    assert_eq!((b.year, b.month, b.day), (Some(1985), 3, 14));
    let c = card("BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nBDAY:--03-14\r\nEND:VCARD\r\n");
    assert_eq!(c.birthday.unwrap().year, None);
    let d = card("BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nBDAY:not-a-date\r\nEND:VCARD\r\n");
    assert!(d.birthday.is_none());
}

#[test]
fn photo_base64_and_uri() {
    let payload = [0xFFu8, 0xD8, 0xFF, 0xE0];
    let b64 = "/9j/4A==";
    let c = card(&format!(
        "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nPHOTO;ENCODING=b;TYPE=JPEG:{b64}\r\nEND:VCARD\r\n"
    ));
    assert_eq!(c.photo.as_deref(), Some(payload.as_slice()));
    let u = card("BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nPHOTO;VALUE=URI:https://example.com/p.jpg\r\nEND:VCARD\r\n");
    assert!(u.photo.is_none());
    assert!(u.unknown_fields.iter().any(|l| l.contains("PHOTO")));
}

#[test]
fn categories_and_id() {
    let c = card("BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nCATEGORIES:friends, family ,work\r\nX-LOCALCONTACTS-ID:abc-123\r\nEND:VCARD\r\n");
    assert_eq!(c.categories, ["friends", "family", "work"]);
    assert_eq!(c.local_id, "abc-123");
}

#[test]
fn assign_default_id() {
    let a = parse(
        b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nEND:VCARD\r\n",
        "x.vcf",
        true,
    )
    .unwrap();
    assert!(!a.local_id.is_empty());
    let b = parse(
        b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nEND:VCARD\r\n",
        "x.vcf",
        false,
    )
    .unwrap();
    assert!(b.local_id.is_empty());
}

#[test]
fn unescape_and_fold() {
    let c = card("BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Smith\\, John\r\nNOTE:line1\\nline2\\Nline3\r\nEND:VCARD\r\n");
    assert_eq!(c.full_name, "Smith, John");
    assert_eq!(c.note, "line1\nline2\nline3");
    let b = card("BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nNOTE:a\\\\n\r\nEND:VCARD\r\n");
    assert_eq!(b.note, "a\\n");
    let folded = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:X\r\nNOTE:abcd\r\n efgh\r\nEND:VCARD\r\n";
    assert_eq!(card(folded).note, "abcdefgh");
}

#[test]
fn parse_multiple_splits() {
    let data = b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Alice\r\nEND:VCARD\r\nBEGIN:VCARD\r\nVERSION:3.0\r\nFN:Bob\r\nEND:VCARD\r\n";
    let cards = parse_multiple(data, "m.vcf", false);
    assert_eq!(
        cards
            .iter()
            .map(|c| c.full_name.as_str())
            .collect::<Vec<_>>(),
        ["Alice", "Bob"]
    );
}

#[test]
fn writer_headers_and_round_trip() {
    let mut c = Card::new("rt.vcf");
    c.local_id = "rt-1".into();
    c.full_name = "Alice Wonder".into();
    c.family_name = "Wonder".into();
    c.given_name = "Alice".into();
    c.organization = "Acme".into();
    c.note = "friend\nfrom college".into();
    c.phones.push(Labeled {
        label: "mobile".into(),
        value: "+15551234567".into(),
    });
    c.emails.push(Labeled {
        label: "home".into(),
        value: "alice@example.com".into(),
    });
    let out = write(&c);
    assert!(out.starts_with("BEGIN:VCARD\r\n"));
    assert!(out.contains("VERSION:3.0\r\n"));
    assert!(out.contains("X-LOCALCONTACTS-ID:rt-1\r\n"));
    assert!(out.ends_with("END:VCARD\r\n"));
    let parsed = parse(out.as_bytes(), "rt.vcf", false).unwrap();
    assert_eq!(parsed.full_name, "Alice Wonder");
    assert_eq!(parsed.note, "friend\nfrom college");
    assert_eq!(parsed.phones[0].value, "+15551234567");
}

#[test]
fn suggested_names() {
    let mut c = Card::new("x.vcf");
    c.given_name = "John".into();
    c.family_name = "Doe".into();
    assert_eq!(suggested_file_name(&c), "john-doe.vcf");
    c.given_name = "Anna".into();
    c.family_name = "Müller".into();
    assert_eq!(suggested_file_name(&c), "anna-mller.vcf");
    let mut empty = Card::new("x.vcf");
    empty.local_id = "lcid-xyz".into();
    assert_eq!(suggested_file_name(&empty), "lcid-xyz.vcf");
}

#[test]
fn type_sanitized_against_injection() {
    let mut c = Card::new("x.vcf");
    c.given_name = "X".into();
    c.local_id = "1".into();
    c.phones.push(Labeled {
        label: "cell:\nEMAIL:evil@x.com".into(),
        value: "555".into(),
    });
    let out = write(&c);
    assert!(!out.contains("EMAIL:evil@x.com"));
}
