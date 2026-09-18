//! Gallery memory-chrome projection over the shared event log.
//!
//! Folds operation events (`memory_hidden`, `memory_seen`, …) into the
//! decisions MemoryCoordinator used to keep in UserDefaults. Same on-disk
//! layout as person-state: `{library}/.gallery/log/<dev>/YYYY-MM.ndjson`.
//! This crate sees `{library}/.gallery` as `root`.
//!
//! Types and projection are independent of [`crate::gallery`]: memory
//! events are not person events, and [`crate::gallery::project_people`]
//! ignores unknown types. Append goes through [`append_memory`], never
//! [`crate::gallery::append_person`].
//!
//! M2 imports a one-shot dump of the five UserDefaults keys via
//! [`migrate_memories_from_snapshot_json`]. The one-shot token is a
//! `memory_migrated` marker written last: if it is missing, migrate may
//! retry and skips snapshot events already present (same type + body
//! keys) so a partial write is not made permanent. There is no memory
//! rename event; `person_renamed` does not rewrite memory ids.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{json, Value};

use crate::event::{
    known_type, new_event_id, now_utc, ts_with_nanos, valid_device, Event,
    TYPE_MEMORY_BIRTHDAYS_SET, TYPE_MEMORY_CLUSTER_SURFACED, TYPE_MEMORY_GENERATED_DAY,
    TYPE_MEMORY_GENERATED_DAY_CLEAR, TYPE_MEMORY_HIDDEN, TYPE_MEMORY_MIGRATED, TYPE_MEMORY_SEEN,
    TYPE_MEMORY_UNHIDDEN,
};
use crate::{append, read_report, Error, Result, TornTailDiagnostic};

/// Projected memory-chrome state after replaying a log.
///
/// `birthdays_enabled` defaults to `true` when no `memory_birthdays_set`
/// event has been seen. `seen` / `surfaced` values are the `at` timestamp
/// from the event body (RFC3339 or unix), stored as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryState {
    pub hidden: BTreeSet<String>,
    pub seen: BTreeMap<String, String>,
    pub surfaced: BTreeMap<String, String>,
    pub birthdays_enabled: bool,
    pub generated_day: Option<String>,
}

impl Default for MemoryState {
    fn default() -> Self {
        Self {
            hidden: BTreeSet::new(),
            seen: BTreeMap::new(),
            surfaced: BTreeMap::new(),
            birthdays_enabled: true,
            generated_day: None,
        }
    }
}

/// Memory chrome recovered from every complete event plus any torn
/// final-line diagnostics. A torn tail is not a reason to resurrect an
/// older snapshot: the complete append-only prefix remains authoritative.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryProjection {
    pub state: MemoryState,
    pub torn_tails: Vec<TornTailDiagnostic>,
}

/// Parsed dump of the five MemoryCoordinator UserDefaults keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemorySnapshot {
    pub hidden: Vec<String>,
    pub seen: BTreeMap<String, String>,
    pub surfaced: BTreeMap<String, String>,
    /// `None` when the dump omitted the key (projection default is true).
    pub birthdays_enabled: Option<bool>,
    pub generated_day: Option<String>,
}

impl MemorySnapshot {
    /// Parse a JSON object with the UserDefaults key names (or the shorter
    /// projection aliases `hidden` / `seen` / `surfaced` / `birthdays` /
    /// `generated_day`).
    pub fn parse(json: &str) -> Result<Self> {
        let value: Value = serde_json::from_str(json)?;
        Self::from_value(&value)
    }

    pub fn from_value(value: &Value) -> Result<Self> {
        let obj = value
            .as_object()
            .ok_or_else(|| Error::Invalid("snapshot must be a JSON object".into()))?;
        let hidden = string_list(
            obj.get("hiddenMemories")
                .or_else(|| obj.get("hidden"))
                .unwrap_or(&Value::Null),
        );
        let seen = timestamp_map(
            obj.get("seenMemoryIDs")
                .or_else(|| obj.get("seen"))
                .unwrap_or(&Value::Null),
        );
        let surfaced = timestamp_map(
            obj.get("surfacedClusters")
                .or_else(|| obj.get("surfaced"))
                .unwrap_or(&Value::Null),
        );
        let birthdays_enabled = bool_opt(
            obj.get("birthdayMemoriesEnabled")
                .or_else(|| obj.get("birthdays"))
                .unwrap_or(&Value::Null),
        );
        let generated_day = timestamp_opt(
            obj.get("memoriesGeneratedDay")
                .or_else(|| obj.get("generated_day"))
                .unwrap_or(&Value::Null),
        );
        Ok(Self {
            hidden,
            seen,
            surfaced,
            birthdays_enabled,
            generated_day,
        })
    }

    /// The MemoryState this dump projects to (no later operation events).
    pub fn to_memory_state(&self) -> MemoryState {
        MemoryState {
            hidden: self.hidden.iter().cloned().collect(),
            seen: self.seen.clone(),
            surfaced: self.surfaced.clone(),
            birthdays_enabled: self.birthdays_enabled.unwrap_or(true),
            generated_day: self.generated_day.clone(),
        }
    }
}

fn string_list(v: &Value) -> Vec<String> {
    match v {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().filter(|s| !s.is_empty()).map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn timestamp_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        }
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(i.to_string())
            } else if let Some(f) = n.as_f64() {
                if f.is_finite() {
                    Some(format_number(f))
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

fn format_number(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < (i64::MAX as f64) {
        format!("{}", f as i64)
    } else {
        format!("{f}")
    }
}

fn timestamp_opt(v: &Value) -> Option<String> {
    timestamp_string(v)
}

fn timestamp_map(v: &Value) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(obj) = v.as_object() else {
        return out;
    };
    for (k, val) in obj {
        if k.is_empty() {
            continue;
        }
        if let Some(ts) = timestamp_string(val) {
            out.insert(k.clone(), ts);
        }
    }
    out
}

fn bool_opt(v: &Value) -> Option<bool> {
    v.as_bool()
}

/// Whether `t` is a gallery memory-chrome operation.
pub fn is_memory_event_type(t: &str) -> bool {
    matches!(
        t,
        TYPE_MEMORY_HIDDEN
            | TYPE_MEMORY_UNHIDDEN
            | TYPE_MEMORY_SEEN
            | TYPE_MEMORY_CLUSTER_SURFACED
            | TYPE_MEMORY_BIRTHDAYS_SET
            | TYPE_MEMORY_GENERATED_DAY
            | TYPE_MEMORY_GENERATED_DAY_CLEAR
            | TYPE_MEMORY_MIGRATED
    )
}

/// Append one memory-chrome event, filling id and ts.
///
/// Does not consult [`crate::gallery::is_person_event_type`]. Memory
/// types are not person types and must not be routed through
/// [`crate::gallery::append_person`].
pub fn append_memory(
    root: impl AsRef<Path>,
    device: &str,
    event_type: &str,
    body_json: &str,
) -> Result<()> {
    if !valid_device(device) {
        return Err(Error::Invalid(format!("invalid device {device:?}")));
    }
    if !known_type(event_type) || !is_memory_event_type(event_type) {
        return Err(Error::Invalid(format!(
            "unknown memory event type {event_type:?}"
        )));
    }
    let body: Value = serde_json::from_str(body_json)?;
    if !body.is_object() {
        return Err(Error::Invalid("body must be a JSON object".into()));
    }
    let root = root.as_ref();
    let report = read_report(root)?;
    if let Some(torn) = report.torn_tails.into_iter().next() {
        return Err(Error::TornTail(torn));
    }
    append(root, &Event::fresh(device, event_type, body))
}

/// Read the log at `root` and fold memory-chrome events.
pub fn project_memories_at(root: impl AsRef<Path>) -> Result<MemoryState> {
    Ok(project_memories_report_at(root)?.state)
}

/// Read the log while preserving torn-tail diagnostics. Complete events
/// are still projected; callers must surface the diagnostic and must not
/// fall back to a stale snapshot.
pub fn project_memories_report_at(root: impl AsRef<Path>) -> Result<MemoryProjection> {
    let report = read_report(root)?;
    Ok(MemoryProjection {
        state: project_memories(&report.events),
        torn_tails: report
            .torn_tails
            .iter()
            .map(TornTailDiagnostic::from)
            .collect(),
    })
}

/// Import a UserDefaults dump as operations. No-op if `device` already wrote
/// a `memory_migrated` marker (one-shot). A missing marker may retry.
pub fn migrate_memories_from_snapshot_json(
    root: impl AsRef<Path>,
    device: &str,
    snapshot_json: &str,
) -> Result<usize> {
    migrate_memories_from_snapshot(root, device, &MemorySnapshot::parse(snapshot_json)?)
}

/// Import a parsed snapshot. Returns the number of events appended,
/// including the `memory_migrated` marker written last.
///
/// The marker is the one-shot token. If it is absent, a retry writes only
/// snapshot events that are not already present for this device (same
/// type and body keys/values) so a crash after a partial write cannot
/// duplicate hidden / seen / surfaced entries, then writes the marker.
pub fn migrate_memories_from_snapshot(
    root: impl AsRef<Path>,
    device: &str,
    snapshot: &MemorySnapshot,
) -> Result<usize> {
    if !valid_device(device) {
        return Err(Error::Invalid(format!("invalid device {device:?}")));
    }
    let report = read_report(root.as_ref())?;
    if let Some(torn) = report.torn_tails.into_iter().next() {
        return Err(Error::TornTail(torn));
    }
    let existing = report.events;
    if existing
        .iter()
        .any(|e| e.dev == device && e.event_type == TYPE_MEMORY_MIGRATED)
    {
        return Ok(0);
    }
    let events = snapshot_events(device, snapshot)?;
    let mut written = 0usize;
    for ev in &events {
        if already_written_snapshot_event(&existing, device, ev) {
            continue;
        }
        append(root.as_ref(), ev)?;
        written += 1;
    }
    append(
        root.as_ref(),
        &Event::fresh(device, TYPE_MEMORY_MIGRATED, json!({})),
    )?;
    written += 1;
    Ok(written)
}

/// True when this device already logged an event of the same type whose
/// body contains every incoming body key with the same value.
fn already_written_snapshot_event(existing: &[Event], device: &str, incoming: &Event) -> bool {
    existing.iter().any(|e| {
        e.dev == device
            && e.event_type == incoming.event_type
            && body_keys_match(&e.body, &incoming.body)
    })
}

fn body_keys_match(have: &Value, want: &Value) -> bool {
    let Some(want) = want.as_object() else {
        return false;
    };
    let Some(have) = have.as_object() else {
        return false;
    };
    want.iter().all(|(k, v)| have.get(k) == Some(v))
}

fn snapshot_events(device: &str, snapshot: &MemorySnapshot) -> Result<Vec<Event>> {
    let mut bodies: Vec<(&str, Value)> = Vec::new();
    for id in &snapshot.hidden {
        bodies.push((TYPE_MEMORY_HIDDEN, json!({"id": id})));
    }
    for (id, at) in &snapshot.seen {
        bodies.push((TYPE_MEMORY_SEEN, json!({"id": id, "at": at})));
    }
    for (key, at) in &snapshot.surfaced {
        bodies.push((TYPE_MEMORY_CLUSTER_SURFACED, json!({"key": key, "at": at})));
    }
    if let Some(enabled) = snapshot.birthdays_enabled {
        bodies.push((TYPE_MEMORY_BIRTHDAYS_SET, json!({"enabled": enabled})));
    }
    if let Some(day) = &snapshot.generated_day {
        bodies.push((TYPE_MEMORY_GENERATED_DAY, json!({"day": day})));
    }
    let base = now_utc();
    let mut out = Vec::with_capacity(bodies.len());
    for (i, (typ, body)) in bodies.into_iter().enumerate() {
        let ts = ts_with_nanos(&base, i as u32)?;
        out.push(Event::new(new_event_id(), ts, device, typ, body));
    }
    Ok(out)
}

fn body_str<'a>(ev: &'a Event, key: &str) -> Option<&'a str> {
    ev.body.get(key).and_then(|v| v.as_str())
}

fn body_timestamp(ev: &Event, key: &str) -> Option<String> {
    ev.body.get(key).and_then(timestamp_string)
}

/// Fold gallery memory-chrome operation events into a projection.
///
/// Person events, including `person_renamed`, are ignored: memory ids are
/// not path-keyed people and a rename is not applicable.
pub fn project_memories(events: &[Event]) -> MemoryState {
    let mut state = MemoryState::default();
    for ev in events {
        match ev.event_type.as_str() {
            TYPE_MEMORY_HIDDEN => {
                if let Some(id) = body_str(ev, "id") {
                    state.hidden.insert(id.to_string());
                }
            }
            TYPE_MEMORY_UNHIDDEN => {
                if let Some(id) = body_str(ev, "id") {
                    state.hidden.remove(id);
                }
            }
            TYPE_MEMORY_SEEN => {
                if let Some(id) = body_str(ev, "id") {
                    if let Some(at) = body_timestamp(ev, "at") {
                        state.seen.insert(id.to_string(), at);
                    }
                }
            }
            TYPE_MEMORY_CLUSTER_SURFACED => {
                if let Some(key) = body_str(ev, "key") {
                    if let Some(at) = body_timestamp(ev, "at") {
                        state.surfaced.insert(key.to_string(), at);
                    }
                }
            }
            TYPE_MEMORY_BIRTHDAYS_SET => {
                if let Some(enabled) = ev.body.get("enabled").and_then(Value::as_bool) {
                    state.birthdays_enabled = enabled;
                }
            }
            TYPE_MEMORY_GENERATED_DAY => {
                if let Some(day) = body_timestamp(ev, "day") {
                    state.generated_day = Some(day);
                }
            }
            TYPE_MEMORY_GENERATED_DAY_CLEAR => {
                state.generated_day = None;
            }
            _ => {}
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{
        TYPE_MEMORY_BIRTHDAYS_SET, TYPE_MEMORY_CLUSTER_SURFACED, TYPE_MEMORY_GENERATED_DAY,
        TYPE_MEMORY_GENERATED_DAY_CLEAR, TYPE_MEMORY_HIDDEN, TYPE_MEMORY_MIGRATED,
        TYPE_MEMORY_SEEN, TYPE_MEMORY_UNHIDDEN, TYPE_PERSON_HIDDEN, TYPE_PERSON_RENAMED,
    };
    use crate::gallery::is_person_event_type;

    #[test]
    fn memory_types_are_not_person_types() {
        for t in [
            TYPE_MEMORY_HIDDEN,
            TYPE_MEMORY_UNHIDDEN,
            TYPE_MEMORY_SEEN,
            TYPE_MEMORY_CLUSTER_SURFACED,
            TYPE_MEMORY_BIRTHDAYS_SET,
            TYPE_MEMORY_GENERATED_DAY,
            TYPE_MEMORY_GENERATED_DAY_CLEAR,
            TYPE_MEMORY_MIGRATED,
        ] {
            assert!(is_memory_event_type(t), "{t}");
            assert!(known_type(t), "{t}");
            assert!(!is_person_event_type(t), "{t} must not be a person type");
        }
        assert!(!is_memory_event_type(TYPE_PERSON_HIDDEN));
        assert!(!is_memory_event_type(TYPE_PERSON_RENAMED));
    }

    #[test]
    fn snapshot_accepts_userdefaults_keys_and_unix() {
        let snap = MemorySnapshot::parse(
            r#"{
                "hiddenMemories": ["a"],
                "seenMemoryIDs": {"a": 1718100000},
                "surfacedClusters": {"k": "2024-06-11T12:00:00Z"},
                "birthdayMemoriesEnabled": false,
                "memoriesGeneratedDay": "2024-06-11"
            }"#,
        )
        .unwrap();
        assert_eq!(snap.hidden, vec!["a".to_string()]);
        assert_eq!(snap.seen.get("a").map(String::as_str), Some("1718100000"));
        assert_eq!(
            snap.surfaced.get("k").map(String::as_str),
            Some("2024-06-11T12:00:00Z")
        );
        assert_eq!(snap.birthdays_enabled, Some(false));
        assert_eq!(snap.generated_day.as_deref(), Some("2024-06-11"));
        assert!(!snap.to_memory_state().birthdays_enabled);
    }

    #[test]
    fn missing_birthdays_key_defaults_true() {
        let snap = MemorySnapshot::parse(r#"{"hiddenMemories":[]}"#).unwrap();
        assert_eq!(snap.birthdays_enabled, None);
        assert!(snap.to_memory_state().birthdays_enabled);
    }
}
