use std::fs;
use std::path::Path;

use health_ffi::HealthArchive;

fn copy_tree(src: &Path, dst: &Path) {
    for ent in fs::read_dir(src).unwrap() {
        let ent = ent.unwrap();
        let to = dst.join(ent.file_name());
        if ent.file_type().unwrap().is_dir() {
            fs::create_dir_all(&to).unwrap();
            copy_tree(&ent.path(), &to);
        } else {
            fs::create_dir_all(to.parent().unwrap()).unwrap();
            fs::copy(ent.path(), to).unwrap();
        }
    }
}

#[test]
fn archive_object_rebuilds_m0_and_lists_current_events() {
    let tmp = tempfile::tempdir().unwrap();
    copy_tree(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/health/testdata/m0")
            .as_path(),
        tmp.path(),
    );
    let archive = HealthArchive::new(tmp.path().to_string_lossy().into());
    let rebuilt = archive.rebuild().unwrap();
    assert!(rebuilt.detail.is_none(), "{rebuilt:?}");
    let counts = archive.counts().unwrap();
    assert_eq!(counts.events, 6);
    assert_eq!(counts.blobs, 1);
    let current = archive.events(true).unwrap();
    assert!(!current
        .iter()
        .any(|row| row.trailing.as_deref() == Some("retracted")));
    assert!(archive.fsck().unwrap().ok);
}
