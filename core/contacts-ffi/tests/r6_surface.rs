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
    let session = ContactsSession::open(root.into(), "test".into()).unwrap();
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
    let session = ContactsSession::open(root.into(), "test".into()).unwrap();
    let groups = session.conflict_rows().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].trailing.as_deref(), Some("auto"));
    session.resolve_group("alice.vcf".into(), vec![]).unwrap();
    let session = ContactsSession::open(root.into(), "test".into()).unwrap();
    assert!(session.conflict_rows().unwrap().is_empty());
    let fields = session.field_rows("a1".into()).unwrap();
    assert!(fields.iter().any(|r| r.value == "a@b.com"));
}

#[test]
fn save_and_delete_go_through_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_str().unwrap();
    let session = ContactsSession::open(root.into(), "test".into()).unwrap();
    let id = session
        .save_vcard(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nX-LOCALCONTACTS-ID:z1\r\nFN:Zed\r\nEND:VCARD\r\n".into(),
            String::new(),
        )
        .unwrap();
    assert_eq!(id, "z1");
    assert_eq!(session.list_rows().unwrap().len(), 1);
    assert!(session.vcard_text("z1".into()).unwrap().contains("Zed"));
    assert!(session.file_name("z1".into()).unwrap().ends_with(".vcf"));
    session.delete("z1".into()).unwrap();
    session.reload().unwrap();
    assert!(session.list_rows().unwrap().is_empty());
}

#[test]
fn save_existing_card_does_not_fork_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_str().unwrap();
    fs::write(
        dir.path().join("alice.vcf"),
        b"BEGIN:VCARD\r\nVERSION:3.0\r\nX-LOCALCONTACTS-ID:a1\r\nFN:Alice\r\nN:;Alice;;;\r\nEND:VCARD\r\n",
    )
    .unwrap();
    let session = ContactsSession::open(root.into(), "test".into()).unwrap();
    session
        .save_vcard(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nX-LOCALCONTACTS-ID:a1\r\nFN:Alicia\r\nN:;Alicia;;;\r\nEND:VCARD\r\n"
                .into(),
            String::new(),
        )
        .unwrap();
    assert!(dir.path().join("alice.vcf").exists());
    assert!(!dir.path().join("alicia.vcf").exists());
    assert!(session.vcard_text("a1".into()).unwrap().contains("Alicia"));
    assert_eq!(session.file_name("a1".into()).unwrap(), "alice.vcf");
}

#[test]
fn choice_group_needs_a_pick() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_str().unwrap();
    fs::write(
        dir.path().join("bob.vcf"),
        b"BEGIN:VCARD\r\nVERSION:3.0\r\nX-LOCALCONTACTS-ID:b1\r\nFN:Bob\r\nTEL;TYPE=cell:1\r\nEND:VCARD\r\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("bob.sync-conflict-20200901-120000-LAPTOP02.vcf"),
        b"BEGIN:VCARD\r\nVERSION:3.0\r\nX-LOCALCONTACTS-ID:b1\r\nFN:Bob\r\nTEL;TYPE=cell:2\r\nEND:VCARD\r\n",
    )
    .unwrap();
    let session = ContactsSession::open(root.into(), "test".into()).unwrap();
    let groups = session.conflict_rows().unwrap();
    assert_eq!(groups[0].trailing.as_deref(), Some("needs choice"));
    assert!(session.resolve_group("bob.vcf".into(), vec![]).is_err());
    let rows = session.conflict_choice_rows("bob.vcf".into()).unwrap();
    assert!(rows.iter().any(|r| r.id.starts_with("tel:cell|")));
    let pick = rows
        .iter()
        .find(|r| r.id.contains("LAPTOP02"))
        .unwrap()
        .id
        .clone();
    session.resolve_group("bob.vcf".into(), vec![pick]).unwrap();
    assert!(session.conflict_rows().unwrap().is_empty());
}

#[test]
fn save_appends_a_folder_log_event() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_str().unwrap();
    let session = ContactsSession::open(root.into(), "phone".into()).unwrap();
    session
        .save_vcard(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nX-LOCALCONTACTS-ID:z1\r\nFN:Zed\r\nEND:VCARD\r\n".into(),
            String::new(),
        )
        .unwrap();
    let log_dir = dir.path().join(".contacts/log/phone");
    let files: Vec<_> = std::fs::read_dir(&log_dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    let text = std::fs::read_to_string(&files[0]).unwrap();
    assert!(text.contains("\"type\":\"contact_saved\""));
    assert!(text.contains("\"id\":\"z1\""));
}
