//! Dual-consumer replay: gallery memory-chrome ops on the shared log.
//!
//! Independent of person-state types. Memory events must not fold into
//! `project_people`, and `person_renamed` is not applicable to memory ids.

use localcore_log::{
    append, append_memory, append_person, is_memory_event_type, is_person_event_type,
    migrate_from_snapshot_json, migrate_memories_from_snapshot_json, project_memories,
    project_memories_at, project_people, project_people_at, read_all, MemorySnapshot, MemoryState,
    TYPE_MEMORY_BIRTHDAYS_SET, TYPE_MEMORY_CLUSTER_SURFACED, TYPE_MEMORY_GENERATED_DAY,
    TYPE_MEMORY_HIDDEN, TYPE_MEMORY_MIGRATED, TYPE_MEMORY_SEEN, TYPE_MEMORY_UNHIDDEN,
    TYPE_PERSON_HIDDEN, TYPE_PERSON_MIGRATED, TYPE_PERSON_RENAMED,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

fn ev(id: &str, ts: &str, dev: &str, typ: &str, body: serde_json::Value) -> localcore_log::Event {
    localcore_log::Event::new(id, ts, dev, typ, body)
}

fn m2_dump() -> String {
    fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/m2/userdefaults-memory-state.json"),
    )
    .unwrap()
}

fn expected_from_m2_dump() -> MemoryState {
    MemoryState {
        hidden: BTreeSet::from(["onThisDay-2024-06-11".to_string()]),
        seen: BTreeMap::from([(
            "onThisDay-2023-01-01".to_string(),
            "2024-06-01T12:00:00Z".to_string(),
        )]),
        surfaced: BTreeMap::from([("onThisDay".to_string(), "2024-06-10T08:00:00Z".to_string())]),
        birthdays_enabled: false,
        generated_day: Some("2024-06-11T00:00:00Z".to_string()),
    }
}

fn person_dump() -> String {
    fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/m2/userdefaults-person-state.json"),
    )
    .unwrap()
}

#[test]
fn memory_ops_project_replay() {
    let root = tempfile::tempdir().unwrap();
    let ops = [
        ev(
            "01900000-0000-7000-8000-0000000000b1",
            "2024-07-01T10:00:00.000000000Z",
            "ios",
            TYPE_MEMORY_HIDDEN,
            json!({"id": "onThisDay-2024-06-11"}),
        ),
        ev(
            "01900000-0000-7000-8000-0000000000b2",
            "2024-07-01T10:00:01.000000000Z",
            "ios",
            TYPE_MEMORY_SEEN,
            json!({"id": "yearsAgo-5-2024-06-11", "at": "2024-07-01T10:00:01Z"}),
        ),
        ev(
            "01900000-0000-7000-8000-0000000000b3",
            "2024-07-01T10:00:02.000000000Z",
            "ios",
            TYPE_MEMORY_CLUSTER_SURFACED,
            json!({"key": "onThisDay", "at": "2024-07-01T10:00:02Z"}),
        ),
        ev(
            "01900000-0000-7000-8000-0000000000b4",
            "2024-07-01T10:00:03.000000000Z",
            "ios",
            TYPE_MEMORY_BIRTHDAYS_SET,
            json!({"enabled": false}),
        ),
        ev(
            "01900000-0000-7000-8000-0000000000b5",
            "2024-07-01T10:00:04.000000000Z",
            "ios",
            TYPE_MEMORY_GENERATED_DAY,
            json!({"day": "2024-07-01T00:00:00Z"}),
        ),
        ev(
            "01900000-0000-7000-8000-0000000000b6",
            "2024-07-01T10:00:05.000000000Z",
            "ios",
            TYPE_MEMORY_UNHIDDEN,
            json!({"id": "onThisDay-2024-06-11"}),
        ),
    ];
    for e in &ops {
        append(root.path(), e).unwrap();
    }
    let state = project_memories_at(root.path()).unwrap();
    assert!(state.hidden.is_empty(), "unhide must drop the id");
    assert_eq!(
        state.seen.get("yearsAgo-5-2024-06-11").map(String::as_str),
        Some("2024-07-01T10:00:01Z")
    );
    assert_eq!(
        state.surfaced.get("onThisDay").map(String::as_str),
        Some("2024-07-01T10:00:02Z")
    );
    assert!(!state.birthdays_enabled);
    assert_eq!(state.generated_day.as_deref(), Some("2024-07-01T00:00:00Z"));
}

#[test]
fn m2_userdefaults_dump_survives_migrate_and_project() {
    let dump = m2_dump();
    let snap = MemorySnapshot::parse(&dump).unwrap();
    assert_eq!(snap.to_memory_state(), expected_from_m2_dump());

    let root = tempfile::tempdir().unwrap();
    let n = migrate_memories_from_snapshot_json(root.path(), "ios", &dump).unwrap();
    assert_eq!(
        n, 6,
        "1 hidden + 1 seen + 1 surfaced + 1 birthdays + 1 day + marker"
    );
    let evs = read_all(root.path()).unwrap();
    assert_eq!(evs.len(), n);
    assert_eq!(
        evs.last().map(|e| e.event_type.as_str()),
        Some(TYPE_MEMORY_MIGRATED),
        "marker is written last"
    );
    assert_eq!(
        project_memories_at(root.path()).unwrap(),
        snap.to_memory_state()
    );

    let again = migrate_memories_from_snapshot_json(root.path(), "ios", &dump).unwrap();
    assert_eq!(again, 0, "migration is one-shot once the marker exists");
    assert_eq!(read_all(root.path()).unwrap().len(), n);
}

/// A crash after some snapshot events, before the marker, must retry.
#[test]
fn migrate_retries_partial_file_until_marker() {
    let dump = m2_dump();
    let root = tempfile::tempdir().unwrap();
    append(
        root.path(),
        &localcore_log::Event::fresh(
            "ios",
            TYPE_MEMORY_HIDDEN,
            json!({"id": "onThisDay-2024-06-11"}),
        ),
    )
    .unwrap();
    append(
        root.path(),
        &localcore_log::Event::fresh(
            "ios",
            TYPE_MEMORY_SEEN,
            json!({"id": "onThisDay-2023-01-01", "at": "2024-06-01T12:00:00Z"}),
        ),
    )
    .unwrap();
    assert!(
        !read_all(root.path())
            .unwrap()
            .iter()
            .any(|e| e.event_type == TYPE_MEMORY_MIGRATED),
        "fixture is a partial file without a marker"
    );

    let n = migrate_memories_from_snapshot_json(root.path(), "ios", &dump).unwrap();
    assert!(n > 0, "partial file without marker still migrates");

    let evs = read_all(root.path()).unwrap();
    let hidden: Vec<&str> = evs
        .iter()
        .filter(|e| e.dev == "ios" && e.event_type == TYPE_MEMORY_HIDDEN)
        .filter_map(|e| e.body.get("id").and_then(|v| v.as_str()))
        .collect();
    assert_eq!(
        hidden,
        ["onThisDay-2024-06-11"],
        "retry must not duplicate memory_hidden entries"
    );
    assert!(
        evs.iter()
            .any(|e| e.dev == "ios" && e.event_type == TYPE_MEMORY_MIGRATED),
        "marker written on the completing retry"
    );
    assert_eq!(
        project_memories_at(root.path()).unwrap(),
        expected_from_m2_dump()
    );

    let again = migrate_memories_from_snapshot_json(root.path(), "ios", &dump).unwrap();
    assert_eq!(again, 0, "second call after marker is 0");
    assert_eq!(read_all(root.path()).unwrap().len(), evs.len());
}

/// `person_renamed` does not rewrite memory ids. Birthday memories keep the
/// path they were generated with; that is a generation concern, not a log
/// fold.
#[test]
fn rename_is_not_applicable_to_memory_ids() {
    let root = tempfile::tempdir().unwrap();
    append(
        root.path(),
        &ev(
            "01900000-0000-7000-8000-0000000000c1",
            "2024-07-01T10:00:00.000000000Z",
            "ios",
            TYPE_MEMORY_HIDDEN,
            json!({"id": "birthday-People/Anna"}),
        ),
    )
    .unwrap();
    append(
        root.path(),
        &ev(
            "01900000-0000-7000-8000-0000000000c2",
            "2024-07-01T10:00:01.000000000Z",
            "ios",
            TYPE_PERSON_RENAMED,
            json!({"from": "People/Anna", "to": "People/Ann"}),
        ),
    )
    .unwrap();
    let state = project_memories_at(root.path()).unwrap();
    assert_eq!(
        state.hidden,
        BTreeSet::from(["birthday-People/Anna".to_string()]),
        "person_renamed must not remap memory ids"
    );
    assert!(!state.hidden.contains("birthday-People/Ann"));
}

/// Memory events live on the same log but must not fold into PeopleState.
#[test]
fn memory_events_do_not_fold_into_project_people() {
    let root = tempfile::tempdir().unwrap();
    migrate_memories_from_snapshot_json(root.path(), "ios", &m2_dump()).unwrap();
    append(
        root.path(),
        &ev(
            "01900000-0000-7000-8000-0000000000d1",
            "2024-07-01T11:00:00.000000000Z",
            "ios",
            TYPE_PERSON_HIDDEN,
            json!({"path": "People/Ada"}),
        ),
    )
    .unwrap();

    let people = project_people_at(root.path()).unwrap();
    assert_eq!(
        people.hidden,
        BTreeSet::from(["People/Ada".to_string()]),
        "only person_hidden should populate PeopleState.hidden"
    );
    assert!(people.featured.is_empty());
    assert!(people.me.is_none());

    let memories = project_memories_at(root.path()).unwrap();
    assert_eq!(memories, expected_from_m2_dump());
    assert!(
        !memories.hidden.contains("People/Ada"),
        "person_hidden must not fold into MemoryState"
    );
}

#[test]
fn person_migrate_does_not_write_memory_marker() {
    let root = tempfile::tempdir().unwrap();
    migrate_from_snapshot_json(root.path(), "ios", &person_dump()).unwrap();
    let evs = read_all(root.path()).unwrap();
    assert!(evs.iter().any(|e| e.event_type == TYPE_PERSON_MIGRATED));
    assert!(!evs.iter().any(|e| e.event_type == TYPE_MEMORY_MIGRATED));
    assert_eq!(
        project_memories(&evs),
        MemoryState::default(),
        "person dump must not populate MemoryState"
    );
}

#[test]
fn append_memory_rejects_person_types() {
    let root = tempfile::tempdir().unwrap();
    let err = append_memory(
        root.path(),
        "ios",
        TYPE_PERSON_HIDDEN,
        r#"{"path":"People/Ada"}"#,
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("unknown memory event type"),
        "{err}"
    );
    assert!(read_all(root.path()).unwrap().is_empty());
}

#[test]
fn append_person_rejects_memory_types() {
    let root = tempfile::tempdir().unwrap();
    let err = append_person(
        root.path(),
        "ios",
        TYPE_MEMORY_HIDDEN,
        r#"{"id":"onThisDay-2024-06-11"}"#,
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("unknown person event type"),
        "{err}"
    );
    assert!(is_memory_event_type(TYPE_MEMORY_HIDDEN));
    assert!(!is_person_event_type(TYPE_MEMORY_HIDDEN));
}

#[test]
fn append_memory_then_project() {
    let root = tempfile::tempdir().unwrap();
    append_memory(
        root.path(),
        "ios",
        TYPE_MEMORY_HIDDEN,
        r#"{"id":"folder-aaaa"}"#,
    )
    .unwrap();
    append_memory(
        root.path(),
        "ios",
        TYPE_MEMORY_SEEN,
        r#"{"id":"folder-aaaa","at":1718100000}"#,
    )
    .unwrap();
    let state = project_memories_at(root.path()).unwrap();
    assert!(state.hidden.contains("folder-aaaa"));
    assert_eq!(
        state.seen.get("folder-aaaa").map(String::as_str),
        Some("1718100000")
    );
}

#[test]
fn memory_events_are_ignored_by_project_people_helper() {
    let events = vec![ev(
        "01900000-0000-7000-8000-0000000000e1",
        "2024-07-01T10:00:00.000000000Z",
        "ios",
        TYPE_MEMORY_HIDDEN,
        json!({"id": "onThisDay-2024-06-11"}),
    )];
    let people = project_people(&events);
    assert!(people.hidden.is_empty());
    assert!(people.featured.is_empty());
    let memories = project_memories(&events);
    assert!(memories.hidden.contains("onThisDay-2024-06-11"));
}
