use std::fs;
use std::path::Path;

use music_core::{
    apply_merge, plan_merge, resolve_conflict_logged, ConflictDisposition, ResolveConflictCommand,
    StdVfs, Store, StoreError,
};

fn copy_fixture(case: &str, names: &[&str]) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/r8")
        .join(case);
    for name in names {
        fs::copy(source.join(name), temp.path().join(name)).unwrap();
    }
    temp
}

fn root(temp: &tempfile::TempDir) -> &str {
    temp.path().to_str().unwrap()
}

#[test]
fn disjoint_ordered_edits_union_and_match_canonical_fixture() {
    let names = ["mix.m3u", "mix.sync-conflict-20200901-120000-PHONE01.m3u"];
    let first = copy_fixture("disjoint", &names);
    let second = copy_fixture("disjoint", &names);
    let expected = include_bytes!("../fixtures/r8/disjoint/expected.m3u");

    let mut outputs = Vec::new();
    for temp in [&first, &second] {
        let vfs = StdVfs::new(music_core::TEMP_PREFIX);
        let store = Store::open(&vfs, root(temp)).unwrap();
        let group = &store.conflict_groups()[0];
        let plan = plan_merge(&vfs, root(temp), group).unwrap();
        assert_eq!(plan.disposition, ConflictDisposition::Auto);

        // Planning is read-only: explicit apply is what permits deletion.
        assert!(temp
            .path()
            .join("mix.sync-conflict-20200901-120000-PHONE01.m3u")
            .exists());
        apply_merge(&vfs, &plan, None).unwrap();
        assert!(!temp
            .path()
            .join("mix.sync-conflict-20200901-120000-PHONE01.m3u")
            .exists());
        outputs.push(fs::read(temp.path().join("mix.m3u")).unwrap());
    }
    assert_eq!(outputs[0], expected);
    assert_eq!(outputs[0], outputs[1], "independent projections converge");
}

#[test]
fn same_gap_requires_explicit_whole_document_choice() {
    let temp = copy_fixture(
        "same-field",
        &["mix.m3u", "mix.sync-conflict-20200901-120000-LAPTOP02.m3u"],
    );
    let vfs = StdVfs::new(music_core::TEMP_PREFIX);
    let store = Store::open(&vfs, root(&temp)).unwrap();
    let plan = plan_merge(&vfs, root(&temp), &store.conflict_groups()[0]).unwrap();
    assert_eq!(plan.disposition, ConflictDisposition::Choice);
    assert!(matches!(
        apply_merge(&vfs, &plan, None),
        Err(StoreError::NeedsChoice)
    ));
    assert!(temp
        .path()
        .join("mix.sync-conflict-20200901-120000-LAPTOP02.m3u")
        .exists());

    apply_merge(
        &vfs,
        &plan,
        Some("mix.sync-conflict-20200901-120000-LAPTOP02.m3u"),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(temp.path().join("mix.m3u")).unwrap(),
        "#EXTM3U\nbase.mp3\nright.mp3\nend.mp3\n"
    );
}

#[test]
fn deleted_versus_modified_keeps_data_only_after_confirmation() {
    let name = "mix.sync-conflict-20200901-120000-PHONE01.m3u8";
    let temp = copy_fixture("deleted-vs-modified", &[name]);
    let vfs = StdVfs::new(music_core::TEMP_PREFIX);
    let mut store = Store::open(&vfs, root(&temp)).unwrap();
    let group_id = music_core::conflict_rows(&vfs, &store).unwrap()[0]
        .id
        .clone();
    let plan = plan_merge(&vfs, root(&temp), &store.conflict_groups()[0]).unwrap();
    assert_eq!(plan.disposition, ConflictDisposition::DeletedVersusModified);
    assert!(!temp.path().join("mix.m3u8").exists());
    assert!(temp.path().join(name).exists());

    resolve_conflict_logged(
        &vfs,
        &mut store,
        "phone",
        ResolveConflictCommand {
            group_id,
            selected_source: None,
        },
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(temp.path().join("mix.m3u8")).unwrap(),
        "#EXTM3U\nkept.mp3\n"
    );
    assert!(!temp.path().join(name).exists());
    assert!(store.conflict_groups().is_empty());
}
