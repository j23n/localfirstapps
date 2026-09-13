//! Walk, search, save, delete through MemVfs.

use contacts_core::{parse, write, Card, Store};
use localcore_vfs::{MemVfs, Vfs};

fn alice() -> String {
    let mut c = Card::new("alice.vcf");
    c.local_id = "alice-1".into();
    c.full_name = "Alice".into();
    c.given_name = "Alice".into();
    write(&c)
}

fn bob() -> String {
    let mut c = Card::new("bob.vcf");
    c.local_id = "bob-1".into();
    c.full_name = "Bob".into();
    c.given_name = "Bob".into();
    write(&c)
}

#[test]
fn walk_excludes_conflict_copies() {
    let vfs = MemVfs::new();
    vfs.insert("/lib/alice.vcf", alice().into_bytes());
    vfs.insert(
        "/lib/alice.sync-conflict-20200901-120000-PHONE01.vcf",
        b"BEGIN:VCARD\nVERSION:3.0\nFN:Alice Phone\nEND:VCARD\n".to_vec(),
    );
    vfs.insert(
        "/lib/alice.sync-conflict-20200901-140000-LAPTOP02.vcf",
        b"BEGIN:VCARD\nVERSION:3.0\nFN:Alice Laptop\nEND:VCARD\n".to_vec(),
    );
    let store = Store::open(&vfs, "/lib").unwrap();
    assert_eq!(store.cards().len(), 1);
    assert_eq!(store.cards()[0].full_name, "Alice");
    assert_eq!(store.conflict_groups().len(), 1);
    assert_eq!(store.conflict_groups()[0].canonical_name, "alice.vcf");
    assert_eq!(store.conflict_groups()[0].copies.len(), 2);
}

#[test]
fn search_and_layout() {
    let vfs = MemVfs::new();
    vfs.insert("/lib/alice.vcf", alice().into_bytes());
    vfs.insert("/lib/bob.vcf", bob().into_bytes());
    let store = Store::open(&vfs, "/lib").unwrap();
    assert!(matches!(
        store.layout(),
        contacts_core::Layout::OneFilePerContact
    ));
    let hits = store.search("ali");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].local_id, "alice-1");
}

#[test]
fn save_assigns_name_and_preserves_siblings() {
    let vfs = MemVfs::new();
    let mut shared = Card::new("people.vcf");
    shared.local_id = "a-1".into();
    shared.full_name = "Ann".into();
    let mut other = Card::new("people.vcf");
    other.local_id = "b-1".into();
    other.full_name = "Ben".into();
    vfs.insert(
        "/lib/people.vcf",
        format!("{}{}", write(&shared), write(&other)).into_bytes(),
    );
    let mut store = Store::open(&vfs, "/lib").unwrap();
    let mut ann = store.get("a-1").unwrap().clone();
    ann.note = "updated".into();
    store.save(&vfs, ann).unwrap();
    let disk = parse(&vfs.read("/lib/people.vcf").unwrap(), "people.vcf", false).unwrap();
    assert_eq!(disk.note, "updated");
    let cards =
        contacts_core::parse_multiple(&vfs.read("/lib/people.vcf").unwrap(), "people.vcf", false);
    assert_eq!(cards.len(), 2);
    assert!(cards
        .iter()
        .any(|c| c.local_id == "b-1" && c.full_name == "Ben"));
}

#[test]
fn save_without_file_name_updates_the_existing_card() {
    let vfs = MemVfs::new();
    vfs.insert("/lib/alice.vcf", alice().into_bytes());
    let mut store = Store::open(&vfs, "/lib").unwrap();
    let mut card = Card::new("");
    card.local_id = "alice-1".into();
    card.full_name = "Alicia".into();
    card.given_name = "Alicia".into();
    store.save(&vfs, card).unwrap();
    assert!(vfs.exists("/lib/alice.vcf"));
    assert!(!vfs.exists("/lib/alicia.vcf"));
    let disk = parse(&vfs.read("/lib/alice.vcf").unwrap(), "alice.vcf", false).unwrap();
    assert_eq!(disk.full_name, "Alicia");
    assert_eq!(store.cards().len(), 1);
}

#[test]
fn delete_last_card_removes_file() {
    let vfs = MemVfs::new();
    vfs.insert("/lib/alice.vcf", alice().into_bytes());
    let mut store = Store::open(&vfs, "/lib").unwrap();
    store.delete(&vfs, "alice-1").unwrap();
    assert!(!vfs.exists("/lib/alice.vcf"));
    assert!(store.cards().is_empty());
}

#[test]
fn missing_id_is_assigned_and_persisted() {
    let vfs = MemVfs::new();
    vfs.insert(
        "/lib/no-id.vcf",
        b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Zed\r\nEND:VCARD\r\n".to_vec(),
    );
    let store = Store::open(&vfs, "/lib").unwrap();
    let card = &store.cards()[0];
    assert!(!card.local_id.is_empty());
    let disk = parse(&vfs.read("/lib/no-id.vcf").unwrap(), "no-id.vcf", false).unwrap();
    assert_eq!(disk.local_id, card.local_id);
}
