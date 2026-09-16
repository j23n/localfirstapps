//! Preservation contract for typed edits, tag actions, and conflict resolution.

use contacts_core::{
    assign_tag_logged, bulk_delete_logged, list_rows_filtered, load_edit_draft, parse, read_ops,
    remove_tag_logged, rename_tag_logged, resolve_logged, save_contact_logged, tag_rows, write,
    Card, ContactEditDraft, MemVfs, SaveContactCommand, Store, StoreError, Vfs,
    TYPE_CONTACT_DELETED, TYPE_CONTACT_SAVED, TYPE_GROUP_RESOLVED,
};

const ORIGINAL: &[u8] = include_bytes!("../fixtures/preservation/pat.vcf");
const COPY: &[u8] =
    include_bytes!("../fixtures/preservation/pat.sync-conflict-20200901-120000-LAPTOP02.vcf");

fn open_original() -> (MemVfs, Store) {
    let vfs = MemVfs::new();
    vfs.insert("/lib/pat.vcf", ORIGINAL.to_vec());
    let store = Store::open(&vfs, "/lib").unwrap();
    (vfs, store)
}

fn assert_preservation_fields(card: &Card) {
    assert_eq!(
        card.photo.as_deref(),
        Some([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a].as_slice())
    );
    assert_eq!(card.photo_media_type.as_deref(), Some("PNG"));
    assert_eq!(card.addresses.len(), 3);
    assert_eq!(card.addresses[0].value.street, "12; Main St");
    assert_eq!(
        card.addresses
            .iter()
            .filter(|address| address.label == "home")
            .count(),
        2
    );
    assert_eq!(
        card.phones
            .iter()
            .filter(|phone| phone.label == "home")
            .count(),
        2
    );
    assert!(card.phones.len() >= 3);
    assert_eq!(
        card.emails
            .iter()
            .filter(|email| email.label == "home")
            .count(),
        2
    );
    assert!(card.emails.len() >= 3);
    assert!(card
        .unknown_fields
        .contains(&"X-LOCAL-UNKNOWN;VALUE=text:preserve-me".to_owned()));
    assert!(card
        .unknown_fields
        .contains(&"item1.X-ABLabel:Custom Apple label".to_owned()));
    assert_eq!(
        card.unknown_fields
            .iter()
            .filter(|field| field.as_str() == "X-DUPLICATE:keep-two")
            .count(),
        2
    );
}

#[test]
fn typed_edit_save_and_reopen_preserve_unedited_card_content() {
    let (vfs, mut store) = open_original();
    let mut draft = load_edit_draft(&vfs, &mut store, "pat-1").unwrap();
    let original_token = draft.content_token.clone().unwrap();
    draft.job_title = "Senior Curator".into();
    draft.note = "Edited in the typed draft".into();
    draft.phones[0].value = "101".into();

    let saved = save_contact_logged(
        &vfs,
        &mut store,
        "test-device",
        SaveContactCommand { draft },
    )
    .unwrap();

    assert_ne!(
        saved.content_token.as_deref(),
        Some(original_token.as_str())
    );
    let reopened = Store::open(&vfs, "/lib").unwrap();
    let card = reopened.get("pat-1").unwrap();
    assert_eq!(card.job_title, "Senior Curator");
    assert_eq!(card.note, "Edited in the typed draft");
    assert_eq!(card.phones[0].value, "101");
    assert_preservation_fields(card);
    assert!(write(card).contains("PHOTO;ENCODING=b;TYPE=PNG:"));
    assert_eq!(
        read_ops(&vfs, "/lib").unwrap()[0].event_type,
        TYPE_CONTACT_SAVED
    );
}

#[test]
fn stale_edit_is_refused_then_reopen_and_save_preserves_external_changes() {
    let (vfs, mut store) = open_original();
    let mut stale = load_edit_draft(&vfs, &mut store, "pat-1").unwrap();
    stale.note = "stale note".into();

    let mut external = parse(ORIGINAL, "pat.vcf", false).unwrap();
    external.organization = "Externally changed".into();
    external.unknown_fields.push("X-EXTERNAL:keep-me".into());
    vfs.insert("/lib/pat.vcf", write(&external).into_bytes());

    let error = save_contact_logged(
        &vfs,
        &mut store,
        "test-device",
        SaveContactCommand { draft: stale },
    )
    .unwrap_err();
    assert!(matches!(error, StoreError::StaleEdit { .. }));
    let disk = parse(&vfs.read("/lib/pat.vcf").unwrap(), "pat.vcf", false).unwrap();
    assert_eq!(disk.organization, "Externally changed");
    assert_eq!(disk.note, "Original note");

    let mut reopened = load_edit_draft(&vfs, &mut store, "pat-1").unwrap();
    reopened.note = "saved after reopen".into();
    save_contact_logged(
        &vfs,
        &mut store,
        "test-device",
        SaveContactCommand { draft: reopened },
    )
    .unwrap();
    let disk = Store::open(&vfs, "/lib").unwrap();
    let card = disk.get("pat-1").unwrap();
    assert_eq!(card.organization, "Externally changed");
    assert_eq!(card.note, "saved after reopen");
    assert!(card
        .unknown_fields
        .contains(&"X-EXTERNAL:keep-me".to_owned()));
    assert_preservation_fields(card);
}

#[test]
fn merge_and_conflict_resolution_preserve_repeated_and_unknown_fields() {
    let vfs = MemVfs::new();
    vfs.insert("/lib/pat.vcf", ORIGINAL.to_vec());
    vfs.insert(
        "/lib/pat.sync-conflict-20200901-120000-LAPTOP02.vcf",
        COPY.to_vec(),
    );
    let mut store = Store::open(&vfs, "/lib").unwrap();
    let group = &store.conflict_groups()[0];
    let plan = contacts_core::plan_merge(&vfs, "/lib", group).unwrap();
    assert!(plan
        .conflicts
        .iter()
        .any(|conflict| conflict.field == "tel:home:1"));
    assert_eq!(plan.merged.addresses.len(), 3);
    assert_eq!(plan.merged.photo_media_type.as_deref(), Some("PNG"));
    assert!(plan
        .merged
        .unknown_fields
        .contains(&"X-COPY-ONLY:merge-me".to_owned()));
    assert_eq!(
        plan.merged
            .unknown_fields
            .iter()
            .filter(|field| field.as_str() == "X-DUPLICATE:keep-two")
            .count(),
        2
    );

    resolve_logged(
        &vfs,
        &mut store,
        "test-device",
        "pat.vcf",
        &["tel:home:1|pat.sync-conflict-20200901-120000-LAPTOP02.vcf".into()],
    )
    .unwrap();

    assert!(store.conflict_groups().is_empty());
    let card = store.get("pat-1").unwrap();
    assert_eq!(card.phones[1].value, "999");
    assert!(card.phones.iter().any(|phone| phone.value == "444"));
    assert!(card
        .emails
        .iter()
        .any(|email| email.value == "pat@other.example"));
    assert!(card
        .unknown_fields
        .contains(&"X-COPY-ONLY:merge-me".to_owned()));
    assert_preservation_fields(card);
    assert!(read_ops(&vfs, "/lib")
        .unwrap()
        .iter()
        .any(|operation| operation.event_type == TYPE_GROUP_RESOLVED));
}

#[test]
fn tag_filter_assign_rename_remove_and_bulk_delete_use_logged_actions() {
    let (vfs, mut store) = open_original();
    let other = ContactEditDraft {
        given_name: "Other".into(),
        categories: vec!["friends".into()],
        ..ContactEditDraft::default()
    };
    let other = save_contact_logged(
        &vfs,
        &mut store,
        "test-device",
        SaveContactCommand { draft: other },
    )
    .unwrap();
    let other_id = other.id.unwrap();

    assert_eq!(
        assign_tag_logged(
            &vfs,
            &mut store,
            "test-device",
            "vip",
            &["pat-1".into(), other_id.clone()],
        )
        .unwrap(),
        2
    );
    assert_eq!(list_rows_filtered(&store, "", Some("vip")).len(), 2);
    assert_eq!(
        tag_rows(&store)
            .into_iter()
            .find(|row| row.id == "vip")
            .unwrap()
            .trailing
            .as_deref(),
        Some("2 contacts")
    );

    assert_eq!(
        rename_tag_logged(&vfs, &mut store, "test-device", "friends", "vip",).unwrap(),
        2
    );
    assert_eq!(
        store
            .get("pat-1")
            .unwrap()
            .categories
            .iter()
            .filter(|tag| tag.as_str() == "vip")
            .count(),
        1
    );
    assert_eq!(
        remove_tag_logged(&vfs, &mut store, "test-device", "vip").unwrap(),
        2
    );
    assert_preservation_fields(store.get("pat-1").unwrap());

    assert_eq!(
        bulk_delete_logged(
            &vfs,
            &mut store,
            "test-device",
            std::slice::from_ref(&other_id),
        )
        .unwrap(),
        1
    );
    let operations = read_ops(&vfs, "/lib").unwrap();
    assert!(
        operations
            .iter()
            .filter(|operation| operation.event_type == TYPE_CONTACT_SAVED)
            .count()
            >= 7
    );
    assert!(operations.iter().any(|operation| {
        operation.event_type == TYPE_CONTACT_DELETED
            && operation.body["id"].as_str() == Some(other_id.as_str())
    }));
}
