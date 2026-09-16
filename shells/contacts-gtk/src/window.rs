//! Laptop chrome or the Comet collapse. Bindings come from `shell-kit-gtk`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;

use contacts_core::{
    apply_draft, choice_rows, conflict_rows, delete_logged, draft_from_card, field_rows, list_rows,
    resolve_logged, save_logged, write, Card, ContactDraft, MergeKind, StdVfs, Store, Vfs,
    TEMP_PREFIX,
};
use shell_kit_gtk::{
    action_row, apply_token_css, banner, confirm_dialog, field_row, field_row_widget, list_page,
    nav_row, primary_action, push_page, search_entry, settings_page, sheet, status_row, text_row,
    ActionRole, ActionRowData, ConfirmData, ContactsScreen, FieldRowData, NavRowData,
    StatusRowData, StatusSeverity, TextRowData,
};

use crate::routing::route_id;
use crate::{hostname, LogLevel, LogStore, Paths, APP_TITLE, COMPACT_WIDTH};

const TOKEN_CSS: &str = include_str!("../../../design/tokens/generated/contacts.css");
const ADW_ACCENT: &str =
    "@define-color accent_bg_color var(--accent);\n@define-color accent_color var(--accent);\n";
const DIAGNOSTIC_CAPACITY: usize = 5_000;

#[derive(Clone)]
pub struct Window {
    inner: Rc<Inner>,
}

struct Inner {
    window: adw::ApplicationWindow,
    header: adw::HeaderBar,
    switcher: adw::ViewSwitcher,
    switcher_bar: adw::ViewSwitcherBar,
    nav: adw::NavigationView,
    root_page: adw::NavigationPage,
    root_stack: gtk::Stack,
    list: gtk::ListBox,
    banner: adw::Banner,
    add: gtk::Button,
    settings_nav: adw::NavigationView,
    settings_box: gtk::Box,
    toast: adw::ToastOverlay,
    vfs: StdVfs,
    device: String,
    paths: Paths,
    store: RefCell<Option<Store>>,
    folder: RefCell<Option<String>>,
    query: RefCell<String>,
    diagnostics: RefCell<LogStore>,
}

impl Window {
    pub fn present(app: &adw::Application, comet: bool) {
        let this = Self::build(app, comet);
        apply_token_css(&format!("{TOKEN_CSS}\n{ADW_ACCENT}"));
        this.inner.window.present();
        this.load_persisted();
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

        let list_col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        list_col.set_widget_name(route_id(ContactsScreen::ContactList));
        let conflict_banner = banner("");
        conflict_banner.set_revealed(false);
        conflict_banner.set_button_label(Some("Review"));
        let search = search_entry();
        search.set_placeholder_text(Some("Name, company, phone, or email"));
        search.set_hexpand(true);
        let scroll = list_page();
        let list = scroll
            .child()
            .and_downcast::<gtk::ListBox>()
            .expect("list_page child");
        list_col.append(&conflict_banner);
        list_col.append(&search);
        list_col.append(&scroll);

        let root_stack = gtk::Stack::new();
        root_stack.add_named(&picker, Some(route_id(ContactsScreen::FolderPicker)));
        root_stack.add_named(&list_col, Some(route_id(ContactsScreen::ContactList)));
        root_stack.set_visible_child_name(route_id(ContactsScreen::FolderPicker));

        let nav = shell_kit_gtk::navigation_view();
        let root_page = push_page("Contacts", &root_stack);
        root_page.set_widget_name(route_id(ContactsScreen::FolderPicker));
        nav.add(&root_page);

        let settings_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let settings_nav = shell_kit_gtk::navigation_view();
        let settings_root = push_page("Settings", &settings_box);
        settings_root.set_widget_name(route_id(ContactsScreen::Settings));
        settings_nav.add(&settings_root);
        let stack = adw::ViewStack::new();
        let contacts_page = stack.add_titled(&nav, Some("contacts"), "Contacts");
        contacts_page.set_icon_name(Some("avatar-default-symbolic"));
        let settings_page = stack.add_titled(
            &settings_nav,
            Some(route_id(ContactsScreen::Settings)),
            "Settings",
        );
        settings_page.set_icon_name(Some("emblem-system-symbolic"));

        let switcher = adw::ViewSwitcher::new();
        switcher.set_stack(Some(&stack));
        switcher.set_policy(adw::ViewSwitcherPolicy::Wide);

        let switcher_bar = adw::ViewSwitcherBar::new();
        switcher_bar.set_stack(Some(&stack));

        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&switcher));

        let add = primary_action("Add");
        add.set_sensitive(false);
        header.pack_end(&add);

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&stack));
        toolbar.add_bottom_bar(&switcher_bar);

        let toast = adw::ToastOverlay::new();
        toast.set_child(Some(&toolbar));
        window.set_content(Some(&toast));

        let paths = Paths::from_env();
        let device = paths.load_or_create_device_id(&hostname());

        let inner = Rc::new(Inner {
            window: window.clone(),
            header: header.clone(),
            switcher: switcher.clone(),
            switcher_bar: switcher_bar.clone(),
            nav: nav.clone(),
            root_page: root_page.clone(),
            root_stack: root_stack.clone(),
            list: list.clone(),
            banner: conflict_banner.clone(),
            add: add.clone(),
            settings_nav: settings_nav.clone(),
            settings_box: settings_box.clone(),
            toast,
            vfs: StdVfs::new(TEMP_PREFIX),
            device,
            paths,
            store: RefCell::new(None),
            folder: RefCell::new(None),
            query: RefCell::new(String::new()),
            diagnostics: RefCell::new(LogStore::new(DIAGNOSTIC_CAPACITY)),
        });
        let this = Window { inner };

        let picker_open = this.clone();
        choose.connect_clicked(move |_| picker_open.pick_folder());

        let added = this.clone();
        add.connect_clicked(move |_| added.present_edit(None));

        let review = this.clone();
        conflict_banner.connect_button_clicked(move |_| review.present_conflicts());

        let searched = this.clone();
        search.connect_search_changed(move |entry| {
            searched.inner.query.replace(entry.text().to_string());
            searched.refill_list();
        });

        let opened = this.clone();
        list.connect_row_activated(move |_, row| {
            let id = row.widget_name();
            if !id.is_empty() {
                opened.push_detail(&id);
            }
        });

        let sized = this.clone();
        window.connect_realize(move |w| {
            if let Some(surface) = w.surface() {
                let on_layout = sized.clone();
                surface.connect_layout(move |_, width, _| on_layout.apply_chrome(width));
            }
            sized.apply_chrome(w.width());
        });
        this.apply_chrome(window.default_width());
        this.record(LogLevel::Info, "app", "Application started");
        this.refill_settings();
        this
    }

    fn apply_chrome(&self, width: i32) {
        let compact = width <= COMPACT_WIDTH;
        self.inner.switcher_bar.set_reveal(compact);
        if compact {
            self.inner
                .header
                .set_title_widget(Option::<&gtk::Widget>::None);
            self.inner.window.set_title(Some(APP_TITLE));
        } else {
            self.inner
                .header
                .set_title_widget(Some(&self.inner.switcher));
        }
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

    fn load_persisted(&self) {
        if let Some(folder) = self.inner.paths.load_folder() {
            if std::path::Path::new(&folder).is_dir() {
                self.open_folder(&folder);
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
                    Some(path) => this.open_folder(&path),
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

    fn open_folder(&self, folder: &str) {
        match Store::open(&self.inner.vfs, folder) {
            Ok(store) => {
                let count = store.cards().len();
                self.inner.store.replace(Some(store));
                self.inner.folder.replace(Some(folder.to_string()));
                self.inner.paths.save_folder(folder);
                self.inner
                    .root_stack
                    .set_visible_child_name(route_id(ContactsScreen::ContactList));
                self.inner
                    .root_page
                    .set_widget_name(route_id(ContactsScreen::ContactList));
                self.inner.add.set_sensitive(true);
                self.inner.nav.pop_to_page(&self.inner.root_page);
                self.record(
                    LogLevel::Info,
                    "folder",
                    format!("Opened contacts folder with {count} contacts"),
                );
                self.refill_list();
                self.refill_settings();
            }
            Err(err) => {
                self.record(LogLevel::Error, "folder", "Could not open contacts folder");
                self.toast(&err.to_string());
            }
        }
    }

    fn with_store_mut<T>(&self, f: impl FnOnce(&dyn Vfs, &mut Store) -> T) -> Option<T> {
        let mut slot = self.inner.store.borrow_mut();
        slot.as_mut()
            .map(|store| f(&self.inner.vfs as &dyn Vfs, store))
    }

    fn refill_list(&self) {
        while let Some(child) = self.inner.list.first_child() {
            self.inner.list.remove(&child);
        }
        let Some(store) = self.inner.store.borrow().clone() else {
            return;
        };
        let query = self.inner.query.borrow().clone();
        let rows = list_rows(&store, &query);
        if rows.is_empty() {
            self.inner.list.append(&status_row(&StatusRowData {
                message: "No contacts".into(),
                severity: StatusSeverity::Info,
            }));
        } else {
            for row in rows {
                let widget = text_row(&TextRowData {
                    title: row.title,
                    subtitle: row.subtitle,
                    trailing: None,
                });
                widget.set_activatable(true);
                widget.set_widget_name(&row.id);
                self.inner.list.append(&widget);
            }
        }
        match conflict_rows(&self.inner.vfs, &store) {
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

    fn push_detail(&self, id: &str) {
        let Some(store) = self.inner.store.borrow().clone() else {
            return;
        };
        let Some(card) = store.get(id).cloned() else {
            self.record(LogLevel::Warning, "contact", "Contact was not found");
            self.toast("Not found");
            return;
        };
        if self.inner.nav.navigation_stack().n_items() > 1 {
            self.inner.nav.pop();
        }
        let fields = field_rows(&card);
        let title = fields
            .iter()
            .find(|row| row.id.as_deref() == Some("fn"))
            .map(|row| row.value.clone())
            .unwrap_or_else(|| card.local_id.clone());
        let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
        column.set_margin_top(12);
        column.set_margin_bottom(12);
        column.set_margin_start(12);
        column.set_margin_end(12);
        for field in fields {
            column.append(&field_row(&FieldRowData {
                label: field.label,
                value: field.value,
                editable: false,
            }));
        }
        let edit = action_row(&ActionRowData {
            label: "Edit".into(),
            role: ActionRole::Normal,
            enabled: true,
        });
        let export = action_row(&ActionRowData {
            label: "Export".into(),
            role: ActionRole::Normal,
            enabled: true,
        });
        let delete = action_row(&ActionRowData {
            label: "Delete Contact".into(),
            role: ActionRole::Destructive,
            enabled: true,
        });
        column.append(&edit);
        column.append(&export);
        column.append(&delete);
        let scroll = gtk::ScrolledWindow::builder().child(&column).build();
        let page = push_page(&title, &scroll);
        page.set_widget_name(route_id(ContactsScreen::ContactDetail));
        self.inner.nav.push(&page);

        let editor = self.clone();
        let edit_id = card.local_id.clone();
        edit.connect_clicked(move |_| editor.present_edit(Some(edit_id.clone())));

        let exporter = self.clone();
        let exported = card.clone();
        export.connect_clicked(move |_| exporter.export_card(&exported));

        let deleter = self.clone();
        let delete_id = card.local_id.clone();
        delete.connect_clicked(move |_| deleter.confirm_delete(&delete_id));
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
                    this.inner.nav.pop_to_page(&this.inner.root_page);
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
        dialog.present(&self.inner.window);
    }

    fn present_edit(&self, id: Option<String>) {
        let draft = if let Some(id) = id.as_ref() {
            let Some(store) = self.inner.store.borrow().clone() else {
                return;
            };
            match store.get(id) {
                Some(card) => draft_from_card(card),
                None => {
                    self.record(LogLevel::Warning, "contact", "Contact was not found");
                    self.toast("Not found");
                    return;
                }
            }
        } else {
            ContactDraft::default()
        };

        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::new();
        let given = entry_field("First Name", &draft.given);
        let family = entry_field("Last Name", &draft.family);
        let org = entry_field("Company", &draft.organization);
        let phone = entry_field("Phone", &draft.phone);
        let email = entry_field("Email", &draft.email);
        let note = entry_field("Notes", &draft.note);
        group.add(&given);
        group.add(&family);
        group.add(&org);
        group.add(&phone);
        group.add(&email);
        group.add(&note);
        page.add(&group);

        let save = primary_action("Save");
        let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
        column.append(&page);
        column.append(&save);
        let dialog = sheet("Contact", &column);
        dialog.set_widget_name(route_id(ContactsScreen::ContactEdit));
        dialog.present(&self.inner.window);

        let this = self.clone();
        let existing = draft.id.clone();
        save.connect_clicked(move |_| {
            let filled = ContactDraft {
                id: existing.clone(),
                given: given.text().to_string(),
                family: family.text().to_string(),
                organization: org.text().to_string(),
                phone: phone.text().to_string(),
                email: email.text().to_string(),
                note: note.text().to_string(),
            };
            let mut card = if let Some(id) = filled.id.as_ref() {
                this.inner
                    .store
                    .borrow()
                    .as_ref()
                    .and_then(|store| store.get(id).cloned())
                    .unwrap_or_else(|| Card::new(""))
            } else {
                Card::new("")
            };
            apply_draft(&mut card, &filled);
            let result =
                this.with_store_mut(|vfs, store| save_logged(vfs, store, &this.inner.device, card));
            match result {
                Some(Ok(_)) => {
                    this.record(LogLevel::Info, "contact", "Saved contact");
                    dialog.close();
                    this.inner.nav.pop_to_page(&this.inner.root_page);
                    this.refill_list();
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

    fn export_card(&self, card: &Card) {
        let dialog = gtk::FileDialog::builder()
            .title("Export contact")
            .initial_name(&card.file_name)
            .modal(true)
            .build();
        let text = write(card);
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
        let Some(store) = self.inner.store.borrow().clone() else {
            self.record(
                LogLevel::Warning,
                "conflict",
                "Conflict review requested without a contacts folder",
            );
            return;
        };
        let groups = match conflict_rows(&self.inner.vfs, &store) {
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
        );
        dialog.set_widget_name(route_id(ContactsScreen::SyncConflictGroup));

        for group in groups {
            let row = text_row(&TextRowData {
                title: group.title.clone(),
                subtitle: Some(group.subtitle.clone()),
                trailing: Some(group.trailing.clone()),
            });
            column.append(&row);
            let name = group.id.clone();
            if group.disposition == MergeKind::Choice {
                let choose = action_row(&ActionRowData {
                    label: "Choose fields".into(),
                    role: ActionRole::Normal,
                    enabled: true,
                });
                let this = self.clone();
                let host = dialog.clone();
                choose.connect_clicked(move |_| this.present_choices(&name, Some(&host)));
                column.append(&choose);
            } else {
                let resolve = primary_action("Resolve");
                let this = self.clone();
                let host = dialog.clone();
                resolve.connect_clicked(move |_| {
                    this.resolve_group(&name, &[], Some(&host));
                });
                column.append(&resolve);
            }
        }

        dialog.present(&self.inner.window);
    }

    fn present_choices(&self, canonical: &str, parent: Option<&adw::Dialog>) {
        let Some(store) = self.inner.store.borrow().clone() else {
            self.record(
                LogLevel::Warning,
                "conflict",
                "Conflict choices requested without a contacts folder",
            );
            return;
        };
        let rows = match choice_rows(&self.inner.vfs, &store, canonical) {
            Ok(rows) => rows,
            Err(err) => {
                self.record(
                    LogLevel::Error,
                    "conflict",
                    "Could not load conflict choices",
                );
                self.toast(&err.to_string());
                return;
            }
        };
        let mut fields: Vec<String> = Vec::new();
        let mut by_field: HashMap<String, Vec<contacts_core::TextRow>> = HashMap::new();
        for row in rows {
            if !by_field.contains_key(&row.title) {
                fields.push(row.title.clone());
            }
            by_field.entry(row.title.clone()).or_default().push(row);
        }

        let page = adw::PreferencesPage::new();
        let picks: Rc<RefCell<HashMap<String, String>>> = Rc::new(RefCell::new(HashMap::new()));
        let checks: Rc<RefCell<Vec<(String, String, gtk::Image)>>> =
            Rc::new(RefCell::new(Vec::new()));
        for field in &fields {
            let group = adw::PreferencesGroup::new();
            group.set_title(field);
            for choice in by_field.get(field).into_iter().flatten() {
                let source = choice.subtitle.clone().unwrap_or_default();
                let value = choice.trailing.clone().unwrap_or_default();
                let row = adw::ActionRow::builder()
                    .title(&source)
                    .subtitle(&value)
                    .activatable(true)
                    .build();
                let check = gtk::Image::from_icon_name("object-select-symbolic");
                check.set_visible(false);
                row.add_suffix(&check);
                checks
                    .borrow_mut()
                    .push((field.clone(), choice.id.clone(), check.clone()));
                let picks = picks.clone();
                let checks = checks.clone();
                let field = field.clone();
                let id = choice.id.clone();
                row.connect_activated(move |_| {
                    picks.borrow_mut().insert(field.clone(), id.clone());
                    for (f, choice_id, image) in checks.borrow().iter() {
                        image.set_visible(f == &field && choice_id == &id);
                    }
                });
                group.add(&row);
            }
            page.add(&group);
        }
        let resolve = primary_action("Resolve");
        let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
        column.append(&page);
        column.append(&resolve);
        let dialog = sheet("Choose", &column);
        dialog.set_widget_name(route_id(ContactsScreen::SyncConflictGroup));
        dialog.present(&self.inner.window);

        let this = self.clone();
        let canonical = canonical.to_string();
        let needed = fields.len();
        let parent = parent.cloned();
        resolve.connect_clicked(move |_| {
            let map = picks.borrow().clone();
            if map.len() != needed {
                this.record(
                    LogLevel::Warning,
                    "conflict",
                    "Resolve requested before every field had a choice",
                );
                this.toast("Choose a value for every field");
                return;
            }
            let ids: Vec<String> = map.into_values().collect();
            this.resolve_group(&canonical, &ids, parent.as_ref());
            dialog.close();
        });
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

    fn present_logs(&self) {
        self.record(LogLevel::Info, "diagnostics", "Opened app diagnostics");

        let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        controls.set_margin_top(8);
        controls.set_margin_start(8);
        controls.set_margin_end(8);

        let search = search_entry();
        search.set_placeholder_text(Some("Message or category"));
        search.set_hexpand(true);
        let level = gtk::DropDown::from_strings(&["All levels", "Info", "Warning", "Error"]);
        let clear = action_row(&ActionRowData {
            label: "Clear".into(),
            role: ActionRole::Destructive,
            enabled: true,
        });
        controls.append(&search);
        controls.append(&level);
        controls.append(&clear);

        let scroll = list_page();
        scroll.set_vexpand(true);
        let list = scroll
            .child()
            .and_downcast::<gtk::ListBox>()
            .expect("list_page child");
        column.append(&controls);
        column.append(&scroll);

        let query = Rc::new(RefCell::new(String::new()));
        let selected_level = Rc::new(Cell::new(None::<LogLevel>));
        self.refill_logs(&list, "", None);

        let searched = self.clone();
        let searched_list = list.clone();
        let searched_query = query.clone();
        let searched_level = selected_level.clone();
        search.connect_search_changed(move |entry| {
            searched_query.replace(entry.text().to_string());
            searched.refill_logs(
                &searched_list,
                &searched_query.borrow(),
                searched_level.get(),
            );
        });

        let filtered = self.clone();
        let filtered_list = list.clone();
        let filtered_query = query.clone();
        let filtered_level = selected_level.clone();
        level.connect_selected_notify(move |dropdown| {
            let selected = match dropdown.selected() {
                1 => Some(LogLevel::Info),
                2 => Some(LogLevel::Warning),
                3 => Some(LogLevel::Error),
                _ => None,
            };
            filtered_level.set(selected);
            filtered.refill_logs(&filtered_list, &filtered_query.borrow(), selected);
        });

        let cleared = self.clone();
        let cleared_list = list.clone();
        let cleared_query = query;
        let cleared_level = selected_level;
        clear.connect_clicked(move |_| {
            cleared.inner.diagnostics.borrow_mut().clear();
            cleared.refill_logs(&cleared_list, &cleared_query.borrow(), cleared_level.get());
        });

        let page = push_page("Logs", &column);
        page.set_widget_name(route_id(ContactsScreen::Logs));
        self.inner.settings_nav.push(&page);
    }

    fn refill_logs(&self, list: &gtk::ListBox, query: &str, level: Option<LogLevel>) {
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }
        let diagnostics = self.inner.diagnostics.borrow();
        let entries = diagnostics.filtered(query, level);
        if entries.is_empty() {
            list.append(&status_row(&StatusRowData {
                message: "No matching app diagnostics".into(),
                severity: StatusSeverity::Info,
            }));
            return;
        }
        for entry in entries {
            list.append(&text_row(&TextRowData {
                title: entry.message.clone(),
                subtitle: Some(format!("{} · {}", entry.time_label(), entry.category)),
                trailing: Some(entry.level.label().into()),
            }));
        }
    }

    fn refill_settings(&self) {
        while let Some(child) = self.inner.settings_box.first_child() {
            self.inner.settings_box.remove(&child);
        }
        self.inner.settings_box.append(&self.settings_page_widget());
    }

    fn settings_page_widget(&self) -> adw::PreferencesPage {
        let page = settings_page();
        page.set_widget_name(route_id(ContactsScreen::Settings));
        let folder_group = adw::PreferencesGroup::new();
        folder_group.set_title("Contacts Folder");
        let folder = self.inner.folder.borrow().clone();
        folder_group.add(&text_row(&TextRowData {
            title: "Folder".into(),
            subtitle: folder.clone(),
            trailing: None,
        }));
        let change = adw::ActionRow::builder()
            .title("Change Folder")
            .activatable(true)
            .build();
        change.add_suffix(&gtk::Image::from_icon_name("folder-open-symbolic"));
        let this = self.clone();
        change.connect_activated(move |_| this.pick_folder());
        folder_group.add(&change);
        page.add(&folder_group);

        let diagnostics = adw::PreferencesGroup::new();
        diagnostics.set_title("Diagnostics");
        let logs = nav_row(&NavRowData {
            label: "Logs".into(),
            trailing: Some("Info, warnings, and errors".into()),
        });
        let this = self.clone();
        logs.connect_activated(move |_| this.present_logs());
        diagnostics.add(&logs);
        page.add(&diagnostics);

        let info = adw::PreferencesGroup::new();
        info.set_title("About");
        info.add(&text_row(&TextRowData {
            title: "Device".into(),
            subtitle: Some(self.inner.device.clone()),
            trailing: None,
        }));
        info.add(&text_row(&TextRowData {
            title: "Apple Contacts".into(),
            subtitle: Some("Apple Contacts sync is available on iOS.".into()),
            trailing: None,
        }));
        info.add(&nav_row(&NavRowData {
            label: "LocalContacts".into(),
            trailing: Some("GTK".into()),
        }));
        page.add(&info);
        page
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
