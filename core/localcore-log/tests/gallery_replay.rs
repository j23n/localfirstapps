//! Dual-consumer replay: gallery person-state ops on the shared log.
//!
//! M2: a pre-change UserDefaults dump migrates to operations and projects
//! back to the same decisions. `person_renamed` is the rename, not a
//! snapshot rewrite of the five keys.

use localcore_log::{
    append, migrate_from_snapshot_json, project_people, project_people_at, read_all, ts_with_nanos,
    Event, PersonSnapshot, PeopleState, TYPE_FEATURED_PHOTO_SET, TYPE_PERSON_FEATURED,
    TYPE_PERSON_HIDDEN, TYPE_PERSON_ME_SET, TYPE_PERSON_MIGRATED, TYPE_PERSON_RENAMED,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

fn ev(id: &str, ts: &str, dev: &str, typ: &str, body: serde_json::Value) -> Event {
    Event::new(id, ts, dev, typ, body)
}

/// Hide Anna, feature Ada, set me to Ada, set Ada's featured photo, rename Anna → Ann.
fn person_state_ops(dev: &str) -> Vec<Event> {
    vec![
        ev(
            "01900000-0000-7000-8000-0000000000a1",
            "2024-07-01T10:00:00.000000000Z",
            dev,
            TYPE_PERSON_HIDDEN,
            json!({"path": "People/Anna"}),
        ),
        ev(
            "01900000-0000-7000-8000-0000000000a2",
            "2024-07-01T10:00:01.000000000Z",
            dev,
            TYPE_PERSON_FEATURED,
            json!({"path": "People/Ada"}),
        ),
        ev(
            "01900000-0000-7000-8000-0000000000a3",
            "2024-07-01T10:00:02.000000000Z",
            dev,
            TYPE_PERSON_ME_SET,
            json!({"path": "People/Ada"}),
        ),
        ev(
            "01900000-0000-7000-8000-0000000000a4",
            "2024-07-01T10:00:03.000000000Z",
            dev,
            TYPE_FEATURED_PHOTO_SET,
            json!({"path": "People/Ada", "photo": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"}),
        ),
        ev(
            "01900000-0000-7000-8000-0000000000a5",
            "2024-07-01T10:00:04.000000000Z",
            dev,
            TYPE_PERSON_RENAMED,
            json!({"from": "People/Anna", "to": "People/Ann"}),
        ),
    ]
}

fn expected_people() -> PeopleState {
    PeopleState {
        hidden: BTreeSet::from(["People/Ann".to_string()]),
        featured: vec!["People/Ada".to_string()],
        me: Some("People/Ada".to_string()),
        featured_photo: BTreeMap::from([(
            "People/Ada".to_string(),
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string(),
        )]),
        links: BTreeMap::new(),
    }
}

#[test]
fn gallery_person_state_replay() {
    let root = tempfile::tempdir().unwrap();
    for e in person_state_ops("ios") {
        append(root.path(), &e).unwrap();
    }
    let evs = read_all(root.path()).unwrap();
    assert_eq!(
        evs.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        [
            "01900000-0000-7000-8000-0000000000a1",
            "01900000-0000-7000-8000-0000000000a2",
            "01900000-0000-7000-8000-0000000000a3",
            "01900000-0000-7000-8000-0000000000a4",
            "01900000-0000-7000-8000-0000000000a5",
        ]
    );
    assert_eq!(
        evs.iter().map(|e| e.event_type.as_str()).collect::<Vec<_>>(),
        [
            TYPE_PERSON_HIDDEN,
            TYPE_PERSON_FEATURED,
            TYPE_PERSON_ME_SET,
            TYPE_FEATURED_PHOTO_SET,
            TYPE_PERSON_RENAMED,
        ]
    );

    let state = project_people(&evs);
    assert_eq!(state, expected_people());
    assert!(
        state.hidden.contains("People/Ann") && !state.hidden.contains("People/Anna"),
        "rename must move hidden Anna → Ann: {:?}",
        state.hidden
    );
    assert_eq!(
        state.featured,
        vec!["People/Ada".to_string()],
        "rename must not drop Ada"
    );
    assert_eq!(state.me.as_deref(), Some("People/Ada"));
}

#[test]
fn gallery_replay_sorts_across_devices() {
    let chronological = person_state_ops("ios");
    let root_a = tempfile::tempdir().unwrap();
    for e in &chronological {
        append(root_a.path(), e).unwrap();
    }
    let order_a: Vec<String> = read_all(root_a.path())
        .unwrap()
        .into_iter()
        .map(|e| format!("{}:{}", e.ts, e.id))
        .collect();

    // Same payloads, other device, appended newest-first into another month file.
    let root_b = tempfile::tempdir().unwrap();
    for e in chronological.iter().rev() {
        let mut flipped = e.clone();
        flipped.dev = "linux".into();
        append(root_b.path(), &flipped).unwrap();
    }
    let order_b: Vec<String> = read_all(root_b.path())
        .unwrap()
        .into_iter()
        .map(|e| format!("{}:{}", e.ts, e.id))
        .collect();
    assert_eq!(order_a, order_b);
    assert_eq!(project_people(&read_all(root_b.path()).unwrap()), expected_people());

    // One archive, two device files: union still sorts by (ts, id), not walk order.
    let root_c = tempfile::tempdir().unwrap();
    for e in chronological.iter().rev() {
        let mut linux = e.clone();
        linux.dev = "linux".into();
        linux.id = format!("{}-linux", e.id);
        append(root_c.path(), &linux).unwrap();
    }
    for e in &chronological {
        append(root_c.path(), e).unwrap();
    }
    let merged = read_all(root_c.path()).unwrap();
    let keys: Vec<(String, String)> = merged.iter().map(|e| (e.ts.clone(), e.id.clone())).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

fn m2_dump() -> String {
    fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/m2/userdefaults-person-state.json"),
    )
    .unwrap()
}

fn expected_from_m2_dump() -> PeopleState {
    PeopleState {
        hidden: BTreeSet::from(["People/Anna Schmidt".to_string()]),
        featured: vec!["People/Ada".to_string(), "People/Cy".to_string()],
        me: Some("People/Ada".to_string()),
        featured_photo: BTreeMap::from([(
            "People/Ada".to_string(),
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string(),
        )]),
        links: BTreeMap::from([
            ("People/Ada".to_string(), "CN:ada-uuid".to_string()),
            ("People/Erin Hidden".to_string(), String::new()),
        ]),
    }
}

/// ADR 0005 R19: pre-M2 UserDefaults dump → migrate → project equals the dump.
#[test]
fn m2_userdefaults_dump_survives_migrate_and_project() {
    let dump = m2_dump();
    let snap = PersonSnapshot::parse(&dump).unwrap();
    assert_eq!(snap.to_people_state(), expected_from_m2_dump());

    let root = tempfile::tempdir().unwrap();
    let n = migrate_from_snapshot_json(root.path(), "ios", &dump).unwrap();
    assert_eq!(
        n, 8,
        "2 featured + 1 photo + 1 me + 2 links + 1 hidden + marker"
    );
    let evs = read_all(root.path()).unwrap();
    assert_eq!(evs.len(), n);
    assert_eq!(
        evs.last().map(|e| e.event_type.as_str()),
        Some(TYPE_PERSON_MIGRATED),
        "marker is written last"
    );
    assert_eq!(
        project_people_at(root.path()).unwrap(),
        snap.to_people_state()
    );
    assert_eq!(
        project_people_at(root.path()).unwrap(),
        expected_from_m2_dump()
    );

    let again = migrate_from_snapshot_json(root.path(), "ios", &dump).unwrap();
    assert_eq!(again, 0, "migration is one-shot once the marker exists");
    assert_eq!(read_all(root.path()).unwrap().len(), n);
}

/// A second device still imports its own dump; replay unions by (ts, id).
#[test]
fn m2_second_device_still_migrates() {
    let dump = m2_dump();
    let root = tempfile::tempdir().unwrap();
    migrate_from_snapshot_json(root.path(), "ios", &dump).unwrap();
    let n = migrate_from_snapshot_json(root.path(), "linux", &dump).unwrap();
    assert_eq!(n, 8);
    assert_eq!(
        project_people_at(root.path()).unwrap(),
        expected_from_m2_dump()
    );
}

/// Rename is a `person_renamed` event, not a rewrite of the five keys.
#[test]
fn m2_rename_is_a_replayed_event() {
    let dump = m2_dump();
    let root = tempfile::tempdir().unwrap();
    migrate_from_snapshot_json(root.path(), "ios", &dump).unwrap();
    let last = read_all(root.path()).unwrap().pop().unwrap();
    let last_nanos: u32 = last.ts[20..29].parse().unwrap();
    let mut renamed = Event::fresh(
        "ios",
        TYPE_PERSON_RENAMED,
        json!({"from": "People/Anna Schmidt", "to": "People/Ann Schmidt"}),
    );
    renamed.ts = ts_with_nanos(&last.ts, last_nanos + 1).unwrap();
    append(root.path(), &renamed).unwrap();
    let state = project_people_at(root.path()).unwrap();
    assert!(state.hidden.contains("People/Ann Schmidt"));
    assert!(!state.hidden.contains("People/Anna Schmidt"));
    assert_eq!(
        state.featured,
        vec!["People/Ada".to_string(), "People/Cy".to_string()]
    );
    assert_eq!(state.me.as_deref(), Some("People/Ada"));
}

/// A crash after some snapshot events, before the marker, must retry. The
/// second call after the marker is a no-op and must not duplicate featured
/// list entries.
#[test]
fn migrate_retries_partial_file_until_marker() {
    let dump = m2_dump();
    let root = tempfile::tempdir().unwrap();
    append(
        root.path(),
        &Event::fresh("ios", TYPE_PERSON_FEATURED, json!({"path": "People/Ada"})),
    )
    .unwrap();
    append(
        root.path(),
        &Event::fresh("ios", TYPE_PERSON_FEATURED, json!({"path": "People/Cy"})),
    )
    .unwrap();
    append(
        root.path(),
        &Event::fresh(
            "ios",
            TYPE_FEATURED_PHOTO_SET,
            json!({
                "path": "People/Ada",
                "photo": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
            }),
        ),
    )
    .unwrap();
    assert!(
        !read_all(root.path())
            .unwrap()
            .iter()
            .any(|e| e.event_type == TYPE_PERSON_MIGRATED),
        "fixture is a partial file without a marker"
    );

    let n = migrate_from_snapshot_json(root.path(), "ios", &dump).unwrap();
    assert!(n > 0, "partial file without marker still migrates");

    let evs = read_all(root.path()).unwrap();
    let featured: Vec<&str> = evs
        .iter()
        .filter(|e| e.dev == "ios" && e.event_type == TYPE_PERSON_FEATURED)
        .filter_map(|e| e.body.get("path").and_then(|v| v.as_str()))
        .collect();
    assert_eq!(
        featured,
        ["People/Ada", "People/Cy"],
        "retry must not duplicate person_featured entries"
    );
    assert!(
        evs.iter()
            .any(|e| e.dev == "ios" && e.event_type == TYPE_PERSON_MIGRATED),
        "marker written on the completing retry"
    );
    assert_eq!(
        project_people_at(root.path()).unwrap(),
        expected_from_m2_dump()
    );

    let again = migrate_from_snapshot_json(root.path(), "ios", &dump).unwrap();
    assert_eq!(again, 0, "second call after marker is 0");
    assert_eq!(read_all(root.path()).unwrap().len(), evs.len());
}
