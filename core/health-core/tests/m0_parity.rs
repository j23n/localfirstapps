//! m0 fixture parity: events, corrections, blobs, portable-v1, torn tails.

use std::fs;
use std::path::{Path, PathBuf};

use health_core::{
    export, fsck, observation_gaps, open, query, rebuild, restore, semantic_dump, table_counts,
    Filter, TYPE_NOTE,
};
use localcore_log::{has_blob_import, Event};

const HELLO_SHA: &str = "a70940623490fa4c251737cf74e1bf75a0327bb18766cc5620edbde3a985c96d";

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn m0_root() -> PathBuf {
    crate_dir().join("../../apps/health/testdata/m0")
}

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
fn rebuild_m0_applies_supersede_and_retract() {
    let tmp = tempfile::tempdir().unwrap();
    copy_tree(&m0_root(), tmp.path());
    let report = rebuild(tmp.path()).unwrap();
    assert_eq!(report.events.len(), 6);
    assert!(report.torn_tails.is_empty());

    let db = open(tmp.path()).unwrap();
    let all = query(&db, &Filter::default()).unwrap();
    assert_eq!(all.len(), 6);
    let current = query(
        &db,
        &Filter {
            current: true,
            ..Filter::default()
        },
    )
    .unwrap();
    let ids: Vec<&str> = current.iter().map(|e| e.id.as_str()).collect();
    assert!(!ids.contains(&"01900000-0000-7000-8000-000000000002"));
    assert!(!ids.contains(&"01900000-0000-7000-8000-000000000004"));
    assert!(ids.contains(&"01900000-0000-7000-8000-000000000001"));
    assert!(ids.contains(&"01900000-0000-7000-8000-000000000003"));

    let notes = query(
        &db,
        &Filter {
            event_type: Some(TYPE_NOTE.into()),
            ..Filter::default()
        },
    )
    .unwrap();
    assert_eq!(notes.len(), 3);

    let counts = table_counts(&db).unwrap();
    assert_eq!(counts.events, 6);
    assert_eq!(counts.blobs, 1);
    assert_eq!(counts.observations, 0);
    assert_eq!(counts.episodes, 0);
    assert!(has_blob_import(tmp.path(), HELLO_SHA).unwrap());
}

#[test]
fn rebuild_keeps_complete_events_before_a_torn_tail() {
    let tmp = tempfile::tempdir().unwrap();
    copy_tree(&m0_root(), tmp.path());
    let path = tmp.path().join("log/manual/2024-01.ndjson");
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend_from_slice(br#"{"id":"torn"#);
    fs::write(&path, bytes).unwrap();

    let report = rebuild(tmp.path()).unwrap();
    assert_eq!(report.events.len(), 6);
    assert_eq!(report.torn_tails.len(), 1);
    let db = open(tmp.path()).unwrap();
    assert_eq!(table_counts(&db).unwrap().events, 6);
}

#[test]
fn semantic_dump_is_ordered_and_stable() {
    let tmp = tempfile::tempdir().unwrap();
    copy_tree(&m0_root(), tmp.path());
    rebuild(tmp.path()).unwrap();
    let db = open(tmp.path()).unwrap();
    let dump = semantic_dump(&db).unwrap();
    let events = dump["events"].as_array().unwrap();
    let ids: Vec<&str> = events.iter().map(|e| e["id"].as_str().unwrap()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted);
    assert_eq!(dump["blobs"][0]["sha256"].as_str().unwrap(), HELLO_SHA);
    let second = events
        .iter()
        .find(|e| e["id"] == "01900000-0000-7000-8000-000000000002")
        .unwrap();
    assert_eq!(
        second["superseded_by"].as_str().unwrap(),
        "01900000-0000-7000-8000-000000000003"
    );
    let retracted = events
        .iter()
        .find(|e| e["id"] == "01900000-0000-7000-8000-000000000004")
        .unwrap();
    assert_eq!(retracted["retracted"], true);
}

#[test]
fn portable_round_trip_and_fsck() {
    let src = tempfile::tempdir().unwrap();
    copy_tree(&m0_root(), src.path());
    let export_dir = tempfile::tempdir().unwrap();
    let man = export(src.path(), export_dir.path()).unwrap();
    assert_eq!(man.archive_version, 1);
    assert_eq!(man.event_count, 6);
    assert_eq!(man.blob_count, 1);
    assert_eq!(man.blobs[0].sha256, HELLO_SHA);
    assert!(export_dir.path().join("README.md").exists());

    let dest = tempfile::tempdir().unwrap();
    restore(export_dir.path(), dest.path()).unwrap();
    restore(export_dir.path(), dest.path()).unwrap();
    rebuild(src.path()).unwrap();
    rebuild(dest.path()).unwrap();
    let src_events: Vec<Event> = localcore_log::read_all(src.path()).unwrap();
    let dest_events: Vec<Event> = localcore_log::read_all(dest.path()).unwrap();
    assert_eq!(src_events, dest_events);
    let src_dump = semantic_dump(&open(src.path()).unwrap()).unwrap();
    let dest_dump = semantic_dump(&open(dest.path()).unwrap()).unwrap();
    assert_eq!(src_dump["blobs"], dest_dump["blobs"]);
    assert_eq!(src_dump["events"], dest_dump["events"]);
    let ok = fsck(dest.path()).unwrap();
    assert!(ok.ok(), "{:?}", ok.issues);
    assert_eq!(ok.events, 6);
    assert_eq!(ok.blobs, 1);
}

#[test]
fn m0_gaps_are_empty_without_samples() {
    let tmp = tempfile::tempdir().unwrap();
    copy_tree(&m0_root(), tmp.path());
    rebuild(tmp.path()).unwrap();
    let db = open(tmp.path()).unwrap();
    let gaps = observation_gaps(&db, None).unwrap();
    assert!(gaps.present.is_empty());
    assert_eq!(gaps.span_days(), 0);
}
