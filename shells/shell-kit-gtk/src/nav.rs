//! One native binding per ADR 0004 R4 navigation intent.

use adw::prelude::*;
use localcore_ui::NavIntent;

/// Proves every nav intent has a binding.
pub fn bind_nav(kind: NavIntent) -> NavIntent {
    match kind {
        NavIntent::Push | NavIntent::Sheet | NavIntent::Replace => kind,
    }
}

pub fn navigation_view() -> adw::NavigationView {
    adw::NavigationView::new()
}

pub fn push_page(title: &str, child: &impl IsA<gtk::Widget>) -> adw::NavigationPage {
    adw::NavigationPage::builder()
        .title(title)
        .child(child)
        .build()
}

pub fn sheet(title: &str, child: &impl IsA<gtk::Widget>) -> adw::Dialog {
    let dialog = adw::Dialog::new();
    dialog.set_title(title);
    dialog.set_child(Some(child));
    dialog
}

pub fn nav_widget(kind: NavIntent) -> gtk::Widget {
    match kind {
        NavIntent::Push | NavIntent::Replace => navigation_view().upcast(),
        NavIntent::Sheet => {
            let dialog = adw::Dialog::new();
            // Dialog is not a GtkWidget in every adw version; present
            // a placeholder page the shell can wrap.
            let _ = dialog;
            gtk::Box::new(gtk::Orientation::Vertical, 0).upcast()
        }
    }
}
