//! Link-to-Contact picker helpers and sheet body.
//!
//! Sort + filter match iOS `ContactLinkSheet.filteredContacts`: A–Z by
//! display name (case-insensitive), then a substring query. Empty
//! given+family sorts after named contacts, then by id. GTK assembly
//! lives here so the parallel 5.10 `window.rs` rewrite only calls in.

use std::path::Path;
use std::rc::Rc;

use adw::prelude::*;
use gallery_ffi::MemoryContactCommandItem;
use shell_kit_gtk::{search_entry, status_row, StatusRowData, StatusSeverity};

const CONTACT_ROW_NAME: &str = "link-contact-choice";
const NO_MATCH_ROW_NAME: &str = "link-contact-no-match";

/// Widgets `present_link_contact` presents and wires to the session.
pub(crate) struct LinkContactSheet {
    pub body: gtk::Widget,
    pub reset: adw::ActionRow,
    pub disable: adw::ActionRow,
    pub choose: Option<adw::ActionRow>,
}

/// Row label: `given family`, trimmed. Same string the chooser shows.
#[must_use]
pub fn contact_display_name(contact: &MemoryContactCommandItem) -> String {
    format!("{} {}", contact.given_name, contact.family_name)
        .trim()
        .to_string()
}

/// A–Z by display name, case-insensitive. Nameless rows follow named
/// ones; ties (and two empty names) break on `id`.
pub fn sort_contacts_for_chooser(contacts: &mut [MemoryContactCommandItem]) {
    contacts.sort_by(|left, right| chooser_sort_key(left).cmp(&chooser_sort_key(right)));
}

/// Case-insensitive substring on display name. Empty / whitespace query
/// keeps every row (caller supplies sort order).
#[must_use]
pub fn filter_contacts_for_chooser<'a>(
    contacts: &'a [MemoryContactCommandItem],
    query: &str,
) -> Vec<&'a MemoryContactCommandItem> {
    let trimmed = query.trim().to_lowercase();
    if trimmed.is_empty() {
        return contacts.iter().collect();
    }
    contacts
        .iter()
        .filter(|contact| {
            contact_display_name(contact)
                .to_lowercase()
                .contains(&trimmed)
        })
        .collect()
}

/// Sort, then filter — iOS `filteredContacts`.
#[must_use]
pub fn chooser_contacts(
    contacts: &[MemoryContactCommandItem],
    query: &str,
) -> Vec<MemoryContactCommandItem> {
    let mut sorted = contacts.to_vec();
    sort_contacts_for_chooser(&mut sorted);
    filter_contacts_for_chooser(&sorted, query)
        .into_iter()
        .cloned()
        .collect()
}

/// Search + list for the Link-to-Contact sheet. Actions stay mounted;
/// only contact rows refill as the query changes.
pub(crate) fn sheet_body(
    contacts: Vec<MemoryContactCommandItem>,
    folder: Option<&Path>,
    on_pick: impl Fn(&str, &adw::Dialog) + Clone + 'static,
) -> LinkContactSheet {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);

    let search = search_entry();
    search.set_hexpand(true);
    search.set_placeholder_text(Some("Search contacts"));
    search.set_search_delay(0);
    search.set_margin_top(12);
    search.set_margin_start(18);
    search.set_margin_end(18);
    column.append(&search);

    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    list.set_margin_top(12);
    list.set_margin_bottom(18);
    list.set_margin_start(18);
    list.set_margin_end(18);

    let choose = if contacts.is_empty() {
        let message = if folder.is_none() {
            "Choose a Contacts folder to match people to .vcf files."
        } else {
            "No contacts in that folder."
        };
        list.append(&status_row(&StatusRowData {
            message: message.into(),
            severity: StatusSeverity::Info,
        }));
        let choose = choice_row("Choose Contacts Folder");
        list.append(&choose);
        Some(choose)
    } else {
        None
    };
    let reset = choice_row("Reset to auto-match");
    let disable = choice_row("Don't match a contact");
    list.append(&reset);
    list.append(&disable);

    let contacts = Rc::new(contacts);
    refill_contact_rows(&list, &contacts, "", on_pick.clone());
    let list_for_search = list.clone();
    search.connect_search_changed(move |entry| {
        refill_contact_rows(&list_for_search, &contacts, &entry.text(), on_pick.clone());
    });

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_min_content_height(280);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&list));
    column.append(&scroll);

    LinkContactSheet {
        body: column.upcast(),
        reset,
        disable,
        choose,
    }
}

fn chooser_sort_key(contact: &MemoryContactCommandItem) -> (bool, String, String) {
    let name = contact_display_name(contact);
    (name.is_empty(), name.to_lowercase(), contact.id.clone())
}

fn choice_row(title: &str) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_use_markup(false);
    row.set_title(title);
    row.set_activatable(true);
    row
}

fn refill_contact_rows(
    list: &gtk::ListBox,
    contacts: &[MemoryContactCommandItem],
    query: &str,
    on_pick: impl Fn(&str, &adw::Dialog) + Clone + 'static,
) {
    remove_named_rows(list, &[CONTACT_ROW_NAME, NO_MATCH_ROW_NAME]);
    if contacts.is_empty() {
        return;
    }
    let visible = chooser_contacts(contacts, query);
    if visible.is_empty() {
        let row = status_row(&StatusRowData {
            message: "No contacts match your search.".into(),
            severity: StatusSeverity::Info,
        });
        row.set_activatable(false);
        row.set_widget_name(NO_MATCH_ROW_NAME);
        list.append(&row);
        return;
    }
    for contact in visible {
        let row = choice_row(&contact_display_name(&contact));
        row.set_widget_name(CONTACT_ROW_NAME);
        let id = contact.id.clone();
        let on_pick = on_pick.clone();
        row.connect_activated(move |row| {
            if let Some(dialog) = row
                .ancestor(adw::Dialog::static_type())
                .and_then(|widget| widget.downcast::<adw::Dialog>().ok())
            {
                on_pick(&id, &dialog);
            }
        });
        list.append(&row);
    }
}

fn remove_named_rows(list: &gtk::ListBox, names: &[&str]) {
    let mut child = list.first_child();
    while let Some(widget) = child {
        let next = widget.next_sibling();
        if names
            .iter()
            .any(|name| widget.widget_name().as_str() == *name)
        {
            list.remove(&widget);
        }
        child = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contact(id: &str, given: &str, family: &str) -> MemoryContactCommandItem {
        MemoryContactCommandItem {
            id: id.into(),
            given_name: given.into(),
            family_name: family.into(),
            birthday_month: None,
            birthday_day: None,
        }
    }

    fn ids(contacts: &[MemoryContactCommandItem]) -> Vec<&str> {
        contacts.iter().map(|contact| contact.id.as_str()).collect()
    }

    #[test]
    fn unsorted_contacts_sort_a_to_z_by_display_name() {
        let contacts = [
            contact("z", "Zoe", "Mitchell"),
            contact("b", "bob", "Stone"),
            contact("a", "Ada", "Lovelace"),
        ];
        assert_eq!(ids(&chooser_contacts(&contacts, "")), ["a", "b", "z"]);
    }

    #[test]
    fn query_ada_keeps_ada_and_drops_bob() {
        let contacts = [
            contact("bob", "Bob", "Stone"),
            contact("ada", "Ada", "Lovelace"),
        ];
        let found = chooser_contacts(&contacts, "ada");
        assert_eq!(ids(&found), ["ada"]);
        assert_eq!(contact_display_name(&found[0]), "Ada Lovelace");
    }

    #[test]
    fn empty_query_returns_the_full_sorted_list() {
        let contacts = [
            contact("c", "Carmen", ""),
            contact("a", "Ada", ""),
            contact("b", "Bob", ""),
        ];
        for query in ["", "   ", "\t"] {
            assert_eq!(
                ids(&chooser_contacts(&contacts, query)),
                ["a", "b", "c"],
                "query {query:?}"
            );
        }
    }

    #[test]
    fn nameless_contacts_sort_after_named_then_by_id() {
        let contacts = [
            contact("z-empty", "", ""),
            contact("named", "Ada", ""),
            contact("a-empty", "  ", "  "),
        ];
        assert_eq!(
            ids(&chooser_contacts(&contacts, "")),
            ["named", "a-empty", "z-empty"]
        );
        assert!(chooser_contacts(&contacts, "ada")
            .iter()
            .all(|contact| contact.id == "named"));
    }
}
