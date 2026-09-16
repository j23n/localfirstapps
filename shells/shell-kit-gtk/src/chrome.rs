//! HIG chrome: primary menu, about/settings dialogs, list|detail split.

use adw::prelude::*;
use gtk::gio;

use crate::builder::SettingsScreenBuilt;
use crate::nav::page;
use crate::{PageChrome, SettingsScreen};

/// One extra item above the standard Settings / About tail.
pub struct MenuCommand {
    pub id: &'static str,
    pub label: String,
}

/// Primary menu (`open-menu-symbolic`, tooltip “Main Menu”).
#[derive(Clone)]
pub struct PrimaryMenu {
    pub button: gtk::MenuButton,
    pub settings: gio::SimpleAction,
    pub about: gio::SimpleAction,
    extra: Vec<gio::SimpleAction>,
    _group: gio::SimpleActionGroup,
}

impl PrimaryMenu {
    #[must_use]
    pub fn extra(&self, id: &str) -> Option<gio::SimpleAction> {
        self.extra
            .iter()
            .find(|action| action.name() == id)
            .cloned()
    }
}

/// Build the HIG primary menu. Last section is always Settings and About.
#[must_use]
pub fn primary_menu(app_name: &str, extra: &[MenuCommand]) -> PrimaryMenu {
    let group = gio::SimpleActionGroup::new();
    let menu = gio::Menu::new();

    let mut extra_actions = Vec::new();
    if !extra.is_empty() {
        let section = gio::Menu::new();
        for command in extra {
            let action = gio::SimpleAction::new(command.id, None);
            group.add_action(&action);
            section.append(Some(&command.label), Some(&format!("menu.{}", command.id)));
            extra_actions.push(action);
        }
        menu.append_section(None, &section);
    }

    let settings = gio::SimpleAction::new("settings", None);
    let about = gio::SimpleAction::new("about", None);
    group.add_action(&settings);
    group.add_action(&about);
    let tail = gio::Menu::new();
    tail.append(Some("Settings"), Some("menu.settings"));
    tail.append(Some(&format!("About {app_name}")), Some("menu.about"));
    menu.append_section(None, &tail);

    let button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Main Menu")
        .primary(true)
        .menu_model(&menu)
        .build();
    button.insert_action_group("menu", Some(&group));

    PrimaryMenu {
        button,
        settings,
        about,
        extra: extra_actions,
        _group: group,
    }
}

/// App About dialog. Label stays the product name (ADR 0007).
#[must_use]
pub fn about_dialog(app_id: &str, app_name: &str, version: &str) -> adw::AboutDialog {
    adw::AboutDialog::builder()
        .application_name(app_name)
        .application_icon(app_id)
        .version(version)
        .developer_name("Johannes")
        .build()
}

/// Settings as a secondary window (`AdwPreferencesDialog`).
#[must_use]
pub fn preferences_dialog(title: &str, built: SettingsScreenBuilt) -> adw::PreferencesDialog {
    let dialog = adw::PreferencesDialog::new();
    dialog.set_title(title);
    dialog.set_search_enabled(false);
    built.page.set_widget_name("settings");
    dialog.add(&built.page);
    dialog
}

/// Assemble [`SettingsScreen`] and wrap it in [`preferences_dialog`].
#[must_use]
pub fn settings_preferences(title: &str, spec: SettingsScreen) -> adw::PreferencesDialog {
    preferences_dialog(title, crate::settings_screen(spec))
}

/// List | object split. Each pane owns one header (HIG browsing / sidebars).
#[derive(Clone)]
pub struct SplitListDetail {
    pub split: adw::NavigationSplitView,
    pub sidebar: adw::NavigationPage,
    pub content: adw::NavigationPage,
    pub sidebar_header: adw::HeaderBar,
    pub content_header: adw::HeaderBar,
    pub sidebar_start: gtk::Box,
    pub sidebar_end: gtk::Box,
    pub content_end: gtk::Box,
    pub content_toolbar: adw::ToolbarView,
    pub sidebar_toolbar: adw::ToolbarView,
}

impl SplitListDetail {
    /// Collapse the split at the existing L12 wide breakpoint (860sp).
    pub fn install(&self, window: &adw::ApplicationWindow) {
        let compact = adw::Breakpoint::new(
            adw::BreakpointCondition::parse("max-width: 860sp").expect("split collapse"),
        );
        compact.add_setter(&self.split, "collapsed", Some(&true.to_value()));
        window.add_breakpoint(compact);
    }

    /// Replace the content pane body and title. Header end is refilled by the app.
    pub fn show_content(&self, title: &str, body: &impl IsA<gtk::Widget>) {
        self.content.set_title(title);
        self.content_toolbar.set_content(Some(body));
        self.split.set_show_content(true);
    }

    pub fn show_sidebar(&self) {
        self.split.set_show_content(false);
    }
}

/// Sidebar list + content placeholder, each in `AdwToolbarView` + header.
#[must_use]
pub fn split_list_detail(
    sidebar_title: &str,
    content_title: &str,
    sidebar_body: &impl IsA<gtk::Widget>,
    content_body: &impl IsA<gtk::Widget>,
) -> SplitListDetail {
    let sidebar_start = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let sidebar_end = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let content_end = gtk::Box::new(gtk::Orientation::Horizontal, 6);

    let sidebar_header = adw::HeaderBar::new();
    sidebar_header.pack_start(&sidebar_start);
    sidebar_header.pack_end(&sidebar_end);

    let sidebar_toolbar = adw::ToolbarView::new();
    sidebar_toolbar.add_top_bar(&sidebar_header);
    sidebar_toolbar.set_content(Some(sidebar_body));
    let sidebar = adw::NavigationPage::builder()
        .title(sidebar_title)
        .tag("sidebar")
        .child(&sidebar_toolbar)
        .build();

    let content_header = adw::HeaderBar::new();
    content_header.pack_end(&content_end);
    let content_toolbar = adw::ToolbarView::new();
    content_toolbar.add_top_bar(&content_header);
    content_toolbar.set_content(Some(content_body));
    let content = adw::NavigationPage::builder()
        .title(content_title)
        .tag("content")
        .child(&content_toolbar)
        .build();

    let split = adw::NavigationSplitView::new();
    split.set_sidebar(Some(&sidebar));
    split.set_content(Some(&content));
    split.set_min_sidebar_width(200.0);
    split.set_max_sidebar_width(320.0);
    split.set_sidebar_width_fraction(0.32);

    SplitListDetail {
        split,
        sidebar,
        content,
        sidebar_header,
        content_header,
        sidebar_start,
        sidebar_end,
        content_end,
        content_toolbar,
        sidebar_toolbar,
    }
}

/// HIG search: hidden bar under the header, header toggle, type-to-search.
#[derive(Clone)]
pub struct SearchChrome {
    pub bar: gtk::SearchBar,
    pub entry: gtk::SearchEntry,
    pub button: gtk::ToggleButton,
}

/// Search button (`edit-find-symbolic`, Adwaita) bound to a [`gtk::SearchBar`].
#[must_use]
pub fn search_chrome(placeholder: &str) -> SearchChrome {
    let entry = crate::search_entry();
    entry.set_placeholder_text(Some(placeholder));
    entry.set_hexpand(true);
    let bar = gtk::SearchBar::new();
    bar.set_show_close_button(true);
    bar.connect_entry(&entry);
    bar.set_child(Some(&entry));
    let button = gtk::ToggleButton::new();
    button.set_icon_name("edit-find-symbolic");
    button.set_tooltip_text(Some("Search"));
    button
        .bind_property("active", &bar, "search-mode-enabled")
        .bidirectional()
        .sync_create()
        .build();
    SearchChrome { bar, entry, button }
}

impl SearchChrome {
    /// Type-to-search when this widget has key focus (usually the window).
    pub fn capture_on(&self, widget: &impl IsA<gtk::Widget>) {
        self.bar.set_key_capture_widget(Some(widget.as_ref()));
    }
}

/// Label-only pill (HIG: icon *or* label outside header bars).
#[must_use]
pub fn pill_primary(label: &str) -> gtk::Button {
    let button = gtk::Button::builder().label(label).build();
    button.add_css_class("pill");
    button.add_css_class("suggested-action");
    button
}

/// Push a titled page onto a preferences dialog (Tags, Logs).
pub fn push_settings_subpage(
    dialog: &adw::PreferencesDialog,
    title: &str,
    route: &str,
    body: &impl IsA<gtk::Widget>,
) {
    let page = page(title, body, PageChrome::pushed());
    page.set_widget_name(route);
    dialog.push_subpage(&page);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_menu_uses_open_menu_icon_and_main_menu_tooltip() {
        crate::with_adw(|| {
            let menu = primary_menu(
                "LocalContacts",
                &[MenuCommand {
                    id: "reload",
                    label: "Reload".into(),
                }],
            );
            assert_eq!(
                menu.button.icon_name().as_deref(),
                Some("open-menu-symbolic")
            );
            assert_eq!(menu.button.tooltip_text().as_deref(), Some("Main Menu"));
            assert!(menu.button.is_primary());
            assert!(menu.extra("reload").is_some());
            assert!(menu.extra("missing").is_none());
        });
    }

    #[test]
    fn split_list_detail_has_two_headers() {
        crate::with_adw(|| {
            let split = split_list_detail(
                "Contacts",
                "Select a Contact",
                &gtk::Label::new(Some("list")),
                &gtk::Label::new(Some("empty")),
            );
            assert_eq!(split.sidebar.title().as_str(), "Contacts");
            assert_eq!(split.content.title().as_str(), "Select a Contact");
            split.show_content("Ada", &gtk::Label::new(Some("ada")));
            assert_eq!(split.content.title().as_str(), "Ada");
            assert!(split.split.shows_content());
        });
    }

    #[test]
    fn pill_primary_is_label_only() {
        crate::with_adw(|| {
            let button = pill_primary("Play All");
            assert_eq!(button.label().as_deref(), Some("Play All"));
            assert!(button.has_css_class("pill"));
            assert!(button.has_css_class("suggested-action"));
        });
    }

    #[test]
    fn search_chrome_uses_adwaita_find_icon_and_full_width_entry() {
        crate::with_adw(|| {
            let search = search_chrome("Name or phone");
            assert_eq!(
                search.button.icon_name().as_deref(),
                Some("edit-find-symbolic")
            );
            assert_eq!(search.button.tooltip_text().as_deref(), Some("Search"));
            assert!(search.entry.hexpands());
            search.button.set_active(true);
            assert!(search.bar.is_search_mode());
        });
    }
}
