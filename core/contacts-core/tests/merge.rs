//! ADR 0005 R8–R11 against the committed fixture trees.

use contacts_core::{apply_merge, parse, plan_merge, write, MergeKind, Store, StoreError};
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
