//! People-rail policy and contact-link resolution for both shells.
//!
//! See all uses [`page_people`] (hidden last). The Collections rail uses
//! [`rail_people`] (hidden omitted). `ContactLinker.linkState` lives here so
//! GTK does not reimplement it.

use std::collections::{HashMap, HashSet};

use gallery_index::{
    page_people as order_page_people, visible_people as order_people, TagSuggestion,
    PEOPLE_RAIL_CAP,
};
use gallery_memories::{Contact, LinkState, PersonKeys, PersonLink};

use crate::library::{MemoryContactCommandItem, MemoryPersonCommandItem, TagStructureItem};

/// UI link kind. Dangling manual ids are [`Self::Unlinked`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PersonLinkKind {
    Unlinked,
    Disabled,
    Manual,
    Auto,
}

/// Display-ready link resolution for one `People/…` tag.
///
/// R6 role: command DTO.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PersonLinkResolution {
    pub kind: PersonLinkKind,
    pub contact_id: Option<String>,
    pub given_name: String,
    pub family_name: String,
}

impl PersonLinkResolution {
    fn from_state(state: LinkState) -> Self {
        match state {
            LinkState::Unlinked => Self {
                kind: PersonLinkKind::Unlinked,
                contact_id: None,
                given_name: String::new(),
                family_name: String::new(),
            },
            LinkState::Disabled => Self {
                kind: PersonLinkKind::Disabled,
                contact_id: None,
                given_name: String::new(),
                family_name: String::new(),
            },
            LinkState::Manual(contact) => Self::of(PersonLinkKind::Manual, contact),
            LinkState::Auto(contact) => Self::of(PersonLinkKind::Auto, contact),
        }
    }

    fn of(kind: PersonLinkKind, contact: Contact) -> Self {
        Self {
            kind,
            contact_id: Some(contact.id),
            given_name: contact.given_name,
            family_name: contact.family_name,
        }
    }
}

/// `ContactLinker.linkState` — same indexes memories use.
#[uniffi::export]
pub fn person_link_state(
    person_path: String,
    display_name: String,
    contacts: Vec<MemoryContactCommandItem>,
    links: Vec<MemoryPersonCommandItem>,
) -> PersonLinkResolution {
    let contacts: Vec<Contact> = contacts.into_iter().map(contact_from_item).collect();
    let links = links_map(links);
    let keys = PersonKeys::from_parts(&contacts, &links, &HashSet::new(), "");
    PersonLinkResolution::from_state(keys.link_state(&person_path, &display_name))
}

/// `PeopleStore.orderedVisiblePeople`.
#[uniffi::export]
pub fn visible_people(
    people: Vec<TagStructureItem>,
    hidden: Vec<String>,
    featured: Vec<String>,
    now: f64,
    recency_gated: bool,
    cap: Option<u32>,
) -> Vec<TagStructureItem> {
    let suggestions: Vec<TagSuggestion> = people.iter().map(suggestion_from_item).collect();
    order_people(
        &suggestions,
        &hidden,
        &featured,
        now,
        recency_gated,
        cap.map(|n| n as usize),
    )
    .into_iter()
    .map(|person| TagStructureItem::of(&person))
    .collect()
}

pub(crate) fn rail_people(
    people: &[TagSuggestion],
    hidden: &[String],
    featured: &[String],
    now: f64,
) -> Vec<TagSuggestion> {
    order_people(people, hidden, featured, now, true, Some(PEOPLE_RAIL_CAP))
}

pub(crate) fn page_people(
    people: &[TagSuggestion],
    hidden: &[String],
    featured: &[String],
) -> Vec<TagSuggestion> {
    order_page_people(people, hidden, featured)
}

fn contact_from_item(item: MemoryContactCommandItem) -> Contact {
    Contact {
        id: item.id,
        given_name: item.given_name,
        family_name: item.family_name,
        birthday_month: item.birthday_month,
        birthday_day: item.birthday_day,
    }
}

fn links_map(links: Vec<MemoryPersonCommandItem>) -> HashMap<String, PersonLink> {
    links
        .into_iter()
        .map(|link| {
            let value = match link.contact_id {
                Some(id) => PersonLink::Manual(id),
                None => PersonLink::Disabled,
            };
            (link.person_path, value)
        })
        .collect()
}

fn suggestion_from_item(item: &TagStructureItem) -> TagSuggestion {
    TagSuggestion {
        id: item.id.clone(),
        display_name: item.display_name.clone(),
        full_path: item.full_path.clone(),
        namespace: item.namespace.clone(),
        count: item.count as usize,
        latest_photo_date: item.latest_photo_date,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ada() -> MemoryContactCommandItem {
        MemoryContactCommandItem {
            id: "vcf:ada".into(),
            given_name: "Ada".into(),
            family_name: "Lovelace".into(),
            birthday_month: Some(12),
            birthday_day: Some(10),
        }
    }

    #[test]
    fn link_state_auto_manual_disabled_and_dangling() {
        let contacts = vec![ada()];
        let auto = person_link_state(
            "People/Ada Lovelace".into(),
            "Ada Lovelace".into(),
            contacts.clone(),
            Vec::new(),
        );
        assert_eq!(auto.kind, PersonLinkKind::Auto);
        assert_eq!(auto.contact_id.as_deref(), Some("vcf:ada"));

        let manual = person_link_state(
            "People/Ada".into(),
            "Ada".into(),
            contacts.clone(),
            vec![MemoryPersonCommandItem {
                person_path: "People/Ada".into(),
                contact_id: Some("vcf:ada".into()),
            }],
        );
        assert_eq!(manual.kind, PersonLinkKind::Manual);

        let disabled = person_link_state(
            "People/Ada Lovelace".into(),
            "Ada Lovelace".into(),
            contacts.clone(),
            vec![MemoryPersonCommandItem {
                person_path: "People/Ada Lovelace".into(),
                contact_id: None,
            }],
        );
        assert_eq!(disabled.kind, PersonLinkKind::Disabled);

        let dangling = person_link_state(
            "People/Ada Lovelace".into(),
            "Ada Lovelace".into(),
            contacts,
            vec![MemoryPersonCommandItem {
                person_path: "People/Ada Lovelace".into(),
                contact_id: Some("CN:missing".into()),
            }],
        );
        assert_eq!(dangling.kind, PersonLinkKind::Unlinked);
    }

    fn person(name: &str) -> TagSuggestion {
        TagSuggestion {
            id: format!("people/{}", name.to_lowercase()),
            display_name: name.into(),
            full_path: format!("People/{name}"),
            namespace: Some("People".into()),
            count: 1,
            latest_photo_date: Some(400.0),
        }
    }

    #[test]
    fn page_keeps_hidden_at_the_end() {
        let people = vec![person("Cy"), person("Ada"), person("Bob")];
        let listed = page_people(&people, &["People/Cy".into()], &["People/Bob".into()]);
        assert_eq!(
            listed
                .iter()
                .map(|person| person.full_path.as_str())
                .collect::<Vec<_>>(),
            vec!["People/Bob", "People/Ada", "People/Cy"]
        );
        let rail = rail_people(
            &people,
            &["People/Cy".into()],
            &["People/Bob".into()],
            500.0,
        );
        assert!(!rail.iter().any(|person| person.full_path == "People/Cy"));
        assert_eq!(rail[0].full_path, "People/Bob");
    }
}
