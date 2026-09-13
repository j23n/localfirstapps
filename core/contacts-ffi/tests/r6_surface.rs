//! Headless FFI: display rows only, no Contact record.

use contacts_ffi::{is_conflict_name, ContactsSession};
use std::fs;
use std::io::Write;

#[test]
fn conflict_name_matches_grammar() {
    assert!(is_conflict_name(
        "alice.sync-conflict-20200901-120000-PHONE01.vcf".into()
    ));
    assert!(!is_conflict_name("alice.vcf".into()));
}

#[test]
fn list_and_fields_are_display_rows() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_str().unwrap();
    let mut f = fs::File::create(dir.path().join("alice.vcf")).unwrap();
    f.write_all(
        b"BEGIN:VCARD\r\nVERSION:3.0\r\nX-LOCALCONTACTS-ID:a1\r\nFN:Alice\r\nN:;Alice;;;\r\nEMAIL;TYPE=home:a@b.com\r\nEND:VCARD\r\n",
    )
    .unwrap();
    let session = ContactsSession::open(root.into()).unwrap();
    let rows = session.list_rows().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "a1");
    assert_eq!(rows[0].title, "Alice");
    let fields = session.field_rows("a1".into()).unwrap();
    assert!(fields
        .iter()
        .any(|r| r.label == "Name" && r.value == "Alice"));
}

#[test]
fn resolve_auto_group() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_str().unwrap();
    fs::write(
        dir.path().join("alice.vcf"),
        b"BEGIN:VCARD\r\nVERSION:3.0\r\nX-LOCALCONTACTS-ID:a1\r\nFN:Alice\r\nTEL;TYPE=cell:1\r\nEND:VCARD\r\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("alice.sync-conflict-20200901-120000-PHONE01.vcf"),
        b"BEGIN:VCARD\r\nVERSION:3.0\r\nX-LOCALCONTACTS-ID:a1\r\nFN:Alice\r\nEMAIL;TYPE=home:a@b.com\r\nEND:VCARD\r\n",
    )
    .unwrap();
    let session = ContactsSession::open(root.into()).unwrap();
    let groups = session.conflict_rows().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].trailing.as_deref(), Some("auto"));
    session.resolve_group("alice.vcf".into()).unwrap();
    let session = ContactsSession::open(root.into()).unwrap();
    assert!(session.conflict_rows().unwrap().is_empty());
    let fields = session.field_rows("a1".into()).unwrap();
    assert!(fields.iter().any(|r| r.value == "a@b.com"));
}
