//! One native binding per ADR 0004 R4 screen kind.

use adw::prelude::*;
use localcore_ui::ScreenKind;

/// Proves every screen kind has a binding. A new kind without an arm
/// fails the build (ADR 0004 R7).
pub fn bind_screen(kind: ScreenKind) -> ScreenKind {
    match kind {
        ScreenKind::List
        | ScreenKind::Grid
        | ScreenKind::Detail
        | ScreenKind::Form
        | ScreenKind::Viewer
        | ScreenKind::Settings => kind,
    }
}

pub fn list_page() -> gtk::ScrolledWindow {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("boxed-list");
    gtk::ScrolledWindow::builder().child(&list).build()
}

pub fn grid_page() -> gtk::ScrolledWindow {
    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .build();
    gtk::ScrolledWindow::builder().child(&grid).build()
}

pub fn detail_page() -> gtk::ScrolledWindow {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let clamp = adw::Clamp::builder().child(&column).build();
    gtk::ScrolledWindow::builder().child(&clamp).build()
}

pub fn form_page() -> adw::PreferencesPage {
    adw::PreferencesPage::new()
}

pub fn viewer_page() -> gtk::Picture {
    let picture = gtk::Picture::new();
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture
}

pub fn settings_page() -> adw::PreferencesPage {
    adw::PreferencesPage::new()
}

pub fn screen_widget(kind: ScreenKind) -> gtk::Widget {
    match kind {
        ScreenKind::List => list_page().upcast(),
        ScreenKind::Grid => grid_page().upcast(),
        ScreenKind::Detail => detail_page().upcast(),
        ScreenKind::Form => form_page().upcast(),
        ScreenKind::Viewer => viewer_page().upcast(),
        ScreenKind::Settings => settings_page().upcast(),
    }
}
