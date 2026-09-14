//! Display rows and logged actions — the surface both shells call.

use contacts_core::{
    apply_draft, choice_rows, conflict_rows, delete_logged, draft_from_card, field_rows, list_rows,
    read_ops, save_logged, write, Card, ContactDraft, MemVfs, Store, TYPE_CONTACT_DELETED,
    TYPE_CONTACT_SAVED,
};

#[test]
fn list_and_fields_match_ffi_copy() {
    let vfs = MemVfs::new();
    let mut store = Store::open(&vfs, "/lib").unwrap();
    let mut card = Card::new("");
    apply_draft(
        &mut card,
        &ContactDraft {
            given: "Ada".into(),
            family: "Lovelace".into(),
            organization: "Analytical".into(),
            email: "ada@example".into(),
            ..ContactDraft::default()
        },
    );
    let saved = save_logged(&vfs, &mut store, "linux-test", card).unwrap();
    let rows = list_rows(&store, "ada");
    assert_eq!(rows[0].title, "Ada Lovelace");
    assert_eq!(rows[0].subtitle.as_deref(), Some("Analytical"));
    let fields = field_rows(store.get(&saved.local_id).unwrap());
    assert_eq!(fields[0].label, "Name");
    assert_eq!(fields[0].value, "Ada Lovelace");
    assert_eq!(
        read_ops(&vfs, "/lib").unwrap()[0].event_type,
        TYPE_CONTACT_SAVED
    );
}

#[test]
fn conflict_row_trailing_is_needs_choice() {
    let vfs = MemVfs::new();
    let mut alice = Card::new("alice.vcf");
    alice.local_id = "alice-1".into();
    alice.full_name = "Alice".into();
    vfs.insert("/lib/alice.vcf", write(&alice).into_bytes());
    vfs.insert(
        "/lib/alice.sync-conflict-20200901-120000-PHONE01.vcf",
        b"BEGIN:VCARD\nVERSION:3.0\nFN:Alice Phone\nEND:VCARD\n".to_vec(),
    );
    let store = Store::open(&vfs, "/lib").unwrap();
    let rows = conflict_rows(&vfs, &store).unwrap();
    assert_eq!(rows[0].trailing.as_deref(), Some("needs choice"));
    assert_eq!(rows[0].subtitle.as_deref(), Some("1 copies"));
    let choices = choice_rows(&vfs, &store, "alice.vcf").unwrap();
    assert!(choices.iter().any(|row| row.id.contains('|')));
}

#[test]
fn draft_round_trip_and_delete_log() {
    let vfs = MemVfs::new();
    let mut store = Store::open(&vfs, "/lib").unwrap();
    let mut card = Card::new("");
    apply_draft(
        &mut card,
        &ContactDraft {
            given: "Ada".into(),
            family: "Lovelace".into(),
            phone: "555".into(),
            ..ContactDraft::default()
        },
    );
    let saved = save_logged(&vfs, &mut store, "linux-test", card).unwrap();
    let draft = draft_from_card(store.get(&saved.local_id).unwrap());
    assert_eq!(draft.given, "Ada");
    assert_eq!(draft.family, "Lovelace");
    assert_eq!(draft.phone, "555");
    delete_logged(&vfs, &mut store, "linux-test", &saved.local_id).unwrap();
    assert!(store.get(&saved.local_id).is_none());
    assert!(read_ops(&vfs, "/lib")
        .unwrap()
        .iter()
        .any(|op| op.event_type == TYPE_CONTACT_DELETED));
}
