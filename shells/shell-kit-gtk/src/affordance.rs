//! One native binding per ADR 0004 R4 affordance.

use adw::prelude::*;
use localcore_ui::Affordance;

use crate::data::ConfirmData;

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

pub fn search_entry() -> gtk::SearchEntry {
    gtk::SearchEntry::new()
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

pub fn primary_action(label: &str) -> gtk::Button {
    let button = gtk::Button::builder().label(label).build();
    button.add_css_class("suggested-action");
    button
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
