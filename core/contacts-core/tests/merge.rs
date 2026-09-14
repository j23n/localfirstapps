//! ADR 0005 R8–R11 against the committed fixture trees.

use contacts_core::{
    apply_merge, parse, plan_merge, write, Card, Labeled, MergeKind, Store, StoreError,
};
use localcore_vfs::{MemVfs, Vfs};

fn load_tree(vfs: &MemVfs, root: &str, fixture_dir: &str) {
    let dir = format!("{}/fixtures/{fixture_dir}", env!("CARGO_MANIFEST_DIR"));
    for entry in std::fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".vcf") {
            continue;
        }
        let bytes = std::fs::read(entry.path()).unwrap();
        vfs.insert(&format!("{root}/{name}"), bytes);
    }
}

#[test]
fn disjoint_fields_merge_automatically() {
    let vfs = MemVfs::new();
    load_tree(&vfs, "/lib", "r8/disjoint");
    let store = Store::open(&vfs, "/lib").unwrap();
    let group = &store.conflict_groups()[0];
    let plan = plan_merge(&vfs, "/lib", group).unwrap();
    assert_eq!(plan.kind, MergeKind::Auto);
    assert!(plan.conflicts.is_empty());
    assert_eq!(plan.merged.phones[0].value, "+15550001");
    assert_eq!(plan.merged.emails[0].value, "alice@example.com");

    let first = write(&plan.merged);
    let second = write(&plan.merged);
    assert_eq!(first, second, "R9: identical inputs, identical bytes");

    let card = apply_merge(&vfs, "/lib", &plan, &[]).unwrap();
    assert_eq!(card.emails[0].value, "alice@example.com");
    assert!(vfs.exists("/lib/alice.vcf"));
    assert!(!vfs.exists("/lib/alice.sync-conflict-20200901-120000-PHONE01.vcf"));
    let disk = parse(&vfs.read("/lib/alice.vcf").unwrap(), "alice.vcf", false).unwrap();
    assert_eq!(write(&disk), write(&card));
}

#[test]
fn same_field_is_a_choice_and_does_not_delete() {
    let vfs = MemVfs::new();
    load_tree(&vfs, "/lib", "r8/same-field");
    let store = Store::open(&vfs, "/lib").unwrap();
    let group = &store.conflict_groups()[0];
    let plan = plan_merge(&vfs, "/lib", group).unwrap();
    assert_eq!(plan.kind, MergeKind::Choice);
    assert!(plan.conflicts.iter().any(|c| c.field == "tel:cell"));
    assert!(vfs.exists("/lib/bob.sync-conflict-20200901-120000-LAPTOP02.vcf"));

    let err = apply_merge(&vfs, "/lib", &plan, &[]).unwrap_err();
    assert_eq!(err, StoreError::IncompleteChoices);
    assert!(
        vfs.exists("/lib/bob.sync-conflict-20200901-120000-LAPTOP02.vcf"),
        "R10: no silent delete"
    );

    let card = apply_merge(
        &vfs,
        "/lib",
        &plan,
        &[(
            "tel:cell".into(),
            "bob.sync-conflict-20200901-120000-LAPTOP02.vcf".into(),
        )],
    )
    .unwrap();
    assert_eq!(card.phones[0].value, "+15552222");
    assert!(!vfs.exists("/lib/bob.sync-conflict-20200901-120000-LAPTOP02.vcf"));
}

#[test]
fn delete_versus_modify_keeps_the_copy() {
    let vfs = MemVfs::new();
    load_tree(&vfs, "/lib", "r8/deleted-vs-modified");
    let store = Store::open(&vfs, "/lib").unwrap();
    assert!(store.cards().is_empty(), "copy is not content");
    let group = &store.conflict_groups()[0];
    let plan = plan_merge(&vfs, "/lib", group).unwrap();
    assert_eq!(plan.kind, MergeKind::DeletedVersusModified);
    assert_eq!(plan.merged.full_name, "Carol");
    apply_merge(&vfs, "/lib", &plan, &[]).unwrap();
    assert!(vfs.exists("/lib/carol.vcf"));
    let disk = parse(&vfs.read("/lib/carol.vcf").unwrap(), "carol.vcf", false).unwrap();
    assert_eq!(disk.full_name, "Carol");
    assert!(!vfs.exists("/lib/carol.sync-conflict-20200901-120000-PHONE01.vcf"));
}

#[test]
fn nested_groups_write_the_survivor_in_their_own_directory() {
    let vfs = MemVfs::new();
    for dir in ["2024", "Archive"] {
        let mut surviving = Card::new("photo.vcf");
        surviving.local_id = format!("{dir}-id");
        surviving.full_name = format!("{dir} Alice");
        vfs.insert(
            &format!("/lib/{dir}/photo.vcf"),
            write(&surviving).into_bytes(),
        );
        vfs.insert(
            &format!("/lib/{dir}/photo.sync-conflict-20200901-120000-PHONE01.vcf"),
            write(&surviving).into_bytes(),
        );
    }

    let store = Store::open(&vfs, "/lib").unwrap();
    assert_eq!(store.conflict_groups().len(), 2);
    for group in store.conflict_groups() {
        let plan = plan_merge(&vfs, "/lib", group).unwrap();
        assert!(plan.surviving_path.starts_with(&group.dir));
        apply_merge(&vfs, "/lib", &plan, &[]).unwrap();
    }

    assert!(vfs.exists("/lib/2024/photo.vcf"));
    assert!(vfs.exists("/lib/Archive/photo.vcf"));
}

#[test]
fn repeated_values_with_the_same_label_are_not_collapsed() {
    let vfs = MemVfs::new();
    let mut card = Card::new("alice.vcf");
    card.local_id = "alice".into();
    card.full_name = "Alice".into();
    card.phones = vec![
        Labeled {
            label: "cell".into(),
            value: "111".into(),
        },
        Labeled {
            label: "cell".into(),
            value: "222".into(),
        },
    ];
    let bytes = write(&card).into_bytes();
    vfs.insert("/lib/alice.vcf", bytes.clone());
    vfs.insert(
        "/lib/alice.sync-conflict-20200901-120000-PHONE01.vcf",
        bytes,
    );

    let store = Store::open(&vfs, "/lib").unwrap();
    let plan = plan_merge(&vfs, "/lib", &store.conflict_groups()[0]).unwrap();

    assert_eq!(plan.kind, MergeKind::Auto);
    assert_eq!(plan.merged.phones.len(), 2);
    assert_eq!(plan.merged.phones[0].value, "111");
    assert_eq!(plan.merged.phones[1].value, "222");
}
