//! Laptop chrome or the Comet collapse. Bindings come from `shell-kit-gtk`.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Instant;

use adw::prelude::*;
use gtk::gio;

use contacts_core::{
    assign_tag_logged, bulk_delete_logged, conflict_preview, conflict_rows, delete_logged,
    detail_rows, export_vcard_text, list_rows_filtered, load_edit_draft, new_edit_draft,
    remove_tag_logged, rename_tag_logged, resolve_logged, save_contact_logged, search_hits,
    tag_rows, BirthdayDraft, ConfinedVfs, ContactEditDraft, LabeledAddressDraft, LabeledValueDraft,
    MergeKind, SaveContactCommand, Store, Vfs, TEMP_PREFIX,
};
use shell_kit_gtk::{
    about_dialog, action_row, apply_progress_row, chip_bar, chrome_progress, confirm_dialog,
    empty_state, field_row, field_row_widget, form_sheet, found_detail, header_action,
    highlight_markup, init_style, list_screen, nav_row, overflow, preferences_dialog,
    primary_action, primary_menu, progress_row, push_settings_subpage, search_hit_row,
    settings_screen, sheet, split_list_detail, status_row, text_row, ActionRole, ActionRowData,
    Chip, ChipMode, ChoiceData, ChromeProgress, ConfirmData, ContactsScreen, EmptyCopy, EmptyKind,
    FieldRowData, Filter, FilterControl, FormSheet, Leading, ListScreen, ListScreenBuilt,
    ListSection, MenuCommand, NavRowData, PrimaryMenu, ProgressDisplay, ProgressRowData,
    SettingsGroup, SettingsScreen, SheetSize, SplitListDetail, StatusRowData, StatusSeverity,
    TextRowData, WorkProgress,
};

use crate::routing::route_id;
use crate::{hostname, LogLevel, LogStore, Paths, APP_TITLE};

const TOKEN_CSS: &str = include_str!("../../../design/tokens/generated/contacts.css");
const DIAGNOSTIC_CAPACITY: usize = 5_000;

#[derive(Clone)]
pub struct Window {
    inner: Rc<Inner>,
}

struct Inner {
    window: adw::ApplicationWindow,
    split: SplitListDetail,
    #[allow(dead_code)]
    menu: PrimaryMenu,
    root_stack: gtk::Stack,
    list: gtk::ListBox,
    list_ui: ListScreenBuilt,
    banner: adw::Banner,
    add: gtk::Button,
    tag_filter: gtk::DropDown,
    tag_names: RefCell<Vec<Option<String>>>,
    selected_tag: RefCell<Option<String>>,
    selection_toggle: gtk::ToggleButton,
    selection_revealer: gtk::Revealer,
    selection_count: gtk::Label,
    assign_tag: gtk::Button,
    bulk_delete: gtk::Button,
    selected_ids: RefCell<BTreeSet<String>>,
    settings_dialog: RefCell<Option<adw::PreferencesDialog>>,
    toast: adw::ToastOverlay,
    vfs: RefCell<Option<ConfinedVfs>>,
    device: String,
    paths: Paths,
    store: RefCell<Option<Store>>,
    folder: RefCell<Option<String>>,
    query: RefCell<String>,
    diagnostics: RefCell<LogStore>,
    chrome_progress: ChromeProgress,
    work: Arc<Mutex<Option<WorkProgress>>>,
    work_rx: RefCell<Option<mpsc::Receiver<ContactsWork>>>,
    work_busy: Cell<bool>,
    work_cancel: RefCell<Option<Arc<AtomicBool>>>,
    work_tick: Cell<bool>,
    scan_progress_row: RefCell<Option<adw::ActionRow>>,
}

enum ContactsWork {
    Ready { folder: String, store: Store },
    Cancelled,
    Failed(String),
}

impl Window {
    pub fn present(app: &adw::Application, launch: &crate::LaunchArgs) {
        let this = Self::build(app, launch.comet);
        init_style(TOKEN_CSS);
        if let Some((width, height)) = launch.size {
            this.inner.window.set_default_size(width, height);
        }
        this.inner.window.present();
        this.apply_launch(launch);
        #[cfg(debug_assertions)]
        if let Some(path) = &launch.snapshot {
            shell_kit_gtk::snapshot::capture_after_first_frame(&this.inner.window, path, app);
        }
        #[cfg(not(debug_assertions))]
        if launch.snapshot.is_some() {
            eprintln!("--snapshot is omitted from release builds");
        }
    }

    fn apply_launch(&self, launch: &crate::LaunchArgs) {
        let route = launch.route.as_deref();
        let skip_folder = route == Some(route_id(ContactsScreen::FolderPicker));
        let wait = launch.snapshot.is_some() || launch.route.is_some();
        if let Some(folder) = launch.folder.as_ref().filter(|_| !skip_folder) {
            match folder.to_str() {
                Some(path) => self.open_folder(path, wait),
                None => self.toast("Folder path is not UTF-8"),
            }
        } else if !skip_folder {
            self.load_persisted(wait);
        }
        if let Some(route) = route {
            self.apply_route(route);
        }
    }

    fn apply_route(&self, route: &str) {
        match route {
            "folder-picker" => {
                self.inner
                    .root_stack
                    .set_visible_child_name(route_id(ContactsScreen::FolderPicker));
            }
            "contact-list" => {
                self.show_list();
            }
            "contact-detail" => {
                self.show_list();
                if let Some(id) = self.first_contact_id() {
                    self.show_detail(&id);
                }
            }
            "contact-edit" => {
                self.show_list();
                self.present_edit(self.first_contact_id());
            }
            "settings" => self.present_settings(),
            "tag-management" => {
                self.present_settings();
                self.present_tag_management();
            }
            "logs" => {
                self.present_settings();
                self.present_logs();
            }
            "sync-conflict-group" => {
                self.show_list();
                self.present_conflicts();
            }
            _ => {}
        }
    }

    fn show_list(&self) {
        if self.inner.store.borrow().is_some() {
            self.inner
                .root_stack
                .set_visible_child_name(route_id(ContactsScreen::ContactList));
        }
    }

    fn first_contact_id(&self) -> Option<String> {
        let store = self.inner.store.borrow();
        list_rows_filtered(store.as_ref()?, "", None)
            .into_iter()
            .next()
            .map(|row| row.id)
    }

    fn build(app: &adw::Application, comet: bool) -> Self {
        let window = adw::ApplicationWindow::new(app);
        window.set_title(Some(APP_TITLE));
        window.set_width_request(360);
        window.set_height_request(294);
        if comet {
            window.set_default_size(540, 620);
        } else {
            window.set_default_size(1200, 800);
        }

        let picker = gtk::Box::new(gtk::Orientation::Vertical, 12);
        picker.set_margin_top(24);
        picker.set_margin_bottom(24);
        picker.set_margin_start(18);
        picker.set_margin_end(18);
        picker.set_widget_name(route_id(ContactsScreen::FolderPicker));
        picker.append(&status_row(&StatusRowData {
            message: "Welcome to LocalContacts".into(),
            severity: StatusSeverity::Info,
        }));
        let hint = gtk::Label::new(Some(
            "Select a folder containing your .vcf contact files, or an empty folder to start fresh.",
        ));
        hint.set_wrap(true);
        hint.set_xalign(0.0);
        hint.add_css_class("dim-label");
        picker.append(&hint);
        let choose = primary_action("Choose Folder");
        picker.append(&choose);

        let list_ui = list_screen(&ListScreen {
            search: true,
            filter: Some(Filter::Choice(ChoiceData {
                labels: vec!["All tags".into()],
                selected: 0,
            })),
            sections: vec![ListSection { heading: None }],
            primary: None,
            selection: Some(vec![
                ActionRowData {
                    label: "Assign Tag".into(),
                    role: ActionRole::Normal,
                    enabled: false,
                },
                ActionRowData {
                    label: "Delete".into(),
                    role: ActionRole::Destructive,
                    enabled: false,
                },
            ]),
            banner: Some(String::new()),
            empty: None,
        });
        list_ui
            .root
            .set_widget_name(route_id(ContactsScreen::ContactList));
        let conflict_banner = list_ui.banner.clone().expect("contact-list banner");
        conflict_banner.set_button_label(Some("Review"));
        let search = list_ui.search.clone().expect("contact-list search");
        search.set_placeholder_text(Some("Name, phone, email, address…"));
        let tag_filter = match list_ui.filter.clone().expect("contact-list filter") {
            FilterControl::Choice(dropdown) => dropdown,
            FilterControl::Scope(_) => {
                panic!("contact-list filter is an open-ended Choice, not Scope")
            }
        };
        tag_filter.set_tooltip_text(Some("Filter by tag"));
        let selection_toggle = gtk::ToggleButton::with_label("Select");
        selection_toggle.set_hexpand(true);
        let selection = list_ui.selection.clone().expect("contact-list selection");
        let selection_count = selection.count.clone();
        let assign_tag = selection
            .buttons
            .first()
            .cloned()
            .expect("assign-tag action");
        let bulk_delete = selection
            .buttons
            .get(1)
            .cloned()
            .expect("bulk-delete action");
        let list = list_ui
            .lists
            .first()
            .cloned()
            .expect("contact-list section");

        let empty = empty_state(
            EmptyKind::EmptyFolder,
            &EmptyCopy {
                title: "Select a Contact".into(),
                description: Some("Choose someone from the list.".into()),
                action: None,
            },
        );
        let split = split_list_detail("Contacts", "Select a Contact", &list_ui.root, &empty.page);
        let add = header_action("list-add-symbolic", "Add Contact");
        add.set_sensitive(false);
        split.sidebar_start.append(&add);
        let chrome_progress = chrome_progress();
        split.sidebar_start.append(&chrome_progress.root);
        let menu = primary_menu(
            APP_TITLE,
            &[
                MenuCommand {
                    id: "reload",
                    label: "Reload".into(),
                },
                MenuCommand {
                    id: "choose-folder",
                    label: "Choose Folder…".into(),
                },
            ],
        );
        split.sidebar_end.append(&menu.button);
        if let Some(selection) = &list_ui.selection {
            list_ui.root.remove(&selection.revealer);
            let bottom = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let select_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            select_row.set_margin_start(8);
            select_row.set_margin_end(8);
            select_row.set_margin_bottom(8);
            select_row.append(&selection_toggle);
            bottom.append(&select_row);
            bottom.append(&selection.revealer);
            list_ui.root.append(&bottom);
        }

        let root_stack = gtk::Stack::new();
        root_stack.add_named(&picker, Some(route_id(ContactsScreen::FolderPicker)));
        root_stack.add_named(&split.split, Some(route_id(ContactsScreen::ContactList)));
        root_stack.set_visible_child_name(route_id(ContactsScreen::FolderPicker));

        let toast = adw::ToastOverlay::new();
        toast.set_child(Some(&root_stack));
        window.set_content(Some(&toast));
        split.install(&window);
        let search_action = gio::SimpleAction::new("search", None);
        let search_focus = search.clone();
        search_action.connect_activate(move |_, _| {
            search_focus.grab_focus();
        });
        window.add_action(&search_action);
        if let Some(app) = window.application() {
            app.set_accels_for_action("win.search", &["<Control>f"]);
        }

        let paths = Paths::from_env();
        let device = paths.load_or_create_device_id(&hostname());

        let inner = Rc::new(Inner {
            window: window.clone(),
            split: split.clone(),
            menu: menu.clone(),
            root_stack: root_stack.clone(),
            list: list.clone(),
            list_ui: list_ui.clone(),
            banner: conflict_banner.clone(),
            add: add.clone(),
            tag_filter: tag_filter.clone(),
            tag_names: RefCell::new(vec![None]),
            selected_tag: RefCell::new(None),
            selection_toggle: selection_toggle.clone(),
            selection_revealer: selection.revealer.clone(),
            selection_count: selection_count.clone(),
            assign_tag: assign_tag.clone(),
            bulk_delete: bulk_delete.clone(),
            selected_ids: RefCell::new(BTreeSet::new()),
            settings_dialog: RefCell::new(None),
            toast,
            vfs: RefCell::new(None),
            device,
            paths,
            store: RefCell::new(None),
            chrome_progress,
            work: Arc::new(Mutex::new(None)),
            work_rx: RefCell::new(None),
            work_busy: Cell::new(false),
            work_cancel: RefCell::new(None),
            work_tick: Cell::new(false),
            scan_progress_row: RefCell::new(None),
            folder: RefCell::new(None),
            query: RefCell::new(String::new()),
            diagnostics: RefCell::new(LogStore::new(DIAGNOSTIC_CAPACITY)),
        });
        let this = Window { inner };

        let picker_open = this.clone();
        choose.connect_clicked(move |_| picker_open.pick_folder());

        let added = this.clone();
        add.connect_clicked(move |_| added.present_edit(None));

        let settings = this.clone();
        menu.settings
            .connect_activate(move |_, _| settings.present_settings());
        let about = this.clone();
        menu.about
            .connect_activate(move |_, _| about.present_about());
        if let Some(reload) = menu.extra("reload") {
            let reloader = this.clone();
            reload.connect_activate(move |_, _| reloader.reload_folder());
        }
        let cancel = this.clone();
        this.inner
            .chrome_progress
            .cancel_button()
            .connect_clicked(move |_| cancel.cancel_folder_work());
        if let Some(choose_folder) = menu.extra("choose-folder") {
            let chooser = this.clone();
            choose_folder.connect_activate(move |_, _| chooser.pick_folder());
        }

        let review = this.clone();
        conflict_banner.connect_button_clicked(move |_| review.present_conflicts());

        let searched = this.clone();
        search.connect_search_changed(move |entry| {
            searched.inner.query.replace(entry.text().to_string());
            searched.refill_list();
        });

        let filtered = this.clone();
        tag_filter.connect_selected_notify(move |dropdown| {
            let tag = filtered
                .inner
                .tag_names
                .borrow()
                .get(dropdown.selected() as usize)
                .cloned()
                .flatten();
            filtered.inner.selected_tag.replace(tag);
            filtered.inner.selected_ids.borrow_mut().clear();
            filtered.update_selection_actions();
            filtered.refill_list();
        });

        let selecting = this.clone();
        selection_toggle.connect_toggled(move |button| {
            let active = button.is_active();
            button.set_label(if active { "Done" } else { "Select" });
            if !active {
                selecting.inner.selected_ids.borrow_mut().clear();
            }
            selecting
                .inner
                .add
                .set_sensitive(!active && selecting.inner.store.borrow().is_some());
            selecting.update_selection_actions();
            selecting.refill_list();
        });

        let assigning = this.clone();
        assign_tag.connect_clicked(move |_| assigning.present_bulk_tag());

        let deleting = this.clone();
        bulk_delete.connect_clicked(move |_| deleting.confirm_bulk_delete());

        let opened = this.clone();
        list.connect_row_activated(move |_, row| {
            let id = row.widget_name();
            if !id.is_empty() {
                if opened.inner.selection_toggle.is_active() {
                    let mut selected = opened.inner.selected_ids.borrow_mut();
                    if !selected.remove(id.as_str()) {
                        selected.insert(id.to_string());
                    }
                    drop(selected);
                    opened.update_selection_actions();
                    opened.refill_list();
                } else {
                    opened.show_detail(&id);
                }
            }
        });

        this.record(LogLevel::Info, "app", "Application started");
        this.refill_settings();
        this
    }

    fn toast(&self, message: &str) {
        self.inner.toast.add_toast(adw::Toast::new(message));
    }

    fn record(&self, level: LogLevel, category: &str, message: impl Into<String>) {
        self.inner
            .diagnostics
            .borrow_mut()
            .record(level, category, message);
    }

    fn load_persisted(&self, wait: bool) {
        let _span = localcore_trace::span_always("contacts", "load_persisted");
        if let Some(folder) = self.inner.paths.load_folder() {
            if std::path::Path::new(&folder).is_dir() {
                self.open_folder(&folder, wait);
            } else {
                self.record(
                    LogLevel::Warning,
                    "folder",
                    "Saved contacts folder is unavailable",
                );
            }
        } else {
            self.record(LogLevel::Info, "folder", "Waiting for a contacts folder");
        }
    }

    fn pick_folder(&self) {
        let dialog = gtk::FileDialog::builder()
            .title("Choose Folder")
            .modal(true)
            .build();
        let window = self.inner.window.clone();
        let this = self.clone();
        dialog.select_folder(
            Some(&window),
            gio::Cancellable::NONE,
            move |result| match result {
                Ok(file) => match file.path().and_then(|p| p.to_str().map(str::to_owned)) {
                    Some(path) => this.open_folder(&path, false),
                    None => {
                        this.record(
                            LogLevel::Warning,
                            "folder",
                            "Selected folder path is not supported",
                        );
                        this.toast("Folder path is not UTF-8");
                    }
                },
                Err(err) => {
                    let msg = err.to_string();
                    if !msg.contains("Dismissed") && !msg.contains("dismissed") {
                        this.record(LogLevel::Error, "folder", "Folder picker failed");
                        this.toast(&msg);
                    }
                }
            },
        );
    }

    fn open_folder(&self, folder: &str, wait: bool) {
        let _span = localcore_trace::span_always("contacts", "open_folder");
        self.start_folder_work(folder.to_string());
        if wait {
            self.drain_folder_work();
        }
    }

    fn folder_vfs(&self) -> Option<ConfinedVfs> {
        self.inner.vfs.borrow().clone()
    }

    fn with_store_mut<T>(&self, f: impl FnOnce(&dyn Vfs, &mut Store) -> T) -> Option<T> {
        let vfs = self.folder_vfs()?;
        let mut slot = self.inner.store.borrow_mut();
        slot.as_mut().map(|store| f(&vfs as &dyn Vfs, store))
    }

    fn refill_list(&self) {
        let _span = localcore_trace::span_always("contacts", "refill_list");
        while let Some(child) = self.inner.list.first_child() {
            self.inner.list.remove(&child);
        }
        let Some(store) = self.inner.store.borrow().clone() else {
            return;
        };
        let query = self.inner.query.borrow().clone();
        let selected_tag = self.inner.selected_tag.borrow().clone();
        if !query.trim().is_empty() {
            let hits = search_hits(&store, &query, selected_tag.as_deref());
            if hits.is_empty() {
                self.inner.list_ui.apply_empty(
                    EmptyKind::NoMatches,
                    &EmptyCopy {
                        title: "No Results".into(),
                        description: None,
                        action: None,
                    },
                );
            } else {
                self.inner.list_ui.show_lists();
                for hit in hits {
                    let subtitle = format!(
                        "{}: {}",
                        gtk::glib::markup_escape_text(&hit.field_label),
                        highlight_markup(&hit.field_value, &query)
                    );
                    let widget = search_hit_row(&hit.title, &subtitle, hit.symbol);
                    widget.set_widget_name(&hit.id);
                    self.inner.list.append(&widget);
                }
            }
        } else {
            let rows = list_rows_filtered(&store, "", selected_tag.as_deref());
            if rows.is_empty() {
                self.inner.list_ui.apply_empty(
                    EmptyKind::EmptyFolder,
                    &EmptyCopy {
                        title: "No contacts".into(),
                        description: None,
                        action: None,
                    },
                );
            } else {
                self.inner.list_ui.show_lists();
                let mut last_key: Option<String> = None;
                for row in rows {
                    let key = section_key(&row.title);
                    if last_key.as_deref() != Some(key.as_str()) {
                        let heading = gtk::Label::new(Some(&key));
                        heading.add_css_class("heading");
                        heading.set_xalign(0.0);
                        heading.set_halign(gtk::Align::Start);
                        heading.set_margin_top(if last_key.is_some() { 8 } else { 0 });
                        let header_row = gtk::ListBoxRow::new();
                        header_row.set_activatable(false);
                        header_row.set_selectable(false);
                        header_row.set_child(Some(&heading));
                        self.inner.list.append(&header_row);
                        last_key = Some(key);
                    }
                    let widget = text_row(&TextRowData {
                        title: row.title.clone(),
                        subtitle: row.subtitle,
                        trailing: None,
                        leading: Some(Leading::Avatar {
                            text: row.title,
                            texture: None,
                        }),
                    });
                    widget.set_activatable(true);
                    widget.set_widget_name(&row.id);
                    if self.inner.selection_toggle.is_active() {
                        let check = gtk::CheckButton::new();
                        check.set_active(self.inner.selected_ids.borrow().contains(&row.id));
                        check.set_sensitive(false);
                        check.set_valign(gtk::Align::Center);
                        widget.add_prefix(&check);
                    }
                    self.inner.list.append(&widget);
                }
            }
        }
        if let Some(vfs) = self.folder_vfs() {
            match conflict_rows(&vfs, &store) {
                Ok(groups) => {
                    if groups.is_empty() {
                        self.inner.banner.set_revealed(false);
                    } else {
                        self.inner
                            .banner
                            .set_title(&format!("{} Sync Conflicts", groups.len()));
                        self.inner.banner.set_revealed(true);
                    }
                }
                Err(err) => {
                    self.record(
                        LogLevel::Error,
                        "conflict",
                        "Could not inspect sync conflicts",
                    );
                    self.toast(&err.to_string());
                }
            }
        }
    }

    fn refill_tag_filter(&self) {
        let rows = self
            .inner
            .store
            .borrow()
            .as_ref()
            .map(tag_rows)
            .unwrap_or_default();
        let current = self.inner.selected_tag.borrow().clone();
        let mut names = vec![None];
        let mut labels = vec!["All tags".to_string()];
        for row in rows {
            labels.push(row.title);
            names.push(Some(row.id));
        }
        let selected = current
            .as_ref()
            .and_then(|tag| {
                names
                    .iter()
                    .position(|candidate| candidate.as_ref() == Some(tag))
            })
            .unwrap_or(0);
        if selected == 0 {
            self.inner.selected_tag.replace(None);
        }
        self.inner.tag_names.replace(names);
        let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        self.inner
            .tag_filter
            .set_model(Some(&gtk::StringList::new(&label_refs)));
        self.inner.tag_filter.set_selected(selected as u32);
    }

    fn update_selection_actions(&self) {
        let count = self.inner.selected_ids.borrow().len();
        self.inner
            .selection_count
            .set_label(&format!("{count} selected"));
        self.inner.assign_tag.set_sensitive(count > 0);
        self.inner.bulk_delete.set_sensitive(count > 0);
        self.inner
            .selection_revealer
            .set_reveal_child(self.inner.selection_toggle.is_active());
    }

    fn show_empty_detail(&self) {
        let empty = empty_state(
            EmptyKind::EmptyFolder,
            &EmptyCopy {
                title: "Select a Contact".into(),
                description: Some("Choose someone from the list.".into()),
                action: None,
            },
        );
        self.clear_content_end();
        self.inner
            .split
            .show_content("Select a Contact", &empty.page);
        self.inner.split.show_sidebar();
    }

    fn show_detail(&self, id: &str) {
        let Some(store) = self.inner.store.borrow().clone() else {
            return;
        };
        let fields = match detail_rows(&store, id) {
            Ok(fields) => fields,
            Err(err) => {
                self.record(LogLevel::Warning, "contact", "Contact was not found");
                self.toast(&err.to_string());
                return;
            }
        };
        let title = fields
            .iter()
            .find(|row| row.id.as_deref() == Some("fn"))
            .map(|row| row.value.clone())
            .unwrap_or_else(|| id.to_string());
        let org = fields
            .iter()
            .find(|row| row.id.as_deref() == Some("org"))
            .map(|row| row.value.clone());
        let scroll = gtk::ScrolledWindow::builder()
            .child(&contact_detail_body(&title, org.as_deref(), &fields))
            .build();
        scroll.set_widget_name(route_id(ContactsScreen::ContactDetail));
        self.inner.split.show_content(&title, &scroll);
        self.fill_detail_actions(id);
    }

    fn fill_detail_actions(&self, id: &str) {
        self.clear_content_end();
        let edit = header_action("document-edit-symbolic", "Edit");
        let editor = self.clone();
        let edit_id = id.to_string();
        edit.connect_clicked(move |_| editor.present_edit(Some(edit_id.clone())));
        self.inner.split.content_end.append(&edit);

        let menu = gio::Menu::new();
        menu.append(Some("Export…"), Some("detail.export"));
        menu.append(Some("Delete Contact…"), Some("detail.delete"));
        let overflow = overflow(&menu);
        overflow.set_tooltip_text(Some("Contact Menu"));
        let group = gio::SimpleActionGroup::new();
        let export = gio::SimpleAction::new("export", None);
        let delete = gio::SimpleAction::new("delete", None);
        let exporter = self.clone();
        let export_id = id.to_string();
        export.connect_activate(move |_, _| exporter.export_contact(&export_id));
        let deleter = self.clone();
        let delete_id = id.to_string();
        delete.connect_activate(move |_, _| deleter.confirm_delete(&delete_id));
        group.add_action(&export);
        group.add_action(&delete);
        overflow.insert_action_group("detail", Some(&group));
        self.inner.split.content_end.append(&overflow);
    }

    fn clear_content_end(&self) {
        while let Some(child) = self.inner.split.content_end.first_child() {
            self.inner.split.content_end.remove(&child);
        }
    }

    fn reload_folder(&self) {
        let Some(folder) = self.inner.folder.borrow().clone() else {
            self.toast("Choose a Folder first");
            return;
        };
        self.open_folder(&folder, false);
    }

    fn start_folder_work(&self, folder: String) {
        if self.inner.work_busy.get() {
            self.cancel_folder_work();
        }
        self.inner.work_busy.set(true);
        if let Ok(mut work) = self.inner.work.lock() {
            *work = Some(WorkProgress::new("Reloading"));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.inner.work_cancel.replace(Some(cancel.clone()));
        let status = self.inner.work.clone();
        let (tx, rx) = mpsc::channel();
        self.inner.work_rx.replace(Some(rx));
        let vfs = match ConfinedVfs::new(TEMP_PREFIX, &folder) {
            Ok(vfs) => vfs,
            Err(error) => {
                let _ = tx.send(ContactsWork::Failed(error.to_string()));
                return;
            }
        };
        thread::spawn(move || {
            let report = |discovered: usize| {
                if let Ok(mut guard) = status.lock() {
                    if let Some(progress) = guard.as_mut() {
                        progress.update("Reloading", Some(found_detail(discovered as u64)), None);
                    }
                }
            };
            let cancelled = || cancel.load(Ordering::Relaxed);
            let outcome =
                match Store::open_with_hooks(&vfs, &folder, Some(&report), Some(&cancelled)) {
                    Ok(Some(store)) => ContactsWork::Ready { folder, store },
                    Ok(None) => ContactsWork::Cancelled,
                    Err(error) => ContactsWork::Failed(error.to_string()),
                };
            let _ = tx.send(outcome);
        });
        self.ensure_work_tick();
        self.sync_chrome_progress();
    }

    fn cancel_folder_work(&self) {
        if let Some(flag) = self.inner.work_cancel.borrow().as_ref() {
            flag.store(true, Ordering::Relaxed);
        }
    }

    fn drain_folder_work(&self) {
        let ctx = gtk::glib::MainContext::default();
        while self.inner.work_busy.get() {
            self.poll_folder_work();
            if self.inner.work_busy.get() {
                ctx.iteration(true);
            }
        }
    }

    fn ensure_work_tick(&self) {
        if self.inner.work_tick.get() {
            return;
        }
        self.inner.work_tick.set(true);
        let this = self.clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            this.poll_folder_work();
            this.sync_chrome_progress();
            if this.inner.work_busy.get() {
                gtk::glib::ControlFlow::Continue
            } else {
                this.inner.work_tick.set(false);
                gtk::glib::ControlFlow::Break
            }
        });
    }

    fn poll_folder_work(&self) {
        let Some(rx) = self.inner.work_rx.borrow_mut().take() else {
            return;
        };
        match rx.try_recv() {
            Ok(ContactsWork::Ready { folder, store }) => {
                self.finish_folder_open(folder, store);
            }
            Ok(ContactsWork::Cancelled) => {
                self.inner.work_busy.set(false);
                if let Ok(mut work) = self.inner.work.lock() {
                    *work = None;
                }
                self.inner.work_cancel.replace(None);
                self.sync_chrome_progress();
                self.toast("Reload cancelled");
            }
            Ok(ContactsWork::Failed(error)) => {
                self.inner.work_busy.set(false);
                if let Ok(mut work) = self.inner.work.lock() {
                    *work = None;
                }
                self.inner.work_cancel.replace(None);
                self.sync_chrome_progress();
                self.record(LogLevel::Error, "folder", "Could not open contacts folder");
                self.toast(&error);
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.inner.work_rx.replace(Some(rx));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.inner.work_busy.set(false);
            }
        }
    }

    fn finish_folder_open(&self, folder: String, store: Store) {
        let vfs = match ConfinedVfs::new(TEMP_PREFIX, &folder) {
            Ok(vfs) => vfs,
            Err(error) => {
                self.inner.work_busy.set(false);
                if let Ok(mut work) = self.inner.work.lock() {
                    *work = None;
                }
                self.inner.work_cancel.replace(None);
                self.sync_chrome_progress();
                self.record(
                    LogLevel::Error,
                    "folder",
                    "Could not confine contacts folder",
                );
                self.toast(&error.to_string());
                return;
            }
        };
        let count = list_rows_filtered(&store, "", None).len();
        self.inner.vfs.replace(Some(vfs));
        self.inner.store.replace(Some(store));
        self.inner.folder.replace(Some(folder.clone()));
        self.inner.paths.save_folder(&folder);
        self.inner.selection_toggle.set_active(false);
        self.inner.selected_ids.borrow_mut().clear();
        self.inner.selected_tag.replace(None);
        self.inner
            .root_stack
            .set_visible_child_name(route_id(ContactsScreen::ContactList));
        self.inner.add.set_sensitive(true);
        self.show_empty_detail();
        self.record(
            LogLevel::Info,
            "folder",
            format!("Opened contacts folder with {count} contacts"),
        );
        self.inner.work_busy.set(false);
        if let Ok(mut work) = self.inner.work.lock() {
            *work = None;
        }
        self.inner.work_cancel.replace(None);
        self.sync_chrome_progress();
        self.refill_tag_filter();
        self.refill_list();
        self.refill_settings();
    }

    fn sync_chrome_progress(&self) {
        let progress = self
            .inner
            .work
            .lock()
            .ok()
            .and_then(|guard| {
                guard
                    .as_ref()
                    .and_then(|work| work.chrome(Instant::now()).cloned())
            })
            .map(|display| ProgressRowData::from(&display));
        self.inner.chrome_progress.apply(progress.as_ref());
        if let Some(row) = self.inner.scan_progress_row.borrow().clone() {
            let display = self
                .inner
                .work
                .lock()
                .ok()
                .and_then(|guard| guard.as_ref().map(|work| work.display().clone()))
                .unwrap_or_else(idle_contacts_progress);
            apply_progress_row(&row, &ProgressRowData::from(&display));
        }
    }

    fn present_about(&self) {
        about_dialog(crate::APP_ID, APP_TITLE, env!("CARGO_PKG_VERSION"))
            .present(Some(&self.inner.window));
    }

    fn present_settings(&self) {
        let dialog = preferences_dialog("Settings", settings_screen(self.settings_spec()));
        dialog.set_widget_name(route_id(ContactsScreen::Settings));
        self.inner.settings_dialog.replace(Some(dialog.clone()));
        dialog.present(Some(&self.inner.window));
    }

    fn confirm_delete(&self, id: &str) {
        let dialog = confirm_dialog(&ConfirmData {
            question: "Delete Contact".into(),
            destructive_label: "Delete".into(),
        });
        let this = self.clone();
        let id = id.to_string();
        dialog.connect_response(None, move |_, response| {
            if response != "confirm" {
                return;
            }
            let result = this
                .with_store_mut(|vfs, store| delete_logged(vfs, store, &this.inner.device, &id));
            match result {
                Some(Ok(())) => {
                    this.record(LogLevel::Info, "contact", "Deleted contact");
                    this.show_empty_detail();
                    this.refill_tag_filter();
                    this.refill_list();
                }
                Some(Err(err)) => {
                    this.record(LogLevel::Error, "contact", "Could not delete contact");
                    this.toast(&err.to_string());
                }
                None => {
                    this.record(
                        LogLevel::Warning,
                        "contact",
                        "Delete requested without a contacts folder",
                    );
                    this.toast("No folder selected. Please select a contacts folder first.");
                }
            }
        });
        dialog.present(Some(&self.inner.window));
    }

    fn present_bulk_tag(&self) {
        let ids: Vec<String> = self.inner.selected_ids.borrow().iter().cloned().collect();
        if ids.is_empty() {
            return;
        }
        let tag = entry_field("Tag", "");
        let apply = primary_action("Assign");
        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::new();
        group.add(&tag);
        page.add(&group);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
        column.append(&page);
        column.append(&apply);
        let dialog = sheet("Assign Tag", &column, SheetSize::Form);
        dialog.present(Some(&self.inner.window));

        let this = self.clone();
        apply.connect_clicked(move |_| {
            let result = this.with_store_mut(|vfs, store| {
                assign_tag_logged(vfs, store, &this.inner.device, tag.text().as_str(), &ids)
            });
            match result {
                Some(Ok(changed)) => {
                    this.record(
                        LogLevel::Info,
                        "tag",
                        format!("Assigned tag to {changed} contacts"),
                    );
                    dialog.close();
                    this.inner.selection_toggle.set_active(false);
                    this.refill_tag_filter();
                    this.refill_list();
                }
                Some(Err(err)) => {
                    this.record(LogLevel::Error, "tag", "Could not assign tag");
                    this.toast(&err.to_string());
                }
                None => this.toast("No folder selected. Please select a contacts folder first."),
            }
        });
    }

    fn confirm_bulk_delete(&self) {
        let ids: Vec<String> = self.inner.selected_ids.borrow().iter().cloned().collect();
        if ids.is_empty() {
            return;
        }
        let dialog = confirm_dialog(&ConfirmData {
            question: format!("Delete {} contacts?", ids.len()),
            destructive_label: "Delete".into(),
        });
        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response != "confirm" {
                return;
            }
            let result = this.with_store_mut(|vfs, store| {
                bulk_delete_logged(vfs, store, &this.inner.device, &ids)
            });
            match result {
                Some(Ok(deleted)) => {
                    this.record(
                        LogLevel::Info,
                        "contact",
                        format!("Deleted {deleted} contacts"),
                    );
                    this.inner.selection_toggle.set_active(false);
                    this.refill_tag_filter();
                    this.refill_list();
                }
                Some(Err(err)) => {
                    this.record(LogLevel::Error, "contact", "Could not delete contacts");
                    this.toast(&err.to_string());
                }
                None => this.toast("No folder selected. Please select a contacts folder first."),
            }
        });
        dialog.present(Some(&self.inner.window));
    }

    fn present_edit(&self, id: Option<String>) {
        let draft = if let Some(id) = id.as_ref() {
            match self.with_store_mut(|vfs, store| load_edit_draft(vfs, store, id)) {
                Some(Ok(draft)) => draft,
                Some(Err(err)) => {
                    self.record(LogLevel::Warning, "contact", "Contact was not found");
                    self.toast(&err.to_string());
                    return;
                }
                None => {
                    self.toast("No folder selected. Please select a contacts folder first.");
                    return;
                }
            }
        } else {
            new_edit_draft()
        };

        let form = ContactEditForm::new(&draft, self);
        let edit = form_sheet(FormSheet {
            title: "Contact".into(),
            cancel: "Cancel".into(),
            confirm: "Save".into(),
            body: form.scroller.clone().upcast(),
        });
        let dialog = edit.dialog.clone();
        dialog.set_widget_name(route_id(ContactsScreen::ContactEdit));
        dialog.present(Some(&self.inner.window));

        let this = self.clone();
        edit.confirm.connect_clicked(move |_| {
            let filled = match form.fill_draft(draft.clone()) {
                Ok(filled) => filled,
                Err(message) => {
                    this.record(LogLevel::Warning, "contact", "Contact form is invalid");
                    this.toast(&message);
                    return;
                }
            };
            let result = this.with_store_mut(|vfs, store| {
                save_contact_logged(
                    vfs,
                    store,
                    &this.inner.device,
                    SaveContactCommand { draft: filled },
                )
            });
            match result {
                Some(Ok(saved)) => {
                    this.record(LogLevel::Info, "contact", "Saved contact");
                    dialog.close();
                    this.refill_tag_filter();
                    this.refill_list();
                    if let Some(id) = saved.id {
                        this.show_detail(&id);
                    }
                }
                Some(Err(err)) => {
                    this.record(LogLevel::Error, "contact", "Could not save contact");
                    this.toast(&err.to_string());
                }
                None => {
                    this.record(
                        LogLevel::Warning,
                        "contact",
                        "Save requested without a contacts folder",
                    );
                    this.toast("No folder selected. Please select a contacts folder first.");
                }
            }
        });
    }

    fn export_contact(&self, id: &str) {
        let text = match self
            .inner
            .store
            .borrow()
            .as_ref()
            .map(|store| export_vcard_text(store, id))
        {
            Some(Ok(text)) => text,
            Some(Err(err)) => {
                self.record(LogLevel::Error, "export", "Could not export contact");
                self.toast(&err.to_string());
                return;
            }
            None => {
                self.toast("No folder selected. Please select a contacts folder first.");
                return;
            }
        };
        let dialog = gtk::FileDialog::builder()
            .title("Export contact")
            .initial_name("contact.vcf")
            .modal(true)
            .build();
        let window = self.inner.window.clone();
        let this = self.clone();
        dialog.save(
            Some(&window),
            gio::Cancellable::NONE,
            move |result| match result {
                Ok(file) => {
                    if let Some(path) = file.path() {
                        if let Err(err) = std::fs::write(path, text.as_bytes()) {
                            this.record(LogLevel::Error, "export", "Could not export contact");
                            this.toast(&err.to_string());
                        } else {
                            this.record(LogLevel::Info, "export", "Exported contact");
                        }
                    } else {
                        this.record(
                            LogLevel::Warning,
                            "export",
                            "Selected export destination is not supported",
                        );
                    }
                }
                Err(err) => {
                    let msg = err.to_string();
                    if !msg.contains("Dismissed") && !msg.contains("dismissed") {
                        this.record(LogLevel::Error, "export", "Contact export failed");
                        this.toast(&msg);
                    }
                }
            },
        );
    }

    fn present_conflicts(&self) {
        let Some((store, vfs)) = self.inner.store.borrow().clone().zip(self.folder_vfs()) else {
            self.record(
                LogLevel::Warning,
                "conflict",
                "Conflict review requested without a contacts folder",
            );
            return;
        };
        let groups = match conflict_rows(&vfs, &store) {
            Ok(groups) => groups,
            Err(err) => {
                self.record(LogLevel::Error, "conflict", "Could not load sync conflicts");
                self.toast(&err.to_string());
                return;
            }
        };
        self.record(
            LogLevel::Info,
            "conflict",
            format!("Reviewing {} sync conflict groups", groups.len()),
        );
        let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
        column.set_margin_top(12);
        column.set_margin_bottom(12);
        column.set_margin_start(12);
        column.set_margin_end(12);
        let intro = gtk::Label::new(Some(
            "Two devices edited the same file. Disjoint fields merge automatically. The same field is a choice. Losing copies are deleted only after you confirm.",
        ));
        intro.set_wrap(true);
        intro.set_xalign(0.0);
        intro.add_css_class("dim-label");
        column.append(&intro);

        let dialog = sheet(
            "Sync Conflicts",
            &gtk::ScrolledWindow::builder().child(&column).build(),
            SheetSize::Picker,
        );
        dialog.set_widget_name(route_id(ContactsScreen::SyncConflictGroup));

        for group in groups {
            let row = text_row(&TextRowData {
                title: group.title.clone(),
                subtitle: Some(group.subtitle.clone()),
                trailing: Some(group.trailing.clone()),
                leading: None,
            });
            column.append(&row);
            let name = group.id.clone();
            let review = action_row(&ActionRowData {
                label: "Review Diff…".into(),
                role: ActionRole::Normal,
                enabled: true,
            });
            let this = self.clone();
            let host = dialog.clone();
            review.connect_clicked(move |_| this.present_conflict_diff(&name, Some(&host)));
            column.append(&review);
        }

        dialog.present(Some(&self.inner.window));
    }

    fn present_conflict_diff(&self, canonical: &str, parent: Option<&adw::Dialog>) {
        let Some((store, vfs)) = self.inner.store.borrow().clone().zip(self.folder_vfs()) else {
            return;
        };
        let preview = match conflict_preview(&vfs, &store, canonical) {
            Ok(preview) => preview,
            Err(err) => {
                self.record(LogLevel::Error, "conflict", "Could not load conflict diff");
                self.toast(&err.to_string());
                return;
            }
        };
        let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
        column.set_margin_top(12);
        column.set_margin_bottom(12);
        column.set_margin_start(12);
        column.set_margin_end(12);
        let discarded = if preview.discarded.is_empty() {
            "No copies will be deleted.".into()
        } else {
            format!("Confirming deletes: {}.", preview.discarded.join(", "))
        };
        let intro = gtk::Label::new(Some(&discarded));
        intro.set_wrap(true);
        intro.set_xalign(0.0);
        intro.add_css_class("dim-label");
        column.append(&intro);

        let page = adw::PreferencesPage::new();
        let picks: Rc<RefCell<HashMap<String, String>>> = Rc::new(RefCell::new(HashMap::new()));
        let checks: Rc<RefCell<Vec<(String, String, gtk::Image)>>> =
            Rc::new(RefCell::new(Vec::new()));
        let edits: Rc<RefCell<HashMap<String, adw::EntryRow>>> =
            Rc::new(RefCell::new(HashMap::new()));

        if preview.fields.is_empty() {
            let group = adw::PreferencesGroup::new();
            group.set_title("Merged Contact");
            for field in &preview.merged_fields {
                group.add(&field_row(&FieldRowData {
                    label: field.label.clone(),
                    value: field.value.clone(),
                    editable: false,
                }));
            }
            page.add(&group);
        } else {
            for field in &preview.fields {
                let group = adw::PreferencesGroup::new();
                group.set_title(&field.field);
                for (index, (source, value)) in field.sides.iter().enumerate() {
                    let choice_id = format!("{}|{source}", field.field);
                    let row = adw::ActionRow::builder()
                        .title(source)
                        .subtitle(value)
                        .activatable(true)
                        .build();
                    let check = gtk::Image::from_icon_name("object-select-symbolic");
                    check.set_visible(index == 0);
                    row.add_suffix(&check);
                    if index == 0 {
                        picks
                            .borrow_mut()
                            .insert(field.field.clone(), choice_id.clone());
                    }
                    checks.borrow_mut().push((
                        field.field.clone(),
                        choice_id.clone(),
                        check.clone(),
                    ));
                    let picks = picks.clone();
                    let checks = checks.clone();
                    let key = field.field.clone();
                    let id = choice_id;
                    row.connect_activated(move |_| {
                        picks.borrow_mut().insert(key.clone(), id.clone());
                        for (f, choice_id, image) in checks.borrow().iter() {
                            image.set_visible(f == &key && choice_id == &id);
                        }
                    });
                    group.add(&row);
                }
                let keep = field
                    .sides
                    .first()
                    .map(|(_, value)| value.clone())
                    .unwrap_or_default();
                let edit = adw::EntryRow::builder().title("Keep").text(&keep).build();
                edits.borrow_mut().insert(field.field.clone(), edit.clone());
                group.add(&edit);
                page.add(&group);
            }
        }
        column.append(&page);
        let confirm = primary_action("Keep This Version");
        column.append(&confirm);
        let dialog = sheet("Sync Conflict", &column, SheetSize::Picker);
        dialog.set_widget_name(route_id(ContactsScreen::SyncConflictGroup));
        dialog.present(Some(&self.inner.window));

        let this = self.clone();
        let canonical = canonical.to_string();
        let needed = preview.fields.len();
        let kind = preview.kind;
        let parent = parent.cloned();
        confirm.connect_clicked(move |_| {
            let ids: Vec<String> = if kind == MergeKind::Choice {
                let map = picks.borrow().clone();
                if map.len() != needed {
                    this.toast("Choose a value for every field");
                    return;
                }
                map.into_values().collect()
            } else {
                Vec::new()
            };
            this.resolve_group(&canonical, &ids, parent.as_ref());
            dialog.close();
        });
        let _ = edits;
    }

    fn resolve_group(&self, canonical: &str, choice_ids: &[String], host: Option<&adw::Dialog>) {
        let result = self.with_store_mut(|vfs, store| {
            resolve_logged(vfs, store, &self.inner.device, canonical, choice_ids)
        });
        match result {
            Some(Ok(())) => {
                self.record(LogLevel::Info, "conflict", "Resolved sync conflict group");
                self.refill_list();
                let empty = self
                    .inner
                    .store
                    .borrow()
                    .as_ref()
                    .is_some_and(|store| store.conflict_groups().is_empty());
                if empty {
                    if let Some(host) = host {
                        host.close();
                    }
                }
            }
            Some(Err(err)) => {
                self.record(
                    LogLevel::Error,
                    "conflict",
                    "Could not resolve sync conflict group",
                );
                self.toast(&err.to_string());
            }
            None => {
                self.record(
                    LogLevel::Warning,
                    "conflict",
                    "Resolve requested without a contacts folder",
                );
                self.toast("No folder selected. Please select a contacts folder first.");
            }
        }
    }

    fn present_tag_management(&self) {
        let ui = list_screen(&ListScreen {
            sections: vec![ListSection { heading: None }],
            ..ListScreen::default()
        });
        self.refill_tag_management(&ui);
        self.push_settings_page("Tags", route_id(ContactsScreen::TagManagement), &ui.root);
    }

    fn refill_tag_management(&self, ui: &ListScreenBuilt) {
        let list = ui.lists.first().expect("tag-management section");
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }
        let rows = self
            .inner
            .store
            .borrow()
            .as_ref()
            .map(tag_rows)
            .unwrap_or_default();
        if rows.is_empty() {
            ui.apply_empty(
                EmptyKind::EmptyFolder,
                &EmptyCopy {
                    title: "No tags".into(),
                    description: None,
                    action: None,
                },
            );
            return;
        }
        ui.show_lists();
        for tag in rows {
            let row = text_row(&TextRowData {
                title: tag.title,
                subtitle: tag.subtitle,
                trailing: tag.trailing,
                leading: None,
            });
            let rename = gtk::Button::from_icon_name("document-edit-symbolic");
            rename.set_tooltip_text(Some("Rename tag"));
            rename.set_valign(gtk::Align::Center);
            let remove = gtk::Button::from_icon_name("user-trash-symbolic");
            remove.set_tooltip_text(Some("Remove tag"));
            remove.add_css_class("destructive-action");
            remove.set_valign(gtk::Align::Center);
            row.add_suffix(&rename);
            row.add_suffix(&remove);
            list.append(&row);

            let this = self.clone();
            let name = tag.id.clone();
            let refreshed = ui.clone();
            rename.connect_clicked(move |_| this.present_rename_tag(&name, &refreshed));

            let this = self.clone();
            let name = tag.id;
            let refreshed = ui.clone();
            remove.connect_clicked(move |_| this.confirm_remove_tag(&name, &refreshed));
        }
    }

    fn present_rename_tag(&self, old_name: &str, managed: &ListScreenBuilt) {
        let name = entry_field("Name", old_name);
        let rename = primary_action("Rename");
        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::new();
        group.add(&name);
        page.add(&group);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
        column.append(&page);
        column.append(&rename);
        let dialog = sheet("Rename Tag", &column, SheetSize::Form);
        dialog.present(Some(&self.inner.window));

        let this = self.clone();
        let old_name = old_name.to_string();
        let managed_list = managed.clone();
        rename.connect_clicked(move |_| {
            let result = this.with_store_mut(|vfs, store| {
                rename_tag_logged(
                    vfs,
                    store,
                    &this.inner.device,
                    &old_name,
                    name.text().as_str(),
                )
            });
            match result {
                Some(Ok(changed)) => {
                    this.record(
                        LogLevel::Info,
                        "tag",
                        format!("Renamed tag on {changed} contacts"),
                    );
                    dialog.close();
                    this.refill_tag_filter();
                    this.refill_list();
                    this.refill_tag_management(&managed_list);
                }
                Some(Err(err)) => {
                    this.record(LogLevel::Error, "tag", "Could not rename tag");
                    this.toast(&err.to_string());
                }
                None => this.toast("No folder selected. Please select a contacts folder first."),
            }
        });
    }

    fn confirm_remove_tag(&self, tag: &str, managed: &ListScreenBuilt) {
        let dialog = confirm_dialog(&ConfirmData {
            question: format!("Remove tag “{tag}” from every contact?"),
            destructive_label: "Remove".into(),
        });
        let this = self.clone();
        let tag = tag.to_string();
        let managed_list = managed.clone();
        dialog.connect_response(None, move |_, response| {
            if response != "confirm" {
                return;
            }
            let result = this.with_store_mut(|vfs, store| {
                remove_tag_logged(vfs, store, &this.inner.device, &tag)
            });
            match result {
                Some(Ok(changed)) => {
                    this.record(
                        LogLevel::Info,
                        "tag",
                        format!("Removed tag from {changed} contacts"),
                    );
                    this.refill_tag_filter();
                    this.refill_list();
                    this.refill_tag_management(&managed_list);
                }
                Some(Err(err)) => {
                    this.record(LogLevel::Error, "tag", "Could not remove tag");
                    this.toast(&err.to_string());
                }
                None => this.toast("No folder selected. Please select a contacts folder first."),
            }
        });
        dialog.present(Some(&self.inner.window));
    }

    fn present_logs(&self) {
        self.record(LogLevel::Info, "diagnostics", "Opened app diagnostics");

        let ui = list_screen(&ListScreen {
            search: true,
            filter: Some(Filter::Scope(vec![
                "All levels".into(),
                "Info".into(),
                "Warning".into(),
                "Error".into(),
            ])),
            sections: vec![ListSection { heading: None }],
            ..ListScreen::default()
        });
        let search = ui.search.clone().expect("logs search");
        search.set_placeholder_text(Some("Message or category"));
        let level = match ui.filter.clone().expect("logs filter") {
            FilterControl::Scope(group) => group,
            FilterControl::Choice(_) => panic!("logs filter is a fixed Scope, not Choice"),
        };
        let clear = action_row(&ActionRowData {
            label: "Clear".into(),
            role: ActionRole::Destructive,
            enabled: true,
        });
        ui.controls.append(&clear);

        let query = Rc::new(RefCell::new(String::new()));
        let selected_level = Rc::new(Cell::new(None::<LogLevel>));
        self.refill_logs(&ui, "", None);

        let searched = self.clone();
        let searched_ui = ui.clone();
        let searched_query = query.clone();
        let searched_level = selected_level.clone();
        search.connect_search_changed(move |entry| {
            searched_query.replace(entry.text().to_string());
            searched.refill_logs(&searched_ui, &searched_query.borrow(), searched_level.get());
        });

        let filtered = self.clone();
        let filtered_ui = ui.clone();
        let filtered_query = query.clone();
        let filtered_level = selected_level.clone();
        level.connect_active_notify(move |group| {
            let selected = match group.active() {
                1 => Some(LogLevel::Info),
                2 => Some(LogLevel::Warning),
                3 => Some(LogLevel::Error),
                _ => None,
            };
            filtered_level.set(selected);
            filtered.refill_logs(&filtered_ui, &filtered_query.borrow(), selected);
        });

        let cleared = self.clone();
        let cleared_ui = ui.clone();
        let cleared_query = query;
        let cleared_level = selected_level;
        clear.connect_clicked(move |_| {
            cleared.inner.diagnostics.borrow_mut().clear();
            cleared.refill_logs(&cleared_ui, &cleared_query.borrow(), cleared_level.get());
        });

        self.push_settings_page("Logs", route_id(ContactsScreen::Logs), &ui.root);
    }

    fn push_settings_page(&self, title: &str, route: &str, body: &impl IsA<gtk::Widget>) {
        if self.inner.settings_dialog.borrow().is_none() {
            self.present_settings();
        }
        if let Some(dialog) = self.inner.settings_dialog.borrow().as_ref() {
            push_settings_subpage(dialog, title, route, body);
        }
    }

    fn refill_logs(&self, ui: &ListScreenBuilt, query: &str, level: Option<LogLevel>) {
        let list = ui.lists.first().expect("logs section");
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }
        let diagnostics = self.inner.diagnostics.borrow();
        let entries = diagnostics.filtered(query, level);
        if entries.is_empty() {
            ui.apply_empty(
                EmptyKind::NoMatches,
                &EmptyCopy {
                    title: "No matching app diagnostics".into(),
                    description: None,
                    action: None,
                },
            );
            return;
        }
        ui.show_lists();
        for entry in entries {
            list.append(&text_row(&TextRowData {
                title: entry.message.clone(),
                subtitle: Some(format!("{} · {}", entry.time_label(), entry.category)),
                trailing: Some(entry.level.label().into()),
                leading: None,
            }));
        }
    }

    fn refill_settings(&self) {}

    fn settings_spec(&self) -> SettingsScreen {
        let folder = self.inner.folder.borrow().clone();
        let store = self.inner.store.borrow();
        let contact_count = store.as_ref().map(|store| store.cards().len()).unwrap_or(0);
        let tag_count = store
            .as_ref()
            .map(|store| tag_rows(store).len())
            .unwrap_or(0);
        drop(store);

        let change = adw::ActionRow::builder()
            .title("Change Folder")
            .activatable(true)
            .build();
        change.add_suffix(&gtk::Image::from_icon_name("folder-open-symbolic"));
        let this = self.clone();
        change.connect_activated(move |_| this.pick_folder());

        let reload = adw::ActionRow::builder()
            .title("Reload")
            .activatable(true)
            .sensitive(folder.is_some())
            .build();
        reload.add_suffix(&gtk::Image::from_icon_name("view-refresh-symbolic"));
        let this = self.clone();
        reload.connect_activated(move |_| this.reload_folder());
        let scan = self
            .inner
            .work
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|work| work.display().clone()))
            .unwrap_or_else(idle_contacts_progress);
        let scan_row = progress_row(&ProgressRowData::from(&scan));
        self.inner.scan_progress_row.replace(Some(scan_row.clone()));

        let tags = nav_row(&NavRowData {
            label: "Tags".into(),
            trailing: Some(tag_count.to_string()),
        });
        let this = self.clone();
        tags.connect_activated(move |_| this.present_tag_management());

        let logs = nav_row(&NavRowData {
            label: "Logs".into(),
            trailing: Some("Info, warnings, and errors".into()),
        });
        let this = self.clone();
        logs.connect_activated(move |_| this.present_logs());

        SettingsScreen {
            groups: vec![
                SettingsGroup {
                    id: "folder".into(),
                    title: "Folder".into(),
                    rows: vec![
                        text_row(&TextRowData {
                            title: "Folder".into(),
                            subtitle: folder,
                            trailing: None,
                            leading: None,
                        })
                        .upcast(),
                        change.upcast(),
                        reload.upcast(),
                    ],
                },
                SettingsGroup {
                    id: "reload".into(),
                    title: "Reload".into(),
                    rows: vec![scan_row.upcast()],
                },
                SettingsGroup {
                    id: "tags".into(),
                    title: "Tags".into(),
                    rows: vec![tags.upcast()],
                },
                SettingsGroup {
                    id: "diagnostics".into(),
                    title: "Diagnostics".into(),
                    rows: vec![logs.upcast()],
                },
                SettingsGroup {
                    id: "info".into(),
                    title: "Info".into(),
                    rows: vec![
                        text_row(&TextRowData {
                            title: "Device".into(),
                            subtitle: Some(self.inner.device.clone()),
                            trailing: None,
                            leading: None,
                        })
                        .upcast(),
                        text_row(&TextRowData {
                            title: "Contacts".into(),
                            subtitle: None,
                            trailing: Some(contact_count.to_string()),
                            leading: None,
                        })
                        .upcast(),
                        text_row(&TextRowData {
                            title: "Version".into(),
                            subtitle: None,
                            trailing: Some(env!("CARGO_PKG_VERSION").into()),
                            leading: None,
                        })
                        .upcast(),
                        text_row(&TextRowData {
                            title: "Apple Contacts".into(),
                            subtitle: Some("Apple Contacts sync is available on iOS.".into()),
                            trailing: None,
                            leading: None,
                        })
                        .upcast(),
                    ],
                },
            ],
        }
    }
}

#[derive(Clone)]
struct ContactEditForm {
    scroller: gtk::ScrolledWindow,
    full_name: adw::EntryRow,
    family_name: adw::EntryRow,
    given_name: adw::EntryRow,
    middle_name: adw::EntryRow,
    name_prefix: adw::EntryRow,
    name_suffix: adw::EntryRow,
    organization: adw::EntryRow,
    job_title: adw::EntryRow,
    nickname: adw::EntryRow,
    urls: LabeledValueList,
    phones: LabeledValueList,
    emails: LabeledValueList,
    addresses: AddressList,
    birthday_enabled: gtk::CheckButton,
    birthday_year: adw::EntryRow,
    birthday_month: adw::EntryRow,
    birthday_day: adw::EntryRow,
    note: gtk::TextView,
    categories: StringList,
    photo: Rc<RefCell<Option<Vec<u8>>>>,
}

impl ContactEditForm {
    fn new(draft: &ContactEditDraft, owner: &Window) -> Self {
        let page = adw::PreferencesPage::new();

        let names = adw::PreferencesGroup::new();
        names.set_title("Name");
        let full_name = entry_field("Display Name", &draft.full_name);
        let family_name = entry_field("Family Name", &draft.family_name);
        let given_name = entry_field("Given Name", &draft.given_name);
        let middle_name = entry_field("Middle Name", &draft.middle_name);
        let name_prefix = entry_field("Prefix", &draft.name_prefix);
        let name_suffix = entry_field("Suffix", &draft.name_suffix);
        for entry in [
            &full_name,
            &family_name,
            &given_name,
            &middle_name,
            &name_prefix,
            &name_suffix,
        ] {
            names.add(entry);
        }
        bind_derived_full_name(&full_name, &given_name, &middle_name, &family_name);
        page.add(&names);

        let work = adw::PreferencesGroup::new();
        work.set_title("Organization");
        let organization = entry_field("Organization", &draft.organization);
        let job_title = entry_field("Job Title", &draft.job_title);
        let nickname = entry_field("Nickname", &draft.nickname);
        for entry in [&organization, &job_title, &nickname] {
            work.add(entry);
        }
        page.add(&work);

        let (phones_group, phones) = labeled_value_group("Phones", "Phone number", &draft.phones);
        page.add(&phones_group);
        let (emails_group, emails) =
            labeled_value_group("Email Addresses", "Email address", &draft.emails);
        page.add(&emails_group);
        let (urls_group, urls) = labeled_value_group("URLs", "URL", &draft.urls);
        page.add(&urls_group);

        let (addresses_group, addresses) = address_group(&draft.addresses);
        page.add(&addresses_group);

        let birthday_group = adw::PreferencesGroup::new();
        birthday_group.set_title("Birthday");
        let birthday_enabled = gtk::CheckButton::with_label("Include birthday");
        birthday_enabled.set_active(draft.birthday.is_some());
        let birthday_year = entry_field(
            "Year (optional)",
            &draft
                .birthday
                .as_ref()
                .and_then(|birthday| birthday.year)
                .map(|year| year.to_string())
                .unwrap_or_default(),
        );
        let birthday_month = entry_field(
            "Month",
            &draft
                .birthday
                .as_ref()
                .map(|birthday| birthday.month.to_string())
                .unwrap_or_default(),
        );
        let birthday_day = entry_field(
            "Day",
            &draft
                .birthday
                .as_ref()
                .map(|birthday| birthday.day.to_string())
                .unwrap_or_default(),
        );
        for entry in [&birthday_year, &birthday_month, &birthday_day] {
            entry.set_sensitive(birthday_enabled.is_active());
        }
        let year = birthday_year.clone();
        let month = birthday_month.clone();
        let day = birthday_day.clone();
        birthday_enabled.connect_toggled(move |enabled| {
            for entry in [&year, &month, &day] {
                entry.set_sensitive(enabled.is_active());
            }
        });
        birthday_group.add(&birthday_enabled);
        birthday_group.add(&birthday_year);
        birthday_group.add(&birthday_month);
        birthday_group.add(&birthday_day);
        page.add(&birthday_group);

        let notes = adw::PreferencesGroup::new();
        notes.set_title("Notes");
        let note = gtk::TextView::new();
        note.set_wrap_mode(gtk::WrapMode::WordChar);
        note.buffer().set_text(&draft.note);
        let note_scroll = gtk::ScrolledWindow::builder()
            .child(&note)
            .min_content_height(100)
            .build();
        notes.add(&note_scroll);
        page.add(&notes);

        let (categories_group, categories) = string_list_group("Tags", &draft.categories);
        page.add(&categories_group);

        let photo_group = adw::PreferencesGroup::new();
        photo_group.set_title("Photo");
        let photo_row = adw::ActionRow::builder().title("Photo").build();
        set_photo_status(&photo_row, draft.photo.as_deref());
        let choose_photo = gtk::Button::with_label("Choose JPEG");
        let remove_photo = gtk::Button::with_label("Remove");
        remove_photo.add_css_class("destructive-action");
        photo_row.add_suffix(&choose_photo);
        photo_row.add_suffix(&remove_photo);
        photo_group.add(&photo_row);
        page.add(&photo_group);

        let photo = Rc::new(RefCell::new(draft.photo.clone()));
        let selected_photo = photo.clone();
        let status = photo_row.clone();
        let window = owner.inner.window.clone();
        let reporter = owner.clone();
        choose_photo.connect_clicked(move |_| {
            let picker = gtk::FileDialog::builder()
                .title("Choose Contact Photo")
                .modal(true)
                .build();
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("JPEG images"));
            filter.add_mime_type("image/jpeg");
            filter.add_suffix("jpg");
            filter.add_suffix("jpeg");
            picker.set_default_filter(Some(&filter));
            let selected_photo = selected_photo.clone();
            let status = status.clone();
            let reporter = reporter.clone();
            picker.open(
                Some(&window),
                gio::Cancellable::NONE,
                move |result| match result {
                    Ok(file) => match file.path().map(std::fs::read) {
                        Some(Ok(bytes)) if bytes.starts_with(&[0xff, 0xd8, 0xff]) => {
                            set_photo_status(&status, Some(&bytes));
                            selected_photo.replace(Some(bytes));
                        }
                        Some(Ok(_)) => reporter.toast("Choose a JPEG image"),
                        Some(Err(err)) => {
                            reporter.record(
                                LogLevel::Error,
                                "photo",
                                "Could not read contact photo",
                            );
                            reporter.toast(&err.to_string());
                        }
                        None => reporter.toast("The selected photo is not a local file"),
                    },
                    Err(err) => {
                        let message = err.to_string();
                        if !message.contains("Dismissed") && !message.contains("dismissed") {
                            reporter.record(
                                LogLevel::Error,
                                "photo",
                                "Contact photo picker failed",
                            );
                            reporter.toast(&message);
                        }
                    }
                },
            );
        });
        let removed_photo = photo.clone();
        let status = photo_row;
        remove_photo.connect_clicked(move |_| {
            removed_photo.replace(None);
            set_photo_status(&status, None);
        });

        let scroller = gtk::ScrolledWindow::builder()
            .child(&page)
            .min_content_height(520)
            .vexpand(true)
            .build();

        Self {
            scroller,
            full_name,
            family_name,
            given_name,
            middle_name,
            name_prefix,
            name_suffix,
            organization,
            job_title,
            nickname,
            urls,
            phones,
            emails,
            addresses,
            birthday_enabled,
            birthday_year,
            birthday_month,
            birthday_day,
            note,
            categories,
            photo,
        }
    }

    fn fill_draft(&self, mut draft: ContactEditDraft) -> Result<ContactEditDraft, String> {
        draft.full_name = self.full_name.text().to_string();
        draft.family_name = self.family_name.text().to_string();
        draft.given_name = self.given_name.text().to_string();
        draft.middle_name = self.middle_name.text().to_string();
        draft.name_prefix = self.name_prefix.text().to_string();
        draft.name_suffix = self.name_suffix.text().to_string();
        draft.organization = self.organization.text().to_string();
        draft.job_title = self.job_title.text().to_string();
        draft.nickname = self.nickname.text().to_string();
        draft.urls = self.urls.values();
        draft.phones = self.phones.values();
        draft.emails = self.emails.values();
        draft.addresses = self.addresses.values();
        draft.birthday = if self.birthday_enabled.is_active() {
            let year_text = self.birthday_year.text();
            let year = if year_text.trim().is_empty() {
                None
            } else {
                Some(
                    year_text
                        .trim()
                        .parse::<i32>()
                        .map_err(|_| "Birthday year must be a number".to_string())?,
                )
            };
            let month = self
                .birthday_month
                .text()
                .trim()
                .parse::<u8>()
                .map_err(|_| "Birthday month must be a number".to_string())?;
            let day = self
                .birthday_day
                .text()
                .trim()
                .parse::<u8>()
                .map_err(|_| "Birthday day must be a number".to_string())?;
            Some(BirthdayDraft { year, month, day })
        } else {
            None
        };
        let buffer = self.note.buffer();
        draft.note = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), true)
            .to_string();
        draft.categories = self.categories.values();
        draft.photo = self.photo.borrow().clone();
        Ok(draft)
    }
}

#[derive(Clone)]
struct LabeledValueList {
    container: gtk::Box,
    rows: Rc<RefCell<Vec<LabeledValueEditor>>>,
    value_placeholder: String,
}

#[derive(Clone)]
struct LabeledValueEditor {
    widget: gtk::Box,
    label: gtk::Entry,
    value: gtk::Entry,
}

impl LabeledValueList {
    fn new(value_placeholder: &str) -> Self {
        Self {
            container: gtk::Box::new(gtk::Orientation::Vertical, 6),
            rows: Rc::new(RefCell::new(Vec::new())),
            value_placeholder: value_placeholder.to_string(),
        }
    }

    fn append(&self, initial: LabeledValueDraft) {
        let widget = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let label = gtk::Entry::builder()
            .placeholder_text("Label")
            .width_chars(10)
            .build();
        label.set_text(&initial.label);
        let value = gtk::Entry::builder()
            .placeholder_text(&self.value_placeholder)
            .hexpand(true)
            .build();
        value.set_text(&initial.value);
        let remove = gtk::Button::from_icon_name("list-remove-symbolic");
        remove.set_tooltip_text(Some("Remove row"));
        widget.append(&label);
        widget.append(&value);
        widget.append(&remove);
        self.container.append(&widget);
        self.rows.borrow_mut().push(LabeledValueEditor {
            widget: widget.clone(),
            label,
            value,
        });

        let list = self.clone();
        remove.connect_clicked(move |_| {
            list.container.remove(&widget);
            list.rows.borrow_mut().retain(|row| row.widget != widget);
        });
    }

    fn values(&self) -> Vec<LabeledValueDraft> {
        self.rows
            .borrow()
            .iter()
            .map(|row| LabeledValueDraft {
                label: row.label.text().to_string(),
                value: row.value.text().to_string(),
            })
            .collect()
    }
}

fn labeled_value_group(
    title: &str,
    value_placeholder: &str,
    rows: &[LabeledValueDraft],
) -> (adw::PreferencesGroup, LabeledValueList) {
    let group = adw::PreferencesGroup::new();
    group.set_title(title);
    let list = LabeledValueList::new(value_placeholder);
    for row in rows {
        list.append(row.clone());
    }
    let add = gtk::Button::with_label("Add");
    let added = list.clone();
    add.connect_clicked(move |_| added.append(LabeledValueDraft::default()));
    group.add(&list.container);
    group.add(&add);
    (group, list)
}

#[derive(Clone)]
struct AddressList {
    container: gtk::Box,
    rows: Rc<RefCell<Vec<AddressEditor>>>,
}

#[derive(Clone)]
struct AddressEditor {
    widget: gtk::Frame,
    label: gtk::Entry,
    street: gtk::Entry,
    city: gtk::Entry,
    state: gtk::Entry,
    postal_code: gtk::Entry,
    country: gtk::Entry,
}

impl AddressList {
    fn new() -> Self {
        Self {
            container: gtk::Box::new(gtk::Orientation::Vertical, 6),
            rows: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn append(&self, initial: LabeledAddressDraft) {
        let widget = gtk::Frame::new(None);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
        column.set_margin_top(6);
        column.set_margin_bottom(6);
        column.set_margin_start(6);
        column.set_margin_end(6);
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let label = gtk::Entry::builder()
            .placeholder_text("Label")
            .hexpand(true)
            .build();
        label.set_text(&initial.label);
        let remove = gtk::Button::from_icon_name("list-remove-symbolic");
        remove.set_tooltip_text(Some("Remove address"));
        header.append(&label);
        header.append(&remove);
        column.append(&header);

        let grid = gtk::Grid::builder()
            .column_spacing(6)
            .row_spacing(6)
            .build();
        let street = address_entry("Street", &initial.street);
        let city = address_entry("City", &initial.city);
        let state = address_entry("State / Region", &initial.state);
        let postal_code = address_entry("Postal Code", &initial.postal_code);
        let country = address_entry("Country", &initial.country);
        grid.attach(&street, 0, 0, 2, 1);
        grid.attach(&city, 0, 1, 1, 1);
        grid.attach(&state, 1, 1, 1, 1);
        grid.attach(&postal_code, 0, 2, 1, 1);
        grid.attach(&country, 1, 2, 1, 1);
        column.append(&grid);
        widget.set_child(Some(&column));
        self.container.append(&widget);
        self.rows.borrow_mut().push(AddressEditor {
            widget: widget.clone(),
            label,
            street,
            city,
            state,
            postal_code,
            country,
        });

        let list = self.clone();
        remove.connect_clicked(move |_| {
            list.container.remove(&widget);
            list.rows.borrow_mut().retain(|row| row.widget != widget);
        });
    }

    fn values(&self) -> Vec<LabeledAddressDraft> {
        self.rows
            .borrow()
            .iter()
            .map(|row| LabeledAddressDraft {
                label: row.label.text().to_string(),
                street: row.street.text().to_string(),
                city: row.city.text().to_string(),
                state: row.state.text().to_string(),
                postal_code: row.postal_code.text().to_string(),
                country: row.country.text().to_string(),
            })
            .collect()
    }
}

fn address_group(rows: &[LabeledAddressDraft]) -> (adw::PreferencesGroup, AddressList) {
    let group = adw::PreferencesGroup::new();
    group.set_title("Addresses");
    let list = AddressList::new();
    for row in rows {
        list.append(row.clone());
    }
    let add = gtk::Button::with_label("Add Address");
    let added = list.clone();
    add.connect_clicked(move |_| added.append(LabeledAddressDraft::default()));
    group.add(&list.container);
    group.add(&add);
    (group, list)
}

fn address_entry(placeholder: &str, value: &str) -> gtk::Entry {
    let entry = gtk::Entry::builder()
        .placeholder_text(placeholder)
        .hexpand(true)
        .build();
    entry.set_text(value);
    entry
}

#[derive(Clone)]
struct StringList {
    container: gtk::Box,
    rows: Rc<RefCell<Vec<StringEditor>>>,
}

#[derive(Clone)]
struct StringEditor {
    widget: gtk::Box,
    value: gtk::Entry,
}

impl StringList {
    fn new() -> Self {
        Self {
            container: gtk::Box::new(gtk::Orientation::Vertical, 6),
            rows: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn append(&self, initial: &str) {
        let widget = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let value = gtk::Entry::builder()
            .placeholder_text("Tag")
            .hexpand(true)
            .build();
        value.set_text(initial);
        let remove = gtk::Button::from_icon_name("list-remove-symbolic");
        remove.set_tooltip_text(Some("Remove tag"));
        widget.append(&value);
        widget.append(&remove);
        self.container.append(&widget);
        self.rows.borrow_mut().push(StringEditor {
            widget: widget.clone(),
            value,
        });

        let list = self.clone();
        remove.connect_clicked(move |_| {
            list.container.remove(&widget);
            list.rows.borrow_mut().retain(|row| row.widget != widget);
        });
    }

    fn values(&self) -> Vec<String> {
        self.rows
            .borrow()
            .iter()
            .map(|row| row.value.text().to_string())
            .collect()
    }
}

fn string_list_group(title: &str, values: &[String]) -> (adw::PreferencesGroup, StringList) {
    let group = adw::PreferencesGroup::new();
    group.set_title(title);
    let list = StringList::new();
    for value in values {
        list.append(value);
    }
    let add = gtk::Button::with_label("Add Tag");
    let added = list.clone();
    add.connect_clicked(move |_| added.append(""));
    group.add(&list.container);
    group.add(&add);
    (group, list)
}

fn set_photo_status(row: &adw::ActionRow, photo: Option<&[u8]>) {
    let status = match photo {
        Some(bytes) => format!("{} bytes", bytes.len()),
        None => "No photo".into(),
    };
    row.set_subtitle(&status);
}

/// Keep Display Name aligned with Given/Middle/Family until the user edits FN.
fn bind_derived_full_name(
    full_name: &adw::EntryRow,
    given_name: &adw::EntryRow,
    middle_name: &adw::EntryRow,
    family_name: &adw::EntryRow,
) {
    let last = Rc::new(RefCell::new(contacts_core::structured_name(
        &given_name.text(),
        &middle_name.text(),
        &family_name.text(),
    )));
    for part in [given_name, middle_name, family_name] {
        let full_name = full_name.clone();
        let given_name = given_name.clone();
        let middle_name = middle_name.clone();
        let family_name = family_name.clone();
        let last = last.clone();
        part.connect_changed(move |_| {
            let composed = contacts_core::structured_name(
                &given_name.text(),
                &middle_name.text(),
                &family_name.text(),
            );
            let current = full_name.text().to_string();
            let previous = last.borrow().clone();
            if current.is_empty() || current == previous {
                full_name.set_text(&composed);
            }
            *last.borrow_mut() = composed;
        });
    }
}

fn entry_field(label: &str, value: &str) -> adw::EntryRow {
    field_row_widget(&FieldRowData {
        label: label.into(),
        value: value.into(),
        editable: true,
    })
    .downcast::<adw::EntryRow>()
    .expect("editable field-row")
}

/// Letter section key from a contact title. `#` for names that do not start
/// with a letter. Initials for the avatar come from the same title.
fn contact_detail_body(
    name: &str,
    org: Option<&str>,
    fields: &[contacts_core::FieldRow],
) -> gtk::Widget {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 18);
    column.set_margin_top(24);
    column.set_margin_bottom(24);
    column.set_margin_start(12);
    column.set_margin_end(12);

    let hero = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let avatar = adw::Avatar::new(96, Some(name), true);
    avatar.set_halign(gtk::Align::Center);
    let title = gtk::Label::new(Some(name));
    title.add_css_class("title-2");
    title.set_wrap(true);
    title.set_justify(gtk::Justification::Center);
    hero.append(&avatar);
    hero.append(&title);
    if let Some(org) = org.filter(|value| !value.is_empty()) {
        let org_label = gtk::Label::new(Some(org));
        org_label.add_css_class("dim-label");
        org_label.set_wrap(true);
        hero.append(&org_label);
    }
    column.append(&hero);

    let mut phones = Vec::new();
    let mut emails = Vec::new();
    let mut urls = Vec::new();
    let mut addresses = Vec::new();
    let mut other = Vec::new();
    let mut categories = Vec::new();
    for field in fields {
        match field.id.as_deref().unwrap_or_default() {
            "fn" | "org" | "photo" => {}
            id if id.starts_with("tel:") => phones.push(field),
            id if id.starts_with("email:") => emails.push(field),
            id if id.starts_with("url:") => urls.push(field),
            id if id.starts_with("adr:") => addresses.push(field),
            id if id.starts_with("category:") => categories.push(field),
            _ => other.push(field),
        }
    }
    for (title, rows) in [
        ("Phone", phones),
        ("Email", emails),
        ("URL", urls),
        ("Address", addresses),
        ("Details", other),
    ] {
        if rows.is_empty() {
            continue;
        }
        let group = adw::PreferencesGroup::new();
        group.set_title(title);
        for field in rows {
            group.add(&field_row(&FieldRowData {
                label: field.label.clone(),
                value: field.value.clone(),
                editable: false,
            }));
        }
        column.append(&group);
    }
    if !categories.is_empty() {
        let chips: Vec<Chip> = categories
            .iter()
            .map(|field| Chip {
                label: field.value.clone(),
                mode: ChipMode::Display,
                icon: None,
            })
            .collect();
        column.append(&chip_bar(&chips));
    }

    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(720);
    clamp.set_tightening_threshold(480);
    clamp.set_child(Some(&column));
    clamp.upcast()
}

fn idle_contacts_progress() -> ProgressDisplay {
    ProgressDisplay {
        label: "Reload Folder".into(),
        detail: None,
        fraction: None,
        cancel: false,
    }
}

fn section_key(title: &str) -> String {
    title
        .chars()
        .find(|ch| ch.is_alphabetic())
        .map(|ch| ch.to_uppercase().to_string())
        .filter(|key| key.chars().next().is_some_and(char::is_alphabetic))
        .unwrap_or_else(|| "#".into())
}
