//! Dual-consumer replay: gallery person-state ops on the shared log.
//!
//! Does not touch Swift PeopleStore or UserDefaults (that is M2).

use localcore_log::{
    append, project_people, read_all, Event, PeopleState, TYPE_FEATURED_PHOTO_SET,
    TYPE_PERSON_FEATURED, TYPE_PERSON_HIDDEN, TYPE_PERSON_ME_SET, TYPE_PERSON_RENAMED,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

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
