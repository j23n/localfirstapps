//! Streamed NDJSON sample blobs project without parsing export.xml.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use health_core::{
    kind_catalog, observation_gaps, open, query_episodes, query_observations, rebuild, short_kind,
    table_counts, TYPE_EPISODE, TYPE_RETRACT,
};
use localcore_blob::put;
use localcore_log::{append, Event, TYPE_BLOB_IMPORT};
use serde_json::json;

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn golden_path() -> PathBuf {
    crate_dir().join("../../apps/health/testdata/apple/golden.ndjson")
}

#[test]
fn golden_ndjson_blob_projects_observations() {
    let tmp = tempfile::tempdir().unwrap();
    let golden = fs::read(golden_path()).unwrap();
    let expected_obs = golden
        .split(|&b| b == b'\n')
        .filter(|line| line.starts_with(br#"{"class":"observation""#))
        .count();
    let expected_eps = golden
        .split(|&b| b == b'\n')
        .filter(|line| line.starts_with(br#"{"class":"episode""#))
        .count();
    assert!(expected_obs > 0);

    let put = put(tmp.path(), golden.as_slice()).unwrap();
    append(
        tmp.path(),
        &Event::new(
            "01900000-0000-7000-8000-0000000000aa",
            "2025-09-16T00:00:00.000000000Z",
            "iphone",
            TYPE_BLOB_IMPORT,
            json!({
                "sha256": put.hash,
                "size": put.size,
                "name": "golden.ndjson",
                "kind": "samples"
            }),
        ),
    )
    .unwrap();

    rebuild(tmp.path()).unwrap();
    let db = open(tmp.path()).unwrap();
    let counts = table_counts(&db).unwrap();
    assert_eq!(counts.observations as usize, expected_obs);
    assert_eq!(counts.episodes as usize, expected_eps);
    let steps = query_observations(&db, Some("HKQuantityTypeIdentifierStepCount"), None).unwrap();
    assert!(!steps.is_empty());
    assert_eq!(short_kind(&steps[0].kind), "StepCount");

    let catalog = kind_catalog(&db).unwrap();
    assert!(catalog.iter().any(|k| k.kind.ends_with("StepCount")));
    let watch_or_phone = catalog.iter().any(|k| k.sources.len() > 1);
    assert!(watch_or_phone);

    let gaps = observation_gaps(&db, Some("HKQuantityTypeIdentifierStepCount")).unwrap();
    assert!(gaps.present_n() > 0);
    assert!(gaps.span_days() >= gaps.present_n() as i64);
}

#[test]
fn sample_stream_does_not_buffer_the_whole_blob() {
    let tmp = tempfile::tempdir().unwrap();
    let mut blob = Vec::new();
    for i in 0..8_000 {
        writeln!(
            &mut blob,
            r#"{{"class":"observation","item":{{"dedup_key":"k{i:05}","kind":"HKQuantityTypeIdentifierHeartRate","source":"Apple Watch","start_ts":"2025-01-01T00:00:00.000000000Z","end_ts":"2025-01-01T00:00:01.000000000Z","value":"{i}"}}}}"#
        )
        .unwrap();
    }
    let put = put(tmp.path(), blob.as_slice()).unwrap();
    append(
        tmp.path(),
        &Event::new(
            "01900000-0000-7000-8000-0000000000bb",
            "2025-01-01T00:00:00.000000000Z",
            "watch",
            TYPE_BLOB_IMPORT,
            json!({
                "sha256": put.hash,
                "size": put.size,
                "name": "hr.ndjson",
                "kind": "healthkit"
            }),
        ),
    )
    .unwrap();
    rebuild(tmp.path()).unwrap();
    let db = open(tmp.path()).unwrap();
    assert_eq!(table_counts(&db).unwrap().observations, 8_000);
}

#[test]
fn observation_events_in_the_log_project_without_a_blob() {
    let tmp = tempfile::tempdir().unwrap();
    append(
        tmp.path(),
        &Event::new(
            "01900000-0000-7000-8000-0000000000cc",
            "2025-02-02T08:00:00.000000000Z",
            "manual",
            health_core::TYPE_OBSERVATION,
            json!({
                "dedup_key": "manual-1",
                "kind": "HKQuantityTypeIdentifierBodyMass",
                "source": "manual",
                "start_ts": "2025-02-02T08:00:00.000000000Z",
                "end_ts": "2025-02-02T08:00:00.000000000Z",
                "unit": "kg",
                "value": "72.4"
            }),
        ),
    )
    .unwrap();
    rebuild(tmp.path()).unwrap();
    let db = open(tmp.path()).unwrap();
    let rows = query_observations(&db, None, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].value.as_deref(), Some("72.4"));
    assert_eq!(
        rows[0].event_id.as_deref(),
        Some("01900000-0000-7000-8000-0000000000cc")
    );
}

#[test]
fn episode_events_in_the_log_project_without_a_blob() {
    let tmp = tempfile::tempdir().unwrap();
    append(
        tmp.path(),
        &Event::new(
            "01900000-0000-7000-8000-0000000000dd",
            "2025-02-02T09:00:00.000000000Z",
            "manual",
            TYPE_EPISODE,
            json!({
                "dedup_key": "ep-1",
                "kind": "HKWorkoutActivityTypeRunning",
                "start_ts": "2025-02-02T09:00:00.000000000Z",
                "end_ts": "2025-02-02T09:30:00.000000000Z"
            }),
        ),
    )
    .unwrap();
    rebuild(tmp.path()).unwrap();
    let db = open(tmp.path()).unwrap();
    let rows = query_episodes(&db, None).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "HKWorkoutActivityTypeRunning");
    assert_eq!(
        rows[0].event_id.as_deref(),
        Some("01900000-0000-7000-8000-0000000000dd")
    );
}

#[test]
fn retracted_observation_is_not_projected() {
    let tmp = tempfile::tempdir().unwrap();
    let obs_id = "01900000-0000-7000-8000-0000000000ee";
    append(
        tmp.path(),
        &Event::new(
            obs_id,
            "2025-03-01T08:00:00.000000000Z",
            "manual",
            health_core::TYPE_OBSERVATION,
            json!({
                "dedup_key": "void-me",
                "kind": "HKQuantityTypeIdentifierBodyMass",
                "source": "manual",
                "start_ts": "2025-03-01T08:00:00.000000000Z",
                "end_ts": "2025-03-01T08:00:00.000000000Z",
                "value": "70"
            }),
        ),
    )
    .unwrap();
    append(
        tmp.path(),
        &Event::new(
            "01900000-0000-7000-8000-0000000000ef",
            "2025-03-01T08:01:00.000000000Z",
            "manual",
            TYPE_RETRACT,
            json!({"target": obs_id}),
        ),
    )
    .unwrap();
    rebuild(tmp.path()).unwrap();
    let db = open(tmp.path()).unwrap();
    assert_eq!(query_observations(&db, None, None).unwrap().len(), 0);
}
