//! Photos-tab year rail. Gallery chrome, not a kit binding.
//!
//! Marks come from [`crate::paging::years_from_structure`] on the same
//! `ViewStructure` that replaces `photos_flat`. Folder / memory / person /
//! album grids do not use this overlay.

use gtk::prelude::*;

use crate::paging::YearMark;

/// Overlay a trailing year-button box on the Photos `ScrolledWindow`.
pub fn overlay_on_scroll(scroll: &gtk::ScrolledWindow) -> (gtk::Overlay, gtk::Box) {
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(scroll));
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);
    let rail = gtk::Box::new(gtk::Orientation::Vertical, 0);
    rail.set_halign(gtk::Align::End);
    rail.set_valign(gtk::Align::Center);
    rail.set_margin_end(8);
    rail.set_visible(false);
    overlay.add_overlay(&rail);
    (overlay, rail)
}

/// Rebuild buttons from the structure that just replaced the photos model.
/// Hidden unless more than one year is present.
///
/// `on_year` receives the flat list index of that year's first month so the
/// shell can fill the `ListStore` out to that row before `scroll_to`.
pub fn refill(rail: &gtk::Box, years: &[YearMark], on_year: impl Fn(u32) + Clone + 'static) {
    while let Some(child) = rail.first_child() {
        rail.remove(&child);
    }
    if years.len() <= 1 {
        rail.set_visible(false);
        return;
    }
    for mark in years {
        let label = gtk::Label::new(Some(&mark.year));
        label.add_css_class("dim-label");
        let button = gtk::Button::new();
        button.add_css_class("flat");
        button.set_child(Some(&label));
        let on_year = on_year.clone();
        let index = mark.first_index;
        button.connect_clicked(move |_| on_year(index));
        rail.append(&button);
    }
    rail.set_visible(true);
}
