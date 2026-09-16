//! Typed screen builders. The app fills a struct; the kit lays it out.
//!
//! Hand-written, R14-safe. These assemble Phase 2a structure (`clamped`,
//! `sheet(Form)`, boxed lists) and do not invent row kinds.

use adw::prelude::*;

use crate::affordance::{banner, choice_dropdown, primary_action, scope_toggle};
use crate::data::{ActionRowData, EmptyCopy, EmptyKind, Filter, ListScreen};
use crate::item::action_row;
use crate::nav::{sheet, SheetSize};
use crate::screen::{clamped, settings_page};

/// Settings group: caller-owned rows, kit-owned layout.
pub struct SettingsGroup {
    pub id: String,
    pub title: String,
    pub rows: Vec<gtk::Widget>,
}

/// Settings screen input. Groups are laid out in caller order.
pub struct SettingsScreen {
    pub groups: Vec<SettingsGroup>,
}

/// Form dialog input. Builds on [`sheet`] with `SheetSize::Form`.
pub struct FormSheet {
    pub title: String,
    pub cancel: String,
    pub confirm: String,
    pub body: gtk::Widget,
}

/// Control [`list_screen`] built for [`Filter`].
#[derive(Clone)]
pub enum FilterControl {
    Scope(adw::ToggleGroup),
    Choice(gtk::DropDown),
}

/// Widgets [`list_screen`] returns so the app can refill and connect.
#[derive(Clone)]
pub struct ListScreenBuilt {
    pub root: gtk::Box,
    pub search: Option<gtk::SearchEntry>,
    pub filter: Option<FilterControl>,
    pub banner: Option<adw::Banner>,
    pub lists: Vec<gtk::ListBox>,
    pub primary: Option<gtk::Button>,
    pub selection: Option<SelectionBar>,
    pub stack: gtk::Stack,
    pub controls: gtk::Box,
    pub empty_page: adw::StatusPage,
}

impl ListScreenBuilt {
    /// Show the inset section lists.
    pub fn show_lists(&self) {
        self.stack.set_visible_child_name("lists");
    }

    /// Show the empty-state page (hiding the lists).
    pub fn show_empty(&self) {
        self.stack.set_visible_child_name("empty");
    }

    /// Paint a new empty state and reveal it in place of the lists.
    pub fn apply_empty(&self, kind: EmptyKind, copy: &EmptyCopy) -> Option<gtk::Button> {
        let action = paint_empty(&self.empty_page, kind, copy);
        self.show_empty();
        action
    }

    pub fn lists_visible(&self) -> bool {
        self.stack.visible_child_name().as_deref() == Some("lists")
    }
}

/// Preferences page plus the groups the app already filled.
#[derive(Clone)]
pub struct SettingsScreenBuilt {
    pub page: adw::PreferencesPage,
    pub groups: Vec<adw::PreferencesGroup>,
}

/// Dialog plus the header actions the app must hook.
#[derive(Clone)]
pub struct FormSheetBuilt {
    pub dialog: adw::Dialog,
    pub cancel: gtk::Button,
    pub confirm: gtk::Button,
}

/// `AdwStatusPage` plus an optional in-content action.
#[derive(Clone)]
pub struct EmptyStateBuilt {
    pub page: adw::StatusPage,
    pub action: Option<gtk::Button>,
}

/// Selection actions in a revealer, for the bottom of a list page.
#[derive(Clone)]
pub struct SelectionBar {
    pub revealer: gtk::Revealer,
    pub count: gtk::Label,
    pub buttons: Vec<gtk::Button>,
}

/// Assemble an inset list page from [`ListScreen`].
pub fn list_screen(spec: &ListScreen) -> ListScreenBuilt {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.set_hexpand(true);
    root.set_vexpand(true);

    let banner = spec.banner.as_ref().map(|title| {
        let widget = banner(title);
        widget.set_revealed(false);
        root.append(&widget);
        widget
    });

    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    controls.set_margin_top(8);
    controls.set_margin_bottom(8);
    controls.set_margin_start(8);
    controls.set_margin_end(8);

    let search = if spec.search {
        let entry = crate::search_entry();
        entry.set_hexpand(true);
        controls.append(&entry);
        Some(entry)
    } else {
        None
    };

    let filter = spec.filter.as_ref().map(|filter| {
        let control = filter_control(filter);
        match &control {
            FilterControl::Scope(group) => controls.append(group),
            FilterControl::Choice(dropdown) => {
                dropdown.set_hexpand(false);
                dropdown.set_width_request(148);
                dropdown.set_valign(gtk::Align::Center);
                controls.append(dropdown);
            }
        }
        control
    });

    if spec.search || spec.filter.is_some() {
        root.append(&controls);
    }

    let primary = spec.primary.as_deref().map(|label| {
        let button = primary_action(label);
        button.add_css_class("pill");
        button.set_halign(gtk::Align::Center);
        button.set_margin_top(8);
        button.set_margin_bottom(8);
        button.set_margin_start(8);
        button.set_margin_end(8);
        root.append(&button);
        button
    });

    let mut lists = Vec::with_capacity(spec.sections.len());
    let sections_col = gtk::Box::new(gtk::Orientation::Vertical, 18);
    sections_col.set_margin_top(12);
    sections_col.set_margin_bottom(12);
    for section in &spec.sections {
        if let Some(heading) = &section.heading {
            let label = gtk::Label::new(Some(heading));
            label.add_css_class("heading");
            label.set_xalign(0.0);
            label.set_margin_start(12);
            label.set_margin_end(12);
            sections_col.append(&label);
        }
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("boxed-list");
        sections_col.append(&list);
        lists.push(list);
    }
    let lists_scroll = clamped(&sections_col);
    lists_scroll.set_vexpand(true);

    let empty = empty_state(
        spec.empty
            .as_ref()
            .map(|state| state.kind)
            .unwrap_or(EmptyKind::EmptyFolder),
        spec.empty
            .as_ref()
            .map(|state| &state.copy)
            .unwrap_or(&EmptyCopy {
                title: String::new(),
                description: None,
                action: None,
            }),
    );
    empty.page.set_vexpand(true);

    let stack = gtk::Stack::new();
    stack.set_vexpand(true);
    stack.add_named(&lists_scroll, Some("lists"));
    stack.add_named(&empty.page, Some("empty"));
    if spec.empty.is_some() {
        stack.set_visible_child_name("empty");
    } else {
        stack.set_visible_child_name("lists");
    }
    root.append(&stack);

    let selection = spec.selection.as_deref().map(|actions| {
        let bar = selection_bar(actions);
        root.append(&bar.revealer);
        bar
    });

    ListScreenBuilt {
        root,
        search,
        filter,
        banner,
        lists,
        primary,
        selection,
        stack,
        controls,
        empty_page: empty.page,
    }
}

/// Assemble settings groups. Debug builds require Folder first and Info last.
pub fn settings_screen(spec: SettingsScreen) -> SettingsScreenBuilt {
    debug_assert_settings_order(&spec.groups);
    let page = settings_page();
    let mut groups = Vec::with_capacity(spec.groups.len());
    for group in spec.groups {
        let pref = adw::PreferencesGroup::new();
        pref.set_title(&group.title);
        pref.set_widget_name(&group.id);
        for row in group.rows {
            pref.add(&row);
        }
        page.add(&pref);
        groups.push(pref);
    }
    SettingsScreenBuilt { page, groups }
}

/// Form-sized sheet with Cancel at start and a suggested confirm at end.
pub fn form_sheet(spec: FormSheet) -> FormSheetBuilt {
    let dialog = sheet(&spec.title, &spec.body, SheetSize::Form);
    let header = sheet_header(&dialog).expect("sheet() must expose a header bar");
    let cancel = gtk::Button::with_label(&spec.cancel);
    let confirm = gtk::Button::with_label(&spec.confirm);
    confirm.add_css_class("suggested-action");
    header.pack_start(&cancel);
    header.pack_end(&confirm);
    let closer = dialog.clone();
    cancel.connect_clicked(move |_| {
        closer.close();
    });
    FormSheetBuilt {
        dialog,
        cancel,
        confirm,
    }
}

/// ADR 0007 R3 empty state. Loading uses a spinner; others use a named icon.
pub fn empty_state(kind: EmptyKind, copy: &EmptyCopy) -> EmptyStateBuilt {
    let page = adw::StatusPage::new();
    let action = paint_empty(&page, kind, copy);
    EmptyStateBuilt { page, action }
}

/// Action buttons in a [`gtk::Revealer`] for the bottom of a list page.
pub fn selection_bar(actions: &[ActionRowData]) -> SelectionBar {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_margin_top(8);
    row.set_margin_bottom(8);
    row.set_margin_start(8);
    row.set_margin_end(8);
    let count = gtk::Label::new(Some("0 selected"));
    count.set_xalign(0.0);
    count.set_hexpand(true);
    row.append(&count);
    let mut buttons = Vec::with_capacity(actions.len());
    for action in actions {
        let button = action_row(action);
        row.append(&button);
        buttons.push(button);
    }
    let revealer = gtk::Revealer::new();
    revealer.set_child(Some(&row));
    revealer.set_reveal_child(false);
    SelectionBar {
        revealer,
        count,
        buttons,
    }
}

fn filter_control(filter: &Filter) -> FilterControl {
    match filter {
        Filter::Scope(options) => FilterControl::Scope(scope_toggle(options)),
        Filter::Choice(data) => FilterControl::Choice(choice_dropdown(data)),
    }
}

fn paint_empty(page: &adw::StatusPage, kind: EmptyKind, copy: &EmptyCopy) -> Option<gtk::Button> {
    page.set_title(&copy.title);
    page.set_description(copy.description.as_deref());
    match kind {
        EmptyKind::Loading => {
            page.set_icon_name(None::<&str>);
            let spinner = adw::Spinner::new();
            spinner.set_halign(gtk::Align::Center);
            page.set_child(Some(&spinner));
            None
        }
        other => {
            page.set_icon_name(Some(empty_icon(other)));
            if let Some(label) = &copy.action {
                let button = primary_action(label);
                button.add_css_class("pill");
                page.set_child(Some(&button));
                Some(button)
            } else {
                page.set_child(None::<&gtk::Widget>);
                None
            }
        }
    }
}

fn empty_icon(kind: EmptyKind) -> &'static str {
    match kind {
        EmptyKind::NoFolder => "folder-symbolic",
        EmptyKind::EmptyFolder => "folder-open-symbolic",
        EmptyKind::NoMatches => "edit-find-symbolic",
        EmptyKind::Loading => "content-loading-symbolic",
        EmptyKind::Error => "dialog-error-symbolic",
    }
}

fn debug_assert_settings_order(groups: &[SettingsGroup]) {
    if let Some(pos) = groups.iter().position(|group| group.id == "folder") {
        debug_assert_eq!(pos, 0, "folder group must be first when present");
    }
    if let Some(pos) = groups.iter().position(|group| group.id == "info") {
        debug_assert_eq!(
            pos,
            groups.len() - 1,
            "info group must be last when present"
        );
    }
}

fn sheet_header(dialog: &adw::Dialog) -> Option<adw::HeaderBar> {
    dialog.child().and_then(|child| find_widget(&child))
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{ChoiceData, EmptyState, Filter, ListSection};

    fn widget_in_tree(root: &gtk::Widget, needle: &gtk::Widget) -> bool {
        if root == needle {
            return true;
        }
        let mut child = root.first_child();
        while let Some(node) = child {
            if widget_in_tree(&node, needle) {
                return true;
            }
            child = node.next_sibling();
        }
        false
    }

    fn boxed_list_count(lists: &[gtk::ListBox]) -> usize {
        lists
            .iter()
            .filter(|list| list.has_css_class("boxed-list"))
            .count()
    }

    fn section(heading: Option<&str>) -> ListSection {
        ListSection {
            heading: heading.map(str::to_owned),
        }
    }

    fn group(id: &str, title: &str) -> SettingsGroup {
        SettingsGroup {
            id: id.into(),
            title: title.into(),
            rows: Vec::new(),
        }
    }

    #[test]
    fn search_is_present_iff_requested() {
        crate::with_adw(|| {
            let with_search = list_screen(&ListScreen {
                search: true,
                sections: vec![section(None)],
                ..ListScreen::default()
            });
            assert!(with_search.search.is_some());
            assert!(find_widget::<gtk::SearchEntry>(with_search.root.upcast_ref()).is_some());

            let without = list_screen(&ListScreen {
                search: false,
                sections: vec![section(None)],
                ..ListScreen::default()
            });
            assert!(without.search.is_none());
            assert!(find_widget::<gtk::SearchEntry>(without.root.upcast_ref()).is_none());
        });
    }

    #[test]
    fn one_boxed_list_per_section() {
        crate::with_adw(|| {
            let built = list_screen(&ListScreen {
                sections: vec![section(Some("A")), section(Some("B")), section(None)],
                ..ListScreen::default()
            });
            assert_eq!(built.lists.len(), 3);
            assert_eq!(boxed_list_count(&built.lists), 3);
            assert!(built.lists_visible());
            let headings = count_heading_labels(built.root.upcast_ref());
            assert_eq!(headings, 2);
        });
    }

    #[test]
    fn empty_state_replaces_the_list() {
        crate::with_adw(|| {
            let built = list_screen(&ListScreen {
                sections: vec![section(Some("People"))],
                empty: Some(EmptyState {
                    kind: EmptyKind::EmptyFolder,
                    copy: EmptyCopy {
                        title: "No contacts".into(),
                        description: None,
                        action: None,
                    },
                }),
                ..ListScreen::default()
            });
            assert_eq!(built.lists.len(), 1);
            assert!(!built.lists_visible());
            assert_eq!(built.stack.visible_child_name().as_deref(), Some("empty"));
            assert_eq!(built.empty_page.title().as_str(), "No contacts");

            built.show_lists();
            assert!(built.lists_visible());
            built.apply_empty(
                EmptyKind::NoMatches,
                &EmptyCopy {
                    title: "No matches".into(),
                    description: None,
                    action: None,
                },
            );
            assert!(!built.lists_visible());
            assert_eq!(built.empty_page.title().as_str(), "No matches");
        });
    }

    #[test]
    fn settings_debug_assert_folder_first_info_last() {
        crate::with_adw(|| {
            let ok = settings_screen(SettingsScreen {
                groups: vec![
                    group("folder", "Folder"),
                    group("diagnostics", "Diagnostics"),
                    group("info", "About"),
                ],
            });
            assert_eq!(ok.groups.len(), 3);
            assert_eq!(ok.groups[0].title().as_str(), "Folder");
            assert_eq!(ok.groups[2].title().as_str(), "About");

            let folder_not_first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = settings_screen(SettingsScreen {
                    groups: vec![
                        group("tags", "Tags"),
                        group("folder", "Folder"),
                        group("info", "Info"),
                    ],
                });
            }));
            assert!(
                folder_not_first.is_err(),
                "folder present but not first must assert"
            );

            let info_not_last = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = settings_screen(SettingsScreen {
                    groups: vec![
                        group("folder", "Folder"),
                        group("info", "Info"),
                        group("diagnostics", "Diagnostics"),
                    ],
                });
            }));
            assert!(
                info_not_last.is_err(),
                "info present but not last must assert"
            );
        });
    }

    #[test]
    fn form_sheet_puts_cancel_and_suggested_confirm_on_the_header() {
        crate::with_adw(|| {
            let built = form_sheet(FormSheet {
                title: "Contact".into(),
                cancel: "Cancel".into(),
                confirm: "Save".into(),
                body: gtk::Label::new(Some("body")).upcast(),
            });
            assert_eq!(
                built.dialog.content_width(),
                SheetSize::Form.content_width()
            );
            assert!(built.confirm.has_css_class("suggested-action"));
            let child = built.dialog.child().expect("sheet child");
            let header = find_widget::<adw::HeaderBar>(&child).expect("header");
            assert!(widget_in_tree(
                header.upcast_ref(),
                built.cancel.upcast_ref()
            ));
            assert!(widget_in_tree(
                header.upcast_ref(),
                built.confirm.upcast_ref()
            ));
        });
    }

    #[test]
    fn empty_state_uses_r3_icons_and_a_spinner_for_loading() {
        crate::with_adw(|| {
            let none = empty_state(
                EmptyKind::NoFolder,
                &EmptyCopy {
                    title: "Choose a Folder".into(),
                    description: Some("Select a folder to get started.".into()),
                    action: Some("Choose Folder".into()),
                },
            );
            assert_eq!(none.page.icon_name().as_deref(), Some("folder-symbolic"));
            assert!(none.action.is_some());

            let loading = empty_state(
                EmptyKind::Loading,
                &EmptyCopy {
                    title: "Loading".into(),
                    description: None,
                    action: None,
                },
            );
            assert!(loading.page.icon_name().is_none());
            assert!(find_widget::<adw::Spinner>(loading.page.upcast_ref()).is_some());
        });
    }

    #[test]
    fn scope_filter_builds_a_toggle_group() {
        crate::with_adw(|| {
            let built = list_screen(&ListScreen {
                filter: Some(Filter::Scope(vec![
                    "All levels".into(),
                    "Info".into(),
                    "Warning".into(),
                    "Error".into(),
                ])),
                sections: vec![section(None)],
                ..ListScreen::default()
            });
            assert!(matches!(built.filter, Some(FilterControl::Scope(_))));
            assert!(find_widget::<adw::ToggleGroup>(built.root.upcast_ref()).is_some());
            assert!(find_widget::<gtk::DropDown>(built.root.upcast_ref()).is_none());
        });
    }

    #[test]
    fn choice_filter_stays_a_dropdown() {
        crate::with_adw(|| {
            let built = list_screen(&ListScreen {
                filter: Some(Filter::Choice(ChoiceData {
                    labels: vec!["All tags".into(), "Family".into()],
                    selected: 0,
                })),
                sections: vec![section(None)],
                ..ListScreen::default()
            });
            assert!(matches!(built.filter, Some(FilterControl::Choice(_))));
            assert!(find_widget::<gtk::DropDown>(built.root.upcast_ref()).is_some());
            assert!(find_widget::<adw::ToggleGroup>(built.root.upcast_ref()).is_none());
        });
    }

    fn count_heading_labels(root: &gtk::Widget) -> usize {
        let mut count = 0;
        if root.is::<gtk::Label>() && root.has_css_class("heading") {
            count += 1;
        }
        let mut child = root.first_child();
        while let Some(node) = child {
            count += count_heading_labels(&node);
            child = node.next_sibling();
        }
        count
    }
}
