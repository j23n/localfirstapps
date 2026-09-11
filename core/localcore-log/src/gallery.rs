//! Gallery person-state projection over the shared event log.
//!
//! Folds operation events (`person_hidden`, `person_renamed`, …) into the
//! path-keyed decisions PeopleStore currently keeps in UserDefaults. This is
//! the dual-consumer proof for ADR 0005 R13/R14; wiring Swift PeopleStore
//! onto the log is M2 and is not done here.

use std::collections::{BTreeMap, BTreeSet};

use crate::event::{
    Event, TYPE_FEATURED_PHOTO_CLEAR, TYPE_FEATURED_PHOTO_SET, TYPE_PERSON_CONTACT_LINK_CLEAR,
    TYPE_PERSON_CONTACT_LINK_SET, TYPE_PERSON_FEATURED, TYPE_PERSON_HIDDEN, TYPE_PERSON_ME_CLEAR,
    TYPE_PERSON_ME_SET, TYPE_PERSON_RENAMED, TYPE_PERSON_UNFEATURED, TYPE_PERSON_UNHIDDEN,
};

/// Projected people-rail state after replaying a log.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PeopleState {
    pub hidden: BTreeSet<String>,
    pub featured: Vec<String>,
    pub me: Option<String>,
    pub featured_photo: BTreeMap<String, String>,
    pub links: BTreeMap<String, String>,
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
                    let contact = body_str(ev, "contact")
                        .or_else(|| body_str(ev, "contact_id"))
                        .unwrap_or("");
                    if !contact.is_empty() {
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
