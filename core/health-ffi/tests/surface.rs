use std::fs;
use std::path::Path;

use health_ffi::{HealthArchive, HealthError};

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

#[test]
fn health_error_maps_core_variants() {
    let invalid: HealthError = health_core::Error::Invalid("bad event".into()).into();
    assert!(matches!(invalid, HealthError::Invalid { .. }));
    assert_eq!(invalid.to_string(), "bad event");

    let missing: HealthError = health_core::Error::MissingDatabase {
        path: Path::new("/tmp/archive.db").to_path_buf(),
    }
    .into();
    assert!(matches!(missing, HealthError::MissingDatabase { .. }));
    assert!(missing.to_string().contains("run rebuild"));

    let io: HealthError = health_core::Error::Io(std::io::Error::other("disk")).into();
    assert!(matches!(io, HealthError::Io { .. }));
}

#[test]
fn restore_archive_uses_safe_restore() {
    use health_core::{append, export, Event, TYPE_NOTE, TYPE_RETRACT};
    use serde_json::json;

    let dest = tempfile::tempdir().unwrap();
    let existing_id = "01900000-0000-7000-8000-0000000000aa";
    append(
        dest.path(),
        &Event::new(
            existing_id,
            "2025-01-01T00:00:00.000000000Z",
            "manual",
            TYPE_NOTE,
            json!({"text": "keep"}),
        ),
    )
    .unwrap();

    let src = tempfile::tempdir().unwrap();
    append(
        src.path(),
        &Event::new(
            "01900000-0000-7000-8000-0000000000bb",
            "2025-01-02T00:00:00.000000000Z",
            "manual",
            TYPE_RETRACT,
            json!({"target": existing_id}),
        ),
    )
    .unwrap();
    let export_dir = tempfile::tempdir().unwrap();
    export(src.path(), export_dir.path()).unwrap();

    let archive = HealthArchive::new(dest.path().to_string_lossy().into());
    let err = archive
        .restore_archive(export_dir.path().to_string_lossy().into())
        .unwrap_err();
    assert!(matches!(err, HealthError::Invalid { .. }), "{err}");
    let dest_events = health_core::read_all(dest.path()).unwrap();
    assert_eq!(dest_events.len(), 1);
    assert!(!dest_events.iter().any(|ev| ev.event_type == TYPE_RETRACT));
}
