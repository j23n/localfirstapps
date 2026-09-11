//! Gallery person-state projection over the shared event log.
//!
//! Folds operation events (`person_hidden`, `person_renamed`, …) into the
//! path-keyed decisions PeopleStore used to keep in UserDefaults (ADR 0005
//! R13/R14). Gallery stores the log at
//! `{library}/.gallery/log/<dev>/YYYY-MM.ndjson`: this crate sees
//! `{library}/.gallery` as `root` and writes `log/<dev>/YYYY-MM.ndjson`.
//!
//! M2 imports a one-shot dump of the five UserDefaults keys via
//! [`migrate_from_snapshot_json`]. A `person_renamed` event is how a rename
//! migrates every device, replacing `PeopleStore.renamePerson` /
//! `GalleryStore.migratePersonState` snapshot rewriting.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{json, Value};

use crate::event::{
    known_type, new_event_id, now_utc, ts_with_nanos, valid_device, Event,
    TYPE_FEATURED_PHOTO_CLEAR, TYPE_FEATURED_PHOTO_SET, TYPE_PERSON_CONTACT_LINK_CLEAR,
    TYPE_PERSON_CONTACT_LINK_SET, TYPE_PERSON_FEATURED, TYPE_PERSON_HIDDEN, TYPE_PERSON_ME_CLEAR,
    TYPE_PERSON_ME_SET, TYPE_PERSON_RENAMED, TYPE_PERSON_UNFEATURED, TYPE_PERSON_UNHIDDEN,
};
use crate::{append, read_all, Error, Result};

/// Projected people-rail state after replaying a log.
///
/// `links` values: a non-empty contact id is `PersonLink.manual`; an empty
/// string is `PersonLink.disabled`. Absence means auto-match by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PeopleState {
    pub hidden: BTreeSet<String>,
    pub featured: Vec<String>,
    pub me: Option<String>,
    pub featured_photo: BTreeMap<String, String>,
    pub links: BTreeMap<String, String>,
}

/// A contact-link decision from a pre-M2 UserDefaults dump.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkDecision {
    Manual(String),
    Disabled,
}

/// Parsed dump of the five PeopleStore / GalleryStore UserDefaults keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersonSnapshot {
    pub hidden: Vec<String>,
    pub featured: Vec<String>,
    pub me: Option<String>,
    pub featured_photo: BTreeMap<String, String>,
    pub links: BTreeMap<String, LinkDecision>,
}

impl PersonSnapshot {
    /// Parse a JSON object with the UserDefaults key names (or the shorter
    /// projection aliases `hidden` / `featured` / `me` / `featured_photo` /
    /// `links`).
    pub fn parse(json: &str) -> Result<Self> {
        let value: Value = serde_json::from_str(json)?;
        Self::from_value(&value)
    }

    pub fn from_value(value: &Value) -> Result<Self> {
        let obj = value
            .as_object()
            .ok_or_else(|| Error::Invalid("snapshot must be a JSON object".into()))?;
        let hidden = string_list(
            obj.get("hiddenPeople")
                .or_else(|| obj.get("hidden"))
                .unwrap_or(&Value::Null),
        );
        let featured = string_list(
            obj.get("pinnedPeople")
                .or_else(|| obj.get("featured"))
                .unwrap_or(&Value::Null),
        );
        let me = string_opt(
            obj.get("mePersonPath")
                .or_else(|| obj.get("me"))
                .unwrap_or(&Value::Null),
        );
        let featured_photo = string_map(
            obj.get("featuredPhotoByPerson")
                .or_else(|| obj.get("featured_photo"))
                .unwrap_or(&Value::Null),
        );
        let links = parse_links(
            obj.get("personContactLinks")
                .or_else(|| obj.get("links"))
                .unwrap_or(&Value::Null),
        )?;
        Ok(Self {
            hidden,
            featured,
            me,
            featured_photo,
            links,
        })
    }

    /// The PeopleState this dump projects to (no rename events).
    pub fn to_people_state(&self) -> PeopleState {
        PeopleState {
            hidden: self.hidden.iter().cloned().collect(),
            featured: self.featured.clone(),
            me: self.me.clone(),
            featured_photo: self.featured_photo.clone(),
            links: self
                .links
                .iter()
                .map(|(path, link)| {
                    (
                        path.clone(),
                        match link {
                            LinkDecision::Manual(id) => id.clone(),
                            LinkDecision::Disabled => String::new(),
                        },
                    )
                })
                .collect(),
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

fn string_opt(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn string_map(v: &Value) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(obj) = v.as_object() else {
        return out;
    };
    for (k, val) in obj {
        if let Some(s) = val.as_str().filter(|s| !s.is_empty()) {
            out.insert(k.clone(), s.to_string());
        }
    }
    out
}

fn parse_links(v: &Value) -> Result<BTreeMap<String, LinkDecision>> {
    let mut out = BTreeMap::new();
    let Some(obj) = v.as_object() else {
        return Ok(out);
    };
    for (path, val) in obj {
        if let Some(link) = parse_link(val) {
            out.insert(path.clone(), link);
        }
    }
    Ok(out)
}

/// Swift `PersonLink` Codable, a bare contact-id string, or `{disabled:true}`.
fn parse_link(v: &Value) -> Option<LinkDecision> {
    match v {
        Value::String(s) if s.is_empty() || s == "disabled" => Some(LinkDecision::Disabled),
        Value::String(s) => Some(LinkDecision::Manual(s.clone())),
        Value::Object(map) => {
            if map.get("disabled").and_then(Value::as_bool) == Some(true) {
                return Some(LinkDecision::Disabled);
            }
            if let Some(Value::Object(inner)) = map.get("disabled") {
                if map.get("manual").is_none() && inner.is_empty() {
                    return Some(LinkDecision::Disabled);
                }
            }
            if let Some(Value::Object(inner)) = map.get("manual") {
                if let Some(id) = inner
                    .get("contactID")
                    .or_else(|| inner.get("contact_id"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    return Some(LinkDecision::Manual(id.to_string()));
                }
            }
            if map.get("contact_id").map(Value::is_null).unwrap_or(false) {
                return Some(LinkDecision::Disabled);
            }
            if let Some(id) = map
                .get("contact")
                .or_else(|| map.get("contact_id"))
                .or_else(|| map.get("contactID"))
                .and_then(Value::as_str)
            {
                return if id.is_empty() {
                    Some(LinkDecision::Disabled)
                } else {
                    Some(LinkDecision::Manual(id.to_string()))
                };
            }
            None
        }
        _ => None,
    }
}

/// Whether `t` is a gallery person-state operation.
pub fn is_person_event_type(t: &str) -> bool {
    matches!(
        t,
        TYPE_PERSON_HIDDEN
            | TYPE_PERSON_UNHIDDEN
            | TYPE_PERSON_FEATURED
            | TYPE_PERSON_UNFEATURED
            | TYPE_FEATURED_PHOTO_SET
            | TYPE_FEATURED_PHOTO_CLEAR
            | TYPE_PERSON_ME_SET
            | TYPE_PERSON_ME_CLEAR
            | TYPE_PERSON_RENAMED
            | TYPE_PERSON_CONTACT_LINK_SET
            | TYPE_PERSON_CONTACT_LINK_CLEAR
    )
}

/// Append one person-state event, filling id and ts.
pub fn append_person(
    root: impl AsRef<Path>,
    device: &str,
    event_type: &str,
    body_json: &str,
) -> Result<()> {
    if !valid_device(device) {
        return Err(Error::Invalid(format!("invalid device {device:?}")));
    }
    if !known_type(event_type) || !is_person_event_type(event_type) {
        return Err(Error::Invalid(format!(
            "unknown person event type {event_type:?}"
        )));
    }
    let body: Value = serde_json::from_str(body_json)?;
    if !body.is_object() {
        return Err(Error::Invalid("body must be a JSON object".into()));
    }
    append(root, &Event::fresh(device, event_type, body))
}

/// Read the log at `root` and fold person-state events.
pub fn project_people_at(root: impl AsRef<Path>) -> Result<PeopleState> {
    Ok(project_people(&read_all(root)?))
}

/// Import a UserDefaults dump as operations. No-op if `device` already wrote
/// any person-state event (one-shot).
pub fn migrate_from_snapshot_json(
    root: impl AsRef<Path>,
    device: &str,
    snapshot_json: &str,
) -> Result<usize> {
    migrate_from_snapshot(root, device, &PersonSnapshot::parse(snapshot_json)?)
}

/// Import a parsed snapshot. Returns the number of events appended.
pub fn migrate_from_snapshot(
    root: impl AsRef<Path>,
    device: &str,
    snapshot: &PersonSnapshot,
) -> Result<usize> {
    if !valid_device(device) {
        return Err(Error::Invalid(format!("invalid device {device:?}")));
    }
    let existing = read_all(root.as_ref())?;
    if existing
        .iter()
        .any(|e| e.dev == device && is_person_event_type(&e.event_type))
    {
        return Ok(0);
    }
    let events = snapshot_events(device, snapshot)?;
    for ev in &events {
        append(root.as_ref(), ev)?;
    }
    Ok(events.len())
}

fn snapshot_events(device: &str, snapshot: &PersonSnapshot) -> Result<Vec<Event>> {
    let mut bodies: Vec<(&str, Value)> = Vec::new();
    // Featured before hidden so an inconsistent dump that lists both still
    // matches hidePerson (hidden wins).
    for path in &snapshot.featured {
        bodies.push((TYPE_PERSON_FEATURED, json!({"path": path})));
    }
    for (path, photo) in &snapshot.featured_photo {
        bodies.push((
            TYPE_FEATURED_PHOTO_SET,
            json!({"path": path, "photo": photo}),
        ));
    }
    if let Some(path) = &snapshot.me {
        bodies.push((TYPE_PERSON_ME_SET, json!({"path": path})));
    }
    for (path, link) in &snapshot.links {
        let body = match link {
            LinkDecision::Manual(id) => json!({"path": path, "contact": id}),
            LinkDecision::Disabled => json!({"path": path, "disabled": true}),
        };
        bodies.push((TYPE_PERSON_CONTACT_LINK_SET, body));
    }
    for path in &snapshot.hidden {
        bodies.push((TYPE_PERSON_HIDDEN, json!({"path": path})));
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

fn path_of(ev: &Event) -> Option<&str> {
    body_str(ev, "path")
}

fn rename_key(set: &mut BTreeSet<String>, from: &str, to: &str) {
    if set.remove(from) {
        set.insert(to.to_string());
    }
}

fn rename_featured(featured: &mut Vec<String>, from: &str, to: &str) {
    if let Some(i) = featured.iter().position(|p| p == from) {
        featured[i] = to.to_string();
        let mut seen = BTreeSet::new();
        featured.retain(|p| seen.insert(p.clone()));
    }
}

fn rename_map(map: &mut BTreeMap<String, String>, from: &str, to: &str) {
    if let Some(val) = map.remove(from) {
        map.entry(to.to_string()).or_insert(val);
    }
}

/// Fold gallery person-state operation events into a projection.
///
/// Rename migrates hidden / featured / me / featured photo / contact links
/// the way `PeopleStore.renamePerson` and `migratePersonState` do today:
/// hidden collapses as a set, featured keeps the older position, and a
/// colliding cover photo or link on the destination name wins.
pub fn project_people(events: &[Event]) -> PeopleState {
    let mut state = PeopleState::default();
    for ev in events {
        match ev.event_type.as_str() {
            TYPE_PERSON_HIDDEN => {
                if let Some(path) = path_of(ev) {
                    state.hidden.insert(path.to_string());
                    state.featured.retain(|p| p != path);
                }
            }
            TYPE_PERSON_UNHIDDEN => {
                if let Some(path) = path_of(ev) {
                    state.hidden.remove(path);
                }
            }
            TYPE_PERSON_FEATURED => {
                if let Some(path) = path_of(ev) {
                    if !state.featured.iter().any(|p| p == path) {
                        state.featured.push(path.to_string());
                    }
                }
            }
            TYPE_PERSON_UNFEATURED => {
                if let Some(path) = path_of(ev) {
                    state.featured.retain(|p| p != path);
                }
            }
            TYPE_FEATURED_PHOTO_SET => {
                if let Some(path) = path_of(ev) {
                    let photo = body_str(ev, "photo")
                        .or_else(|| body_str(ev, "photo_id"))
                        .unwrap_or("");
                    if !photo.is_empty() {
                        state
                            .featured_photo
                            .insert(path.to_string(), photo.to_string());
                    }
                }
            }
            TYPE_FEATURED_PHOTO_CLEAR => {
                if let Some(path) = path_of(ev) {
                    state.featured_photo.remove(path);
                }
            }
            TYPE_PERSON_ME_SET => {
                if let Some(path) = path_of(ev) {
                    state.me = Some(path.to_string());
                }
            }
            TYPE_PERSON_ME_CLEAR => {
                state.me = None;
            }
            TYPE_PERSON_RENAMED => {
                let from = body_str(ev, "from").unwrap_or("");
                let to = body_str(ev, "to").unwrap_or("");
                if from.is_empty() || to.is_empty() || from == to {
                    continue;
                }
                rename_key(&mut state.hidden, from, to);
                rename_featured(&mut state.featured, from, to);
                rename_map(&mut state.featured_photo, from, to);
                rename_map(&mut state.links, from, to);
                if state.me.as_deref() == Some(from) {
                    state.me = Some(to.to_string());
                }
            }
            TYPE_PERSON_CONTACT_LINK_SET => {
                if let Some(path) = path_of(ev) {
                    let disabled = ev.body.get("disabled").and_then(|v| v.as_bool()) == Some(true);
                    let contact = body_str(ev, "contact")
                        .or_else(|| body_str(ev, "contact_id"))
                        .unwrap_or("");
                    if disabled {
                        state.links.insert(path.to_string(), String::new());
                    } else if !contact.is_empty() {
                        state.links.insert(path.to_string(), contact.to_string());
                    }
                }
            }
            TYPE_PERSON_CONTACT_LINK_CLEAR => {
                if let Some(path) = path_of(ev) {
                    state.links.remove(path);
                }
            }
            _ => {}
        }
    }
    state
}
