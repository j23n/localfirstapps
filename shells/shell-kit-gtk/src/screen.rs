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
    list_box_page().0
}

/// L1 content column: clamp at 720, tighten at 480, 12px side margins.
pub fn clamped(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let clamp = adw::Clamp::builder()
        .child(child)
        .maximum_size(720)
        .tightening_threshold(480)
        .build();
    clamp.set_margin_start(12);
    clamp.set_margin_end(12);
    gtk::ScrolledWindow::builder().child(&clamp).build()
}

/// Same as [`list_page`], plus the list itself.
///
/// GTK 4.20+ may wrap the list in a `GtkViewport`, so
/// `ScrolledWindow::child` is not always the `ListBox`. Keep the
/// list handle from construction instead of fishing it out.
pub fn list_box_page() -> (gtk::ScrolledWindow, gtk::ListBox) {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("boxed-list");
    let scroll = clamped(&list);
    (scroll, list)
}

/// Flush media list (HIG list view): large/dynamic tracks, not a boxed list.
pub fn flush_media_list() -> (gtk::ScrolledWindow, gtk::ListBox) {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.set_show_separators(true);
    list.set_hexpand(true);
    let scroll = gtk::ScrolledWindow::builder().child(&list).build();
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);
    (scroll, list)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn find_widget<T: IsA<gtk::Widget>>(root: &gtk::Widget) -> Option<T> {
        if let Ok(hit) = root.clone().downcast::<T>() {
            return Some(hit);
        }
        let mut child = root.first_child();
        while let Some(node) = child {
            if let Some(hit) = find_widget::<T>(&node) {
                return Some(hit);
            }
            child = node.next_sibling();
        }
        None
    }

    #[test]
    fn list_box_page_is_clamped_and_returns_a_live_list() {
        crate::with_adw(|| {
            let (scroll, list) = list_box_page();
            let clamp = find_widget::<adw::Clamp>(&scroll.upcast())
                .expect("list_box_page must wrap the list in AdwClamp");
            assert_eq!(clamp.maximum_size(), 720);
            assert_eq!(clamp.tightening_threshold(), 480);
            list.append(&gtk::Label::new(Some("row")));
            assert!(list.first_child().is_some());
        });
    }
}
