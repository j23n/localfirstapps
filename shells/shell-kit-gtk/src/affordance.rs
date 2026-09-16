//! One native binding per ADR 0004 R4 affordance.

use adw::prelude::*;
use gtk::gio;
use localcore_ui::Affordance;

use crate::data::{ChoiceData, ConfirmData};

/// Proves every affordance has a binding.
pub fn bind_affordance(kind: Affordance) -> Affordance {
    match kind {
        Affordance::Search
        | Affordance::Filter
        | Affordance::Sort
        | Affordance::Selection
        | Affordance::PrimaryAction
        | Affordance::Overflow
        | Affordance::Banner
        | Affordance::Confirm => kind,
    }
}

/// Search field. [`crate::list_screen`] places this; keep the helper
/// as a thin constructor for callers that still assemble by hand.
pub fn search_entry() -> gtk::SearchEntry {
    gtk::SearchEntry::new()
}

/// L6 small fixed set: libadwaita `ToggleGroup` (floor Adw 1.9).
pub fn scope_toggle(options: &[impl AsRef<str>]) -> adw::ToggleGroup {
    let group = adw::ToggleGroup::new();
    for (index, option) in options.iter().enumerate() {
        let label = option.as_ref();
        let toggle = adw::Toggle::builder()
            .name(format!("scope-{index}"))
            .label(label)
            .build();
        group.add(toggle);
    }
    if !options.is_empty() {
        group.set_active(0);
    }
    group
}

pub fn filter_button() -> gtk::MenuButton {
    gtk::MenuButton::builder()
        .icon_name("funnel-symbolic")
        .build()
}

pub fn sort_button() -> gtk::MenuButton {
    gtk::MenuButton::builder()
        .icon_name("view-sort-descending-symbolic")
        .build()
}

/// Native single-choice control used by sort and named-predicate pickers.
pub fn choice_dropdown(data: &ChoiceData) -> gtk::DropDown {
    let refs = data.labels.iter().map(String::as_str).collect::<Vec<_>>();
    let dropdown = gtk::DropDown::from_strings(&refs);
    let selected = if data.labels.is_empty() {
        gtk::INVALID_LIST_POSITION
    } else {
        data.selected
            .min(data.labels.len().saturating_sub(1) as u32)
    };
    dropdown.set_selected(selected);
    dropdown
}

pub fn primary_action(label: &str) -> gtk::Button {
    let button = gtk::Button::builder().label(label).build();
    button.add_css_class("suggested-action");
    button
}

/// Header-bar icon action (page-level create/add).
pub fn header_action(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .build()
}

/// In-content primary (`pill suggested-action`).
pub fn inline_primary(label: &str, icon: &str) -> gtk::Button {
    let button = gtk::Button::builder().label(label).icon_name(icon).build();
    button.add_css_class("pill");
    button.add_css_class("suggested-action");
    button
}

/// Overflow menu built from a `gio::Menu` the app passes.
pub fn overflow(menu: &gio::Menu) -> gtk::MenuButton {
    gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .menu_model(menu)
        .build()
}

pub fn overflow_button() -> gtk::MenuButton {
    gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .build()
}

pub fn banner(title: &str) -> adw::Banner {
    adw::Banner::builder().title(title).revealed(true).build()
}

pub fn confirm_dialog(data: &ConfirmData) -> adw::AlertDialog {
    let dialog = adw::AlertDialog::new(Some(&data.question), None);
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("confirm", &data.destructive_label);
    dialog.set_response_appearance("confirm", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog
}

pub fn affordance_widget(kind: Affordance) -> gtk::Widget {
    match kind {
        Affordance::Search => search_entry().upcast(),
        Affordance::Filter => filter_button().upcast(),
        Affordance::Sort => sort_button().upcast(),
        Affordance::Selection => gtk::ToggleButton::new().upcast(),
        Affordance::PrimaryAction => primary_action("").upcast(),
        Affordance::Overflow => overflow_button().upcast(),
        Affordance::Banner => banner("").upcast(),
        Affordance::Confirm => {
            // AlertDialog is not a Widget; the chrome that offers
            // confirm is a destructive button that presents it.
            primary_action("").upcast()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::prelude::WidgetExt;

    #[test]
    fn scope_toggle_is_a_toggle_group() {
        crate::with_adw(|| {
            let group = scope_toggle(&["All".to_string(), "Info".to_string()]);
            assert_eq!(group.n_toggles(), 2);
            assert_eq!(group.active(), 0);
        });
    }

    #[test]
    fn header_action_and_inline_primary_use_libadwaita_classes() {
        crate::with_adw(|| {
            let header = header_action("list-add-symbolic", "Add");
            assert_eq!(header.icon_name().as_deref(), Some("list-add-symbolic"));
            assert_eq!(header.tooltip_text().as_deref(), Some("Add"));

            let primary = inline_primary("Play All", "media-playback-start-symbolic");
            assert!(primary.has_css_class("pill"));
            assert!(primary.has_css_class("suggested-action"));
        });
    }

    #[test]
    fn overflow_uses_the_passed_menu() {
        crate::with_adw(|| {
            let menu = gio::Menu::new();
            menu.append(Some("Export"), Some("win.export"));
            let button = overflow(&menu);
            assert_eq!(button.icon_name().as_deref(), Some("view-more-symbolic"));
            assert!(button.menu_model().is_some());
        });
    }
}
