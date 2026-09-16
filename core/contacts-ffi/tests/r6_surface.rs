//! Headless FFI: display rows and command DTOs, never a Contact/Card record.

use contacts_ffi::{
    is_conflict_name, ContactsError, ContactsSession, LabeledValueDraft, MergeKind,
    SaveContactCommand,
};
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
    assert_eq!(groups[0].trailing, "auto");
    assert_eq!(groups[0].disposition, MergeKind::Auto);
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
    let mut draft = session.new_contact_draft();
    draft.given_name = "Zed".into();
    draft.full_name = "Zed".into();
    let saved = session.save_contact(SaveContactCommand { draft }).unwrap();
    let id = saved.id.unwrap();
    assert_eq!(session.list_rows().unwrap().len(), 1);
    assert!(session
        .export_vcard_text(id.clone())
        .unwrap()
        .contains("Zed"));
    assert!(session.file_name(id.clone()).unwrap().ends_with(".vcf"));
    session.delete(id).unwrap();
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
    let mut draft = session.contact_edit_draft("a1".into()).unwrap();
    draft.full_name = "Alicia".into();
    draft.given_name = "Alicia".into();
    session.save_contact(SaveContactCommand { draft }).unwrap();
    assert!(dir.path().join("alice.vcf").exists());
    assert!(!dir.path().join("alicia.vcf").exists());
    assert!(session
        .export_vcard_text("a1".into())
        .unwrap()
        .contains("Alicia"));
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
    assert_eq!(groups[0].trailing, "needs choice");
    assert_eq!(groups[0].disposition, MergeKind::Choice);
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
    let mut draft = session.new_contact_draft();
    draft.given_name = "Zed".into();
    let saved = session.save_contact(SaveContactCommand { draft }).unwrap();
    let id = saved.id.unwrap();
    let log_dir = dir.path().join(".contacts/log/phone");
    let files: Vec<_> = std::fs::read_dir(&log_dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    let text = std::fs::read_to_string(&files[0]).unwrap();
    assert!(text.contains("\"type\":\"contact_saved\""));
    assert!(text.contains(&format!("\"id\":\"{id}\"")));
}

#[test]
fn typed_draft_preserves_rich_fields_and_refuses_stale_save() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_str().unwrap();
    let original = include_bytes!("../../contacts-core/fixtures/preservation/pat.vcf");
    fs::write(dir.path().join("pat.vcf"), original).unwrap();
    let session = ContactsSession::open(root.into(), "phone".into()).unwrap();
    let mut draft = session.contact_edit_draft("pat-1".into()).unwrap();
    assert_eq!(draft.phones.len(), 3);
    assert_eq!(draft.emails.len(), 3);
    assert_eq!(draft.addresses.len(), 3);
    assert_eq!(
        draft.photo.as_deref(),
        Some([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a].as_slice())
    );
    draft.note = "typed save".into();
    session
        .save_contact(SaveContactCommand {
            draft: draft.clone(),
        })
        .unwrap();
    let reopened = session.contact_edit_draft("pat-1".into()).unwrap();
    assert_eq!(reopened.note, "typed save");
    assert_eq!(
        reopened
            .phones
            .iter()
            .filter(|phone| phone.label == "home")
            .count(),
        2
    );
    let exported = session.export_vcard_text("pat-1".into()).unwrap();
    assert!(exported.contains("X-LOCAL-UNKNOWN;VALUE=text:preserve-me"));
    assert!(exported.contains("PHOTO;ENCODING=b;TYPE=PNG:"));

    let externally_changed = exported.replace("ORG:Archive House", "ORG:External");
    fs::write(dir.path().join("pat.vcf"), externally_changed).unwrap();
    let error = session
        .save_contact(SaveContactCommand { draft })
        .unwrap_err();
    assert!(matches!(
        error,
        ContactsError::StaleEdit {
            user_actionable: true,
            ..
        }
    ));
}

#[test]
fn typed_tag_filter_and_bulk_actions_share_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_str().unwrap();
    let session = ContactsSession::open(root.into(), "phone".into()).unwrap();
    let mut first = session.new_contact_draft();
    first.given_name = "Ada".into();
    first.phones.push(LabeledValueDraft {
        label: "mobile".into(),
        value: "111".into(),
    });
    let first_id = session
        .save_contact(SaveContactCommand { draft: first })
        .unwrap()
        .id
        .unwrap();
    let mut second = session.new_contact_draft();
    second.given_name = "Grace".into();
    let second_id = session
        .save_contact(SaveContactCommand { draft: second })
        .unwrap()
        .id
        .unwrap();

    assert_eq!(
        session
            .assign_tag("pioneers".into(), vec![first_id.clone(), second_id.clone()])
            .unwrap(),
        2
    );
    assert_eq!(
        session
            .filtered_list_rows(String::new(), Some("pioneers".into()))
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        session.tag_rows().unwrap()[0].trailing.as_deref(),
        Some("2 contacts")
    );
    assert_eq!(
        session
            .rename_tag("pioneers".into(), "computing".into())
            .unwrap(),
        2
    );
    assert_eq!(session.remove_tag("computing".into()).unwrap(), 2);
    assert_eq!(session.delete_many(vec![second_id]).unwrap(), 1);
    assert_eq!(session.list_rows().unwrap()[0].id, first_id);
}
