//! Kit chrome for the Gallery shell. Photos bind FFI windows; leftover
//! GTK is never enabled.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::TryRecvError;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Instant;

use adw::prelude::*;
use gallery_ffi::{ConflictKind, MergeKind};
use gtk::gio;
use gtk::glib;
use pango::prelude::FontMapExt;
use shell_kit_gtk::{
    about_dialog, action_row, adaptive_shell, apply_progress_row, chip_bar, chrome_progress,
    confirm_dialog, empty_state, field_row, highlight_markup, init_style, list_screen, nav_row,
    navigation_view, overflow, overflow_button, page, preferences_dialog, primary_menu,
    progress_row, push_page, push_settings_subpage, search_hit_row, selection_bar, settings_screen,
    sheet, status_row, text_row, toggle_row, ActionRole, ActionRowData, AdaptiveShell, Chip,
    ChipMode, ChromeProgress, ConfirmData, EmptyCopy, EmptyKind, FieldRowData, GalleryScreen,
    ListScreen, ListScreenBuilt, ListSection, LogLevel, MenuCommand, NavRowData, PageChrome,
    PrimaryMenu, ProgressRowData, RootPage, SelectionBar, SettingsGroup, SettingsScreen, SheetSize,
    StatusRowData, StatusSeverity, TextRowData, ToggleRowData,
};

#[path = "select.rs"]
mod select;

use crate::folders::FolderEntry;
use crate::paging::{
    same_item_ids, view_item_from_object, years_from_structure, PageCache, ViewItem, ViewList,
    ViewListModel, YearMark,
};
use crate::routing::{
    folder_stack_pop_policy, nav_key_policy, route_id, FolderStackPop, NavKey, NavKeyAction,
};
use crate::session::{EventFolder, PhotoIntent, PreparedHub, PreparedUi, Session, TextPage};
use crate::thumbs::ThumbCache;
use crate::year_rail;
use crate::{
    events_hub_preview_limit, people_hub_preview_limit, APP_TITLE, COMPACT_WIDTH, EVENT_GAP_PX,
    EVENT_TILE_PX, PERSON_GAP_PX, PERSON_TILE_PX, VIEWER_SHORT_HEIGHT,
};
use select::{
    select_scope_actions, selection_actions, set_tile_select_badge, tile_photo_id, MoveUi,
    MutateOutcome,
};

const PHOTO_FILL_CHUNK: usize = 48;
/// First Photos / Folders page, and each scroll-ahead splice. Matches the
/// FFI window. Do not idle-append the whole library — GridView then binds
/// hundreds of off-screen tiles on a warm start.
const PHOTO_FILL_AHEAD: usize = 256;

const TOKEN_CSS: &str = include_str!("../../../design/tokens/generated/gallery.css");
const SHELL_CSS: &str = r#"
.gallery-tile {
  min-width: 108px;
  min-height: 108px;
  border-radius: var(--thumb-radius);
}
gridview.gallery-photos,
gridview.gallery-people,
gridview.gallery-events {
  padding: 0;
  background-color: transparent;
}
gridview.gallery-photos > child {
  margin: 1px;
  padding: 0;
}
gridview.gallery-people > child,
gridview.gallery-people > child:hover,
gridview.gallery-people > child:selected,
gridview.gallery-events > child,
gridview.gallery-events > child:hover,
gridview.gallery-events > child:selected,
gridview.gallery-folders > child,
gridview.gallery-folders > child:hover,
gridview.gallery-folders > child:selected {
  margin: 5px;
  padding: 0;
  background-color: transparent;
  box-shadow: none;
}
gridview.gallery-folders {
  padding: 4px 10px 16px 10px;
  background-color: transparent;
}
.folder-crumb-bar {
  padding: 6px 10px 2px 10px;
}
listview.folder-tree {
  background-color: transparent;
  padding: 6px;
}
listview.folder-tree > row {
  border-radius: 6px;
  padding: 2px 4px;
}
.gallery-hub-clip {
  min-width: 0;
}
.gallery-hub-clip scrollbar {
  opacity: 0;
  min-width: 0;
  min-height: 0;
}
listview.gallery-memories {
  background-color: transparent;
  padding: 0;
}
listview.gallery-memories > row {
  background-color: transparent;
  padding: 0;
  margin: 0 12px 0 0;
}
listview.gallery-memories > row:hover,
listview.gallery-memories > row:selected {
  background-color: transparent;
}
.memory-card {
  background-color: transparent;
  border-radius: var(--memory-radius);
}
.person-card,
.event-card {
  background-color: transparent;
  border-radius: var(--card-radius);
}
.cover-scrim {
  background-image: linear-gradient(
    to top,
    alpha(black, 0.55),
    transparent 58%
  );
}
button.person-card,
button.event-card {
  padding: 0;
  min-width: 0;
  min-height: 0;
  background: transparent;
}
.memory-tile {
  border-radius: var(--memory-radius);
}
.cover-tile {
  border-radius: var(--card-radius);
}
picture.memory-tile,
picture.cover-tile {
  overflow: hidden;
}
.cover-title,
.memory-title {
  color: white;
  text-shadow: 0 1px 3px alpha(black, 0.35);
}
.cover-subtitle {
  color: alpha(white, 0.85);
  text-shadow: 0 1px 3px alpha(black, 0.35);
}
overlay.memory-card .cover-captions {
  margin: 0 14px 14px 14px;
}
overlay.person-card .cover-captions,
overlay.event-card .cover-captions {
  margin: 0 9px 7px 9px;
}
.memory-title {
  font-family: "Newsreader";
  font-style: italic;
}
.person-badges {
  margin: 8px;
}
.person-badge {
  min-width: 20px;
  min-height: 20px;
  padding: 0;
  border-radius: 10px;
  background-color: alpha(black, 0.45);
  color: white;
}
.person-badge image {
  margin: 0;
}
overlay.person-card.person-hidden {
  opacity: 0.55;
}
.person-menu-item {
  min-width: 12em;
  padding: 6px 12px;
}
.person-menu-item label {
  min-width: 10em;
}
button.gallery-play {
  min-width: 64px;
  min-height: 64px;
}
.video-badge {
  margin: 5px;
  padding: 3px 4px;
  color: white;
  background-color: alpha(black, 0.5);
  border-radius: 4px;
}
.select-badge {
  margin: 5px;
  padding: 3px 4px;
  color: white;
  background-color: alpha(black, 0.5);
  border-radius: 4px;
}
"#;
const PHOTO_TILE_PX: i32 = 108;
const MEMORY_CARD_W: i32 = 264;
const MEMORY_CARD_H: i32 = 328;
const DIAGNOSTIC_CAPACITY: usize = 5_000;

/// Navigation stack that currently hosts the photo viewer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ViewerHost {
    #[default]
    Photos,
    Folders,
    Collections,
}

struct BenchState {
    started: Instant,
    scan_started: Cell<Option<Instant>>,
    done: Cell<bool>,
    pending: Cell<Option<(f64, f64, f64, f64, f64)>>,
}

struct PreparedPhotos {
    list: ViewList,
    years: Vec<YearMark>,
    ids: Arc<Vec<String>>,
}

#[derive(Clone)]
struct ExplorerFill {
    folder_id: String,
    folder_n: usize,
    total: usize,
}

struct PendingDrill {
    token: u64,
    grid: gtk::GridView,
    cache: Rc<RefCell<PageCache<gallery_ffi::GalleryMediaItem>>>,
    model: Rc<RefCell<Option<ViewListModel>>>,
}

#[derive(Clone)]
pub struct Window {
    inner: Rc<Inner>,
}

struct Inner {
    window: adw::ApplicationWindow,
    shell: AdaptiveShell,
    #[allow(dead_code)]
    menu: PrimaryMenu,
    root_stack: gtk::Stack,
    folders_nav: adw::NavigationView,
    collections_nav: adw::NavigationView,
    photos_nav: adw::NavigationView,
    #[allow(dead_code)]
    folders_split: adw::OverlaySplitView,
    folders_sidebar: gtk::ListView,
    folders_host: gtk::Stack,
    folders_scroll: gtk::ScrolledWindow,
    folders_grid: gtk::GridView,
    folders_crumbs: gtk::Box,
    folders_back: gtk::Button,
    photos_host: gtk::Stack,
    photos_scroll: gtk::ScrolledWindow,
    photos_grid: gtk::GridView,
    photos_year_rail: gtk::Box,
    photos_search: gtk::SearchEntry,
    logs_search: RefCell<Option<gtk::SearchEntry>>,
    photos_hits: gtk::ListBox,
    photos_chips: gtk::Box,
    collections_box: gtk::Box,
    collections_scroll: gtk::ScrolledWindow,
    hub_reflow_queued: Cell<bool>,
    settings_dialog: RefCell<Option<adw::PreferencesDialog>>,
    toast: adw::ToastOverlay,
    session: RefCell<Session>,
    watch: RefCell<Option<localgallery::watch::WatchHandle>>,
    watch_rx: RefCell<Option<std::sync::mpsc::Receiver<()>>>,
    thumbs: ThumbCache,
    media_pages: RefCell<PageCache<gallery_ffi::GalleryMediaItem>>,
    folders_model: RefCell<Option<ViewListModel>>,
    photos_model: RefCell<Option<ViewListModel>>,
    photos_bound_ids: RefCell<Option<Arc<Vec<String>>>>,
    photos_fill_items: RefCell<Option<Rc<Vec<ViewItem>>>>,
    photos_reload_queued: Cell<bool>,
    photos_fill_queued: Cell<bool>,
    explorer_fill_queued: Cell<bool>,
    explorer_fill: RefCell<Option<ExplorerFill>>,
    folder_stack: RefCell<Vec<Option<String>>>,
    last_viewer_host: Cell<ViewerHost>,
    scan_rx: RefCell<Option<mpsc::Receiver<ScanWire>>>,
    photos_rx: RefCell<Option<mpsc::Receiver<PreparedPhotos>>>,
    drill_rx: RefCell<Option<mpsc::Receiver<(u64, ViewList)>>>,
    memories_rx: RefCell<Option<mpsc::Receiver<Vec<gallery_ffi::MemoryStructure>>>>,
    leftover_rx: RefCell<Option<mpsc::Receiver<Result<(PathBuf, f64), String>>>>,
    leftover_cancel: RefCell<Option<Arc<AtomicBool>>>,
    analysis_rx: RefCell<Option<mpsc::Receiver<AnalysisEvent>>>,
    pack_rx: RefCell<Option<mpsc::Receiver<PackEvent>>>,
    scan_busy: Cell<bool>,
    analysis_busy: Cell<bool>,
    pack_busy: Cell<bool>,
    analysis_everything: Cell<bool>,
    analysis_tagging: Cell<bool>,
    analysis_faces: Cell<bool>,
    analysis_places: Cell<bool>,
    photo_fill_gen: Cell<u64>,
    photo_fill_active: Cell<bool>,
    folders_fill_gen: Cell<u64>,
    drill_token: Cell<u64>,
    pending_drill: RefCell<Option<PendingDrill>>,
    hub: RefCell<Option<PreparedHub>>,
    hub_people_limit: Cell<usize>,
    hub_events_limit: Cell<usize>,
    folder_page: RefCell<Option<TextPage>>,
    scan_progress: RefCell<Option<adw::ActionRow>>,
    chrome_progress: ChromeProgress,
    bench: RefCell<Option<BenchState>>,
    selection_bar: SelectionBar,
    select_scope_bar: SelectionBar,
    watch_mute: RefCell<Option<Arc<localgallery::watch::MuteGate>>>,
    watch_ignore_ticks: Cell<u32>,
    mutate_rx: RefCell<Option<mpsc::Receiver<MutateOutcome>>>,
    move_ui: RefCell<Option<MoveUi>>,
}

struct ScanWire {
    folder: PathBuf,
    toasts: Vec<String>,
    result: Result<PreparedUi, String>,
}

enum AnalysisEvent {
    Done {
        summary: localgallery::AnalysisSummary,
        ui: Option<PreparedUi>,
    },
}

enum PackEvent {
    Downloaded(Result<localgallery::PackStatus, String>),
    Removed(Result<(), String>),
}

impl Window {
    pub fn present(app: &adw::Application, launch: &crate::LaunchArgs) {
        let this = Self::build(app, launch.comet);
        load_display_font();
        init_style(TOKEN_CSS);
        apply_shell_css();
        if let Some((width, height)) = launch.size {
            this.inner.window.set_default_size(width, height);
        }
        this.inner.window.present();
        if launch.bench {
            if launch.folder.is_none() {
                eprintln!("--bench needs --folder");
                std::process::exit(2);
            }
            this.arm_bench();
        }
        this.apply_launch(launch);
        if launch.bench {
            return;
        }
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
        let skip_folder = route == Some(route_id(GalleryScreen::FolderPicker));
        if let Some(folder) = launch.folder.as_ref().filter(|_| !skip_folder) {
            self.open_folder(folder);
        } else if !skip_folder {
            self.load_persisted();
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
                    .set_visible_child_name(route_id(GalleryScreen::FolderPicker));
            }
            "folders" => self.show_tab("folders"),
            "collections" => self.show_tab("collections"),
            "photos" => self.show_tab("photos"),
            "folder" => {
                self.show_tab("folders");
                if let Some(id) = self.first_folder_id() {
                    self.show_folder(Some(&id));
                }
            }
            "people" => {
                self.show_tab("collections");
                self.push_people();
            }
            "person" => {
                self.show_tab("collections");
                self.push_people();
                if let Ok(page) = self.inner.session.borrow().people_page() {
                    if let Some(row) = page.rows.into_iter().next() {
                        self.push_person(&row.id, &row.title);
                    }
                }
            }
            "events" => {
                self.show_tab("collections");
                self.push_events();
            }
            "album" => {
                self.show_tab("collections");
                if let Ok(page) = self.inner.session.borrow().collection_section("albums") {
                    if let Some(row) = page.rows.into_iter().next() {
                        self.push_album(&row.id, &row.title);
                    }
                }
            }
            "memory" => {
                self.show_tab("collections");
                let memory = self
                    .inner
                    .session
                    .borrow_mut()
                    .ensure_memories()
                    .first()
                    .cloned();
                if let Some(memory) = memory {
                    self.push_memory(&memory);
                }
            }
            "viewer" => {
                self.show_tab("photos");
                if let Some(id) = self.first_photo_id() {
                    self.push_viewer(&id, ViewerHost::Photos);
                }
            }
            "photo-info" => {
                self.show_tab("photos");
                if let Some(id) = self.first_photo_id() {
                    self.push_viewer(&id, ViewerHost::Photos);
                    self.present_photo_info(&id, ViewerHost::Photos);
                }
            }
            "settings" => self.present_settings(),
            "logs" => {
                self.present_settings();
                self.present_logs();
            }
            "sync-conflict-group" => self.present_conflicts(),
            _ => {}
        }
    }

    fn show_tab(&self, id: &str) {
        if self.inner.session.borrow().folder().is_some() {
            self.inner.root_stack.set_visible_child_name("library");
            self.inner.shell.stack.set_visible_child_name(id);
            if id == "photos" && self.inner.photos_model.borrow().is_none() {
                self.request_photos();
            }
            if id == "folders" {
                self.ensure_explorer_photos();
            }
        }
    }

    fn build(app: &adw::Application, comet: bool) -> Self {
        let window = adw::ApplicationWindow::new(app);
        window.set_title(Some(APP_TITLE));
        window.set_width_request(360);
        window.set_height_request(294);
        window.set_default_size(
            if comet { 540 } else { 1200 },
            if comet { 620 } else { 800 },
        );

        let picker = empty_state(
            EmptyKind::NoFolder,
            &EmptyCopy {
                title: "Choose a Folder".into(),
                description: Some("Select a folder of photos. The library is that folder.".into()),
                action: Some("Choose Folder…".into()),
            },
        );
        picker
            .page
            .set_widget_name(route_id(GalleryScreen::FolderPicker));
        let choose = picker.action.clone().expect("folder-picker primary action");

        let folders_host = gtk::Stack::new();
        let folders_empty = empty_state(
            EmptyKind::EmptyFolder,
            &EmptyCopy {
                title: "This folder is empty".into(),
                description: Some("No subfolders or photos here.".into()),
                action: None,
            },
        );
        let folders_grid =
            gtk::GridView::new(None::<gtk::NoSelection>, None::<gtk::SignalListItemFactory>);
        folders_grid.add_css_class("gallery-folders");
        folders_grid.add_css_class("gallery-photos");
        folders_grid.set_single_click_activate(true);
        pack_photo_grid(&folders_grid);
        let folders_scroll = gtk::ScrolledWindow::new();
        folders_scroll.set_hexpand(true);
        folders_scroll.set_vexpand(true);
        folders_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        folders_scroll.set_child(Some(&folders_grid));
        folders_host.add_named(&folders_scroll, Some("list"));
        folders_host.add_named(&folders_empty.page, Some("empty"));
        folders_host.set_widget_name(route_id(GalleryScreen::Folders));

        let folders_back = gtk::Button::from_icon_name("go-previous-symbolic");
        folders_back.add_css_class("flat");
        folders_back.set_tooltip_text(Some("Back"));
        folders_back.set_valign(gtk::Align::Center);
        folders_back.set_visible(false);
        let folders_tree_toggle = gtk::ToggleButton::new();
        folders_tree_toggle.set_icon_name("sidebar-show-right-symbolic");
        folders_tree_toggle.add_css_class("flat");
        folders_tree_toggle.set_tooltip_text(Some("Folder Tree"));
        folders_tree_toggle.set_valign(gtk::Align::Center);
        let folders_crumbs = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        folders_crumbs.set_hexpand(true);
        folders_crumbs.add_css_class("folder-crumbs");
        let crumb_bar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        crumb_bar.add_css_class("folder-crumb-bar");
        crumb_bar.append(&folders_back);
        crumb_bar.append(&folders_tree_toggle);
        crumb_bar.append(&folders_crumbs);

        let folders_sidebar = gtk::ListView::new(
            None::<gtk::SingleSelection>,
            None::<gtk::SignalListItemFactory>,
        );
        folders_sidebar.add_css_class("navigation-sidebar");
        folders_sidebar.add_css_class("folder-tree");
        folders_sidebar.set_single_click_activate(true);
        let folders_sidebar_scroll = gtk::ScrolledWindow::new();
        folders_sidebar_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        folders_sidebar_scroll.set_child(Some(&folders_sidebar));
        folders_sidebar_scroll.set_size_request(220, -1);
        folders_sidebar_scroll.add_css_class("sidebar");

        let folders_body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        folders_body.set_hexpand(true);
        folders_body.set_vexpand(true);
        folders_body.append(&crumb_bar);
        folders_body.append(&folders_host);

        let folders_split = adw::OverlaySplitView::new();
        folders_split.set_sidebar(Some(&folders_sidebar_scroll));
        folders_split.set_content(Some(&folders_body));
        folders_split.set_min_sidebar_width(200.0);
        folders_split.set_max_sidebar_width(300.0);
        folders_split.set_sidebar_width_fraction(0.22);
        folders_split.set_enable_hide_gesture(true);
        folders_split.set_enable_show_gesture(true);
        folders_split
            .bind_property("show-sidebar", &folders_tree_toggle, "active")
            .bidirectional()
            .sync_create()
            .build();
        folders_split
            .bind_property("collapsed", &folders_tree_toggle, "visible")
            .sync_create()
            .build();

        let folders_root = page("Folders", &folders_split, PageChrome::root());
        folders_root.set_tag(Some("root"));
        folders_root.set_widget_name(route_id(GalleryScreen::Folders));
        let folders_nav = navigation_view();
        folders_nav.set_hexpand(true);
        folders_nav.set_vexpand(true);
        folders_nav.add(&folders_root);

        let collections_box = gtk::Box::new(gtk::Orientation::Vertical, 24);
        collections_box.set_hexpand(true);
        collections_box.set_margin_top(8);
        collections_box.set_margin_bottom(24);
        collections_box.set_margin_start(16);
        collections_box.set_margin_end(16);
        let collections_scroll = gtk::ScrolledWindow::new();
        collections_scroll.set_hexpand(true);
        collections_scroll.set_vexpand(true);
        collections_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        collections_scroll.set_child(Some(&collections_box));
        let collections_root = page("Collections", &collections_scroll, PageChrome::root());
        collections_root.set_tag(Some("root"));
        collections_root.set_widget_name(route_id(GalleryScreen::Collections));
        let collections_nav = navigation_view();
        collections_nav.set_hexpand(true);
        collections_nav.set_vexpand(true);
        collections_nav.add(&collections_root);

        let photos_search = gtk::SearchEntry::new();
        photos_search.set_placeholder_text(Some("Search photos"));
        photos_search.set_hexpand(true);
        let photos_hits = gtk::ListBox::new();
        photos_hits.add_css_class("boxed-list");
        photos_hits.set_selection_mode(gtk::SelectionMode::None);
        photos_hits.set_visible(false);
        let photos_chips = gtk::Box::new(gtk::Orientation::Vertical, 8);
        photos_chips.set_visible(false);
        let photos_bar = gtk::Box::new(gtk::Orientation::Vertical, 8);
        photos_bar.set_margin_top(8);
        photos_bar.set_margin_bottom(12);
        photos_bar.set_margin_start(8);
        photos_bar.set_margin_end(8);
        photos_bar.append(&photos_search);
        photos_bar.append(&photos_hits);
        photos_bar.append(&photos_chips);
        let photos_grid =
            gtk::GridView::new(None::<gtk::NoSelection>, None::<gtk::SignalListItemFactory>);
        photos_grid.add_css_class("gallery-photos");
        photos_grid.set_min_columns(2);
        photos_grid.set_max_columns(24);
        photos_grid.set_single_click_activate(true);
        let photos_scroll = gtk::ScrolledWindow::new();
        photos_scroll.set_child(Some(&photos_grid));
        photos_scroll.set_hexpand(true);
        photos_scroll.set_vexpand(true);
        photos_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        let (photos_overlay, photos_year_rail) = year_rail::overlay_on_scroll(&photos_scroll);
        let photos_empty = empty_state(
            EmptyKind::EmptyFolder,
            &EmptyCopy {
                title: "No photos".into(),
                description: Some("This folder has no photos yet.".into()),
                action: None,
            },
        );
        let photos_host = gtk::Stack::new();
        photos_host.set_hexpand(true);
        photos_host.set_vexpand(true);
        photos_host.add_named(&photos_overlay, Some("grid"));
        photos_host.add_named(&photos_empty.page, Some("empty"));
        let photos_col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        photos_col.set_hexpand(true);
        photos_col.set_vexpand(true);
        photos_col.append(&photos_bar);
        photos_col.append(&photos_host);
        photos_col.set_widget_name(route_id(GalleryScreen::Photos));
        let photos_root = page("Photos", &photos_col, PageChrome::root());
        photos_root.set_tag(Some("root"));
        let photos_nav = navigation_view();
        photos_nav.set_hexpand(true);
        photos_nav.set_vexpand(true);
        photos_nav.add(&photos_root);

        let shell = adaptive_shell(
            APP_TITLE,
            &[
                RootPage {
                    id: "folders",
                    title: "Folders",
                    icon: "folder-symbolic",
                    child: folders_nav.clone().upcast(),
                },
                RootPage {
                    id: "collections",
                    title: "Collections",
                    icon: "view-grid-symbolic",
                    child: collections_nav.clone().upcast(),
                },
                RootPage {
                    id: "photos",
                    title: "Photos",
                    icon: "image-x-generic-symbolic",
                    child: photos_nav.clone().upcast(),
                },
            ],
        );
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
                MenuCommand {
                    id: "scan-photos",
                    label: "Scan Photos".into(),
                },
                MenuCommand {
                    id: "select",
                    label: "Select".into(),
                },
            ],
        );
        let chrome_progress = chrome_progress();
        shell.header.pack_start(&chrome_progress.root);
        shell.header.pack_end(&menu.button);
        shell.install(&window);
        let folder_collapse = adw::Breakpoint::new(
            adw::BreakpointCondition::parse("max-width: 860sp").expect("folder split collapse"),
        );
        folder_collapse.add_setter(&folders_split, "collapsed", Some(&true.to_value()));
        folder_collapse.add_setter(&folders_split, "show-sidebar", Some(&false.to_value()));
        window.add_breakpoint(folder_collapse);
        let selection = selection_bar(&selection_actions());
        let select_scope = selection_bar(&select_scope_actions());
        shell.toolbar.add_top_bar(&select_scope.revealer);
        shell.toolbar.add_bottom_bar(&selection.revealer);

        let root_stack = gtk::Stack::new();
        root_stack.add_named(&picker.page, Some(route_id(GalleryScreen::FolderPicker)));
        root_stack.add_named(&shell.toolbar, Some("library"));
        root_stack.set_visible_child_name(route_id(GalleryScreen::FolderPicker));

        let toast = adw::ToastOverlay::new();
        toast.set_child(Some(&root_stack));
        window.set_content(Some(&toast));

        let inner = Rc::new(Inner {
            window: window.clone(),
            shell,
            menu: menu.clone(),
            root_stack,
            folders_nav: folders_nav.clone(),
            collections_nav: collections_nav.clone(),
            photos_nav: photos_nav.clone(),
            folders_split: folders_split.clone(),
            folders_sidebar: folders_sidebar.clone(),
            folders_host,
            folders_scroll: folders_scroll.clone(),
            folders_grid: folders_grid.clone(),
            folders_crumbs,
            folders_back: folders_back.clone(),
            photos_host,
            photos_scroll: photos_scroll.clone(),
            photos_grid: photos_grid.clone(),
            photos_year_rail,
            photos_search: photos_search.clone(),
            logs_search: RefCell::new(None),
            photos_hits: photos_hits.clone(),
            photos_chips,
            collections_box,
            collections_scroll: collections_scroll.clone(),
            hub_reflow_queued: Cell::new(false),
            settings_dialog: RefCell::new(None),
            toast,
            session: RefCell::new(Session::with_persist(DIAGNOSTIC_CAPACITY, true)),
            watch: RefCell::new(None),
            watch_rx: RefCell::new(None),
            thumbs: ThumbCache::new(),
            media_pages: RefCell::new(PageCache::default()),
            folders_model: RefCell::new(None),
            photos_model: RefCell::new(None),
            photos_bound_ids: RefCell::new(None),
            photos_fill_items: RefCell::new(None),
            photos_reload_queued: Cell::new(false),
            photos_fill_queued: Cell::new(false),
            explorer_fill_queued: Cell::new(false),
            explorer_fill: RefCell::new(None),
            folder_stack: RefCell::new(vec![None]),
            last_viewer_host: Cell::new(ViewerHost::Photos),
            scan_rx: RefCell::new(None),
            photos_rx: RefCell::new(None),
            drill_rx: RefCell::new(None),
            memories_rx: RefCell::new(None),
            leftover_rx: RefCell::new(None),
            leftover_cancel: RefCell::new(None),
            analysis_rx: RefCell::new(None),
            pack_rx: RefCell::new(None),
            scan_busy: Cell::new(false),
            analysis_busy: Cell::new(false),
            pack_busy: Cell::new(false),
            analysis_everything: Cell::new(true),
            analysis_tagging: Cell::new(true),
            analysis_faces: Cell::new(true),
            analysis_places: Cell::new(true),
            photo_fill_gen: Cell::new(0),
            photo_fill_active: Cell::new(false),
            folders_fill_gen: Cell::new(0),
            drill_token: Cell::new(0),
            pending_drill: RefCell::new(None),
            hub: RefCell::new(None),
            hub_people_limit: Cell::new(0),
            hub_events_limit: Cell::new(0),
            folder_page: RefCell::new(None),
            scan_progress: RefCell::new(None),
            chrome_progress,
            bench: RefCell::new(None),
            selection_bar: selection,
            select_scope_bar: select_scope,
            watch_mute: RefCell::new(None),
            watch_ignore_ticks: Cell::new(0),
            mutate_rx: RefCell::new(None),
            move_ui: RefCell::new(None),
        });
        let this = Window { inner };

        this.install_folder_factory();
        this.install_folder_sidebar_factory();
        this.install_photo_factory();
        this.attach_photo_grid_menu(&photos_grid, ViewerHost::Photos);
        this.attach_photo_grid_menu(&folders_grid, ViewerHost::Folders);
        bind_cover_grid_columns(&folders_scroll, &folders_grid, EVENT_TILE_PX, EVENT_GAP_PX);
        let picker_open = this.clone();
        choose.connect_clicked(move |_| picker_open.pick_folder());

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
        if let Some(choose_folder) = menu.extra("choose-folder") {
            let chooser = this.clone();
            choose_folder.connect_activate(move |_, _| chooser.pick_folder());
        }
        if let Some(scan) = menu.extra("scan-photos") {
            let scanner = this.clone();
            scan.connect_activate(move |_, _| scanner.scan_photos());
        }
        if let Some(select) = menu.extra("select") {
            let selector = this.clone();
            select.connect_activate(move |_, _| selector.set_selecting(true));
        }
        if let Some(cancel) = this.inner.select_scope_bar.buttons.first() {
            let selector = this.clone();
            cancel.connect_clicked(move |_| selector.cancel_select_mode());
        }
        if let Some(select_all) = this.inner.select_scope_bar.buttons.get(1) {
            let selector = this.clone();
            select_all.connect_clicked(move |_| selector.select_visible_photos());
        }
        if let Some(deselect) = this.inner.select_scope_bar.buttons.get(2) {
            let selector = this.clone();
            deselect.connect_clicked(move |_| selector.deselect_all_photos());
        }
        if let Some(share) = this.inner.selection_bar.buttons.first() {
            let sharer = this.clone();
            share.connect_clicked(move |_| sharer.share_selected());
        }
        if let Some(mover) = this.inner.selection_bar.buttons.get(1) {
            let moving = this.clone();
            mover.connect_clicked(move |_| moving.move_selected());
        }
        if let Some(deleter) = this.inner.selection_bar.buttons.get(2) {
            let deleting = this.clone();
            deleter.connect_clicked(move |_| deleting.delete_selected());
        }
        let keys = gtk::EventControllerKey::new();
        let keyed = this.clone();
        keys.connect_key_pressed(move |_, key, _, mods| {
            let alt = mods.contains(gtk::gdk::ModifierType::ALT_MASK);
            let nav_key = if key == gtk::gdk::Key::Escape {
                Some(NavKey::Escape)
            } else if alt && key == gtk::gdk::Key::Left {
                Some(NavKey::AltLeft)
            } else {
                None
            };
            let Some(nav_key) = nav_key else {
                return glib::Propagation::Proceed;
            };
            match nav_key_policy(
                keyed.inner.session.borrow().is_selecting(),
                keyed.search_entry_focused(),
                nav_key,
            ) {
                NavKeyAction::CancelSelect => {
                    keyed.cancel_select_mode();
                    glib::Propagation::Stop
                }
                NavKeyAction::Pop => {
                    if keyed.pop_visible_nav() {
                        glib::Propagation::Stop
                    } else {
                        glib::Propagation::Proceed
                    }
                }
                NavKeyAction::Ignore => glib::Propagation::Proceed,
            }
        });
        this.inner.window.add_controller(keys);
        let tabbed = this.clone();
        let last_tab = RefCell::new(
            this.inner
                .shell
                .stack
                .visible_child_name()
                .map(|name| name.to_string()),
        );
        this.inner
            .shell
            .stack
            .connect_notify_local(Some("visible-child"), move |stack, _| {
                let now = stack.visible_child_name().map(|name| name.to_string());
                if *last_tab.borrow() == now {
                    return;
                }
                *last_tab.borrow_mut() = now.clone();
                if tabbed.inner.session.borrow().is_selecting() {
                    tabbed.set_selecting(false);
                }
                if now.as_deref() == Some("folders") {
                    tabbed.ensure_explorer_photos();
                }
            });
        let cancel = this.clone();
        this.inner
            .chrome_progress
            .cancel_button()
            .connect_clicked(move |_| cancel.cancel_work());

        let searched = this.clone();
        photos_search.connect_search_changed(move |entry| {
            searched
                .inner
                .session
                .borrow_mut()
                .set_query(entry.text().to_string());
            searched.refill_search_hits();
            searched.request_photos();
        });
        let hit_act = this.clone();
        photos_hits.connect_row_activated(move |_, row| {
            let id = row.widget_name();
            hit_act.activate_search_hit(&id);
        });

        let folder_act = this.clone();
        folders_grid.connect_activate(move |_, pos| {
            let item = folder_act
                .inner
                .folders_model
                .borrow()
                .as_ref()
                .and_then(|model| model.item(pos));
            let Some(item) = item else {
                return;
            };
            if item.section == "folders" {
                folder_act.show_folder(Some(&item.id));
            } else {
                folder_act.activate_explorer_photo(&item.id);
            }
        });
        let tree_act = this.clone();
        folders_sidebar.connect_activate(move |list, pos| {
            if let Some(id) = sidebar_folder_id(list, pos) {
                tree_act.show_folder(Some(&id));
            }
        });
        let back_act = this.clone();
        folders_back.connect_clicked(move |_| back_act.folder_back());
        let photo_act = this.clone();
        photos_grid.connect_activate(move |_, pos| {
            let item = photo_act
                .inner
                .photos_model
                .borrow()
                .as_ref()
                .and_then(|model| model.item(pos));
            let Some(item) = item else {
                return;
            };
            photo_act.activate_photo(&item.id, ViewerHost::Photos);
        });

        let popped = this.clone();
        folders_nav.connect_popped(move |_, page| {
            // Clone the stack so the RefCell borrow does not live across
            // the match (temporary lifetime would panic on borrow_mut).
            let stack = popped.inner.folder_stack.borrow().clone();
            match folder_stack_pop_policy(&page.widget_name(), &stack) {
                FolderStackPop::Leave => {}
                FolderStackPop::PopFolder { parent } => {
                    popped.show_folder(parent.as_deref());
                }
            }
        });

        let sized = this.clone();
        photos_grid.connect_realize(move |grid| {
            sized.update_photo_columns(grid.width());
        });
        let photos_adj = this.inner.photos_scroll.vadjustment();
        let fill_photos = this.clone();
        photos_adj.connect_value_changed(move |_| {
            fill_photos.maybe_fill_photos();
        });
        let fill_photos_changed = this.clone();
        photos_adj.connect_changed(move |_| {
            fill_photos_changed.maybe_fill_photos();
        });
        let folders_adj = this.inner.folders_scroll.vadjustment();
        let fill_folders = this.clone();
        folders_adj.connect_value_changed(move |_| {
            fill_folders.maybe_fill_explorer();
        });
        let fill_folders_changed = this.clone();
        folders_adj.connect_changed(move |_| {
            fill_folders_changed.maybe_fill_explorer();
        });
        let reflow_window = this.clone();
        this.inner
            .window
            .connect_notify_local(Some("default-width"), move |_, _| {
                reflow_window.schedule_collections_reflow();
            });
        let reflow_width = this.clone();
        this.inner
            .window
            .connect_notify_local(Some("width"), move |_, _| {
                reflow_width.schedule_collections_reflow();
            });
        let reflow_scroll = this.clone();
        this.inner
            .collections_scroll
            .connect_notify_local(Some("width"), move |_, _| {
                reflow_scroll.schedule_collections_reflow();
            });

        this.inner
            .session
            .borrow_mut()
            .record(LogLevel::Info, "app", "Application started");
        this.poll_idle();
        this
    }

    fn install_folder_factory(&self) {
        let factory = gtk::SignalListItemFactory::new();
        let thumbs = self.inner.thumbs.clone();
        let this = self.clone();
        factory.connect_setup(move |_, obj| {
            let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
            item.set_child(Some(&explorer_tile()));
        });
        factory.connect_bind({
            let thumbs = thumbs.clone();
            let this = this.clone();
            move |_, obj| {
                let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
                let Some(root) = item.child() else { return };
                let Some(object) = item.item() else { return };
                let Some(view) = view_item_from_object(&object) else {
                    return;
                };
                if this.inner.scan_busy.get() {
                    return;
                }
                this.unbind_explorer_tile(&root);
                if view.section == "folders" {
                    root.set_widget_name("folder-tile");
                    show_named_stack_child(&root, "folder");
                    let entry = this.inner.session.borrow().folder_entry(&view.id);
                    if let Some(entry) = entry {
                        this.bind_folder_tile(&root, &entry);
                    }
                } else {
                    show_named_stack_child(&root, "photo");
                    this.bind_tile_selection(item, &view.id);
                    if let Some(picture) = find_named_widget::<gtk::Picture>(&root, "photo-pic") {
                        let host = this
                            .inner
                            .session
                            .borrow()
                            .host_for_photo(&view.id)
                            .cloned();
                        if let Some(host) = host {
                            let scale = this.inner.window.scale_factor().max(1) as u32;
                            thumbs.bind_grid(&picture, &host.path, &view.id, scale);
                            set_tile_video_badge(item, host.is_video);
                        }
                    }
                }
            }
        });
        factory.connect_unbind({
            let this = this.clone();
            move |_, obj| {
                let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
                if let Some(child) = item.child() {
                    this.unbind_explorer_tile(&child);
                }
                set_tile_video_badge(item, false);
                set_tile_select_badge(item, false);
            }
        });
        self.inner.folders_grid.set_factory(Some(&factory));
    }

    fn install_folder_sidebar_factory(&self) {
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(move |_, obj| {
            let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
            item.set_child(Some(&sidebar_folder_row()));
        });
        factory.connect_bind(move |_, obj| {
            let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
            let Some(row) = item.item().and_downcast::<gtk::TreeListRow>() else {
                return;
            };
            let Some(expander) = item.child().and_downcast::<gtk::TreeExpander>() else {
                return;
            };
            expander.set_list_row(Some(&row));
            let Some(boxed) = row.item().and_downcast::<glib::BoxedAnyObject>() else {
                return;
            };
            let entry = boxed.borrow::<FolderEntry>().clone();
            if let Some(label) = find_named_widget::<gtk::Label>(&expander, "folder-name") {
                label.set_text(&entry.name);
            }
            if let Some(count) = find_named_widget::<gtk::Label>(&expander, "folder-count") {
                count.set_text(&photo_count_caption(entry.total_photo_count.max(0) as usize));
            }
        });
        self.inner.folders_sidebar.set_factory(Some(&factory));
    }

    fn install_photo_factory(&self) {
        let factory = gtk::SignalListItemFactory::new();
        let thumbs = self.inner.thumbs.clone();
        let this = self.clone();
        factory.connect_setup(move |_, obj| {
            let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
            item.set_child(Some(&photo_tile()));
        });
        factory.connect_bind({
            let thumbs = thumbs.clone();
            let this = this.clone();
            move |_, obj| {
                let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
                let Some(picture) = picture_from_item(item) else {
                    return;
                };
                let Some(object) = item.item() else { return };
                let Some(view) = view_item_from_object(&object) else {
                    return;
                };
                this.bind_tile_selection(item, &view.id);
                if this.inner.scan_busy.get() {
                    return;
                }
                if let Some(is_video) = this.bind_grid_thumb(&thumbs, &picture, &view.id) {
                    set_tile_video_badge(item, is_video);
                    return;
                }
                let generation = this
                    .inner
                    .photos_model
                    .borrow()
                    .as_ref()
                    .map(ViewListModel::generation)
                    .unwrap_or(0);
                let this_stale = this.clone();
                let media =
                    this.fetch_media(&view, generation, &this.inner.media_pages, move || {
                        this_stale.queue_photos_reload()
                    });
                if let Some(media) = media {
                    let scale = this.inner.window.scale_factor().max(1) as u32;
                    thumbs.bind_grid(&picture, &media.thumbnail_ref, &media.id, scale);
                    set_tile_video_badge(item, media.badge.as_deref() == Some("Video"));
                }
            }
        });
        factory.connect_unbind({
            let thumbs = thumbs.clone();
            move |_, obj| {
                let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
                if let Some(picture) = picture_from_item(item) {
                    thumbs.recycle(&picture);
                }
                set_tile_video_badge(item, false);
                set_tile_select_badge(item, false);
            }
        });
        self.inner.photos_grid.set_factory(Some(&factory));
    }

    /// Grid thumbs use the leftover host path. `photo_window` goes stale
    /// while the scan worker mutates the shared index; a same-id
    /// `bind_photos` skip then never rebound the factory.
    fn bind_grid_thumb(
        &self,
        thumbs: &ThumbCache,
        picture: &gtk::Picture,
        id: &str,
    ) -> Option<bool> {
        let host = self.inner.session.borrow().host_for_photo(id).cloned()?;
        let scale = self.inner.window.scale_factor().max(1) as u32;
        thumbs.bind_grid(picture, &host.path, id, scale);
        Some(host.is_video)
    }

    fn refresh_photo_grid_thumbs(&self) {
        let thumbs = self.inner.thumbs.clone();
        let mut child = self.inner.photos_grid.first_child();
        while let Some(widget) = child {
            if let Some(id) = tile_photo_id(&widget) {
                if let Some(picture) = find_widget::<gtk::Picture>(&widget) {
                    let _ = self.bind_grid_thumb(&thumbs, &picture, &id);
                }
            }
            child = widget.next_sibling();
        }
    }

    fn fetch_media(
        &self,
        view: &ViewItem,
        generation: u64,
        cache: &RefCell<PageCache<gallery_ffi::GalleryMediaItem>>,
        on_stale: impl FnOnce() + 'static,
    ) -> Option<gallery_ffi::GalleryMediaItem> {
        let session = self.inner.session.borrow();
        let mut cache = cache.borrow_mut();
        if cache.generation() != generation {
            cache.replace_generation(generation);
        }
        match cache.fetch(generation, &view.section, view.index, |offset, limit| {
            let t0 = std::time::Instant::now();
            let result =
                session
                    .index()
                    .photo_window(view.section.clone(), offset, limit, generation);
            localcore_trace::event(
                "photos",
                format!(
                    "photo_window section={} off={} lim={} {} {:?}",
                    view.section,
                    offset,
                    limit,
                    localcore_trace::fmt_ms(t0.elapsed()),
                    result
                        .as_ref()
                        .map(|w| w.len())
                        .map_err(|e| format!("{e:?}")),
                ),
            );
            result
        }) {
            Ok(Some(item)) => Some(item.clone()),
            Ok(None) => None,
            Err(gallery_ffi::ViewError::StaleGeneration { .. }) => {
                glib::idle_add_local_once(on_stale);
                None
            }
            Err(_) => None,
        }
    }

    fn toast(&self, message: &str) {
        self.inner.toast.add_toast(adw::Toast::new(message));
    }

    fn load_persisted(&self) {
        let config = localgallery::Config::load();
        if let Some(folder) = config.library_root {
            if localgallery::config::root_is_available(&folder) {
                self.open_folder(&folder);
            } else {
                self.inner.session.borrow_mut().record(
                    LogLevel::Warning,
                    "folder",
                    "Saved photo folder is unavailable",
                );
            }
        } else {
            self.inner.session.borrow_mut().record(
                LogLevel::Info,
                "folder",
                "Waiting for a photo folder",
            );
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
                Ok(file) => match file.path() {
                    Some(path) => this.open_folder(&path),
                    None => {
                        this.inner.session.borrow_mut().record(
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
                        this.inner.session.borrow_mut().record(
                            LogLevel::Error,
                            "folder",
                            "Folder picker failed",
                        );
                        this.toast(&msg);
                    }
                }
            },
        );
    }

    fn open_folder(&self, folder: &Path) {
        self.start_scan(folder.to_path_buf(), true);
    }

    fn reload_folder(&self) {
        let Some(folder) = self.inner.session.borrow().folder().map(Path::to_path_buf) else {
            self.toast("Choose a Folder first");
            return;
        };
        self.start_scan(folder, false);
    }

    fn scan_photos(&self) {
        self.start_analysis(self.selected_analysis_phases(), false);
    }

    fn selected_analysis_phases(&self) -> localgallery::AnalysisPhases {
        if self.inner.analysis_everything.get() {
            return localgallery::AnalysisPhases::all();
        }
        localgallery::AnalysisPhases {
            tagging: self.inner.analysis_tagging.get(),
            faces: self.inner.analysis_faces.get(),
            places: self.inner.analysis_places.get(),
        }
    }

    fn start_analysis(&self, phases: localgallery::AnalysisPhases, force: bool) {
        if self.inner.scan_busy.get()
            || self.inner.analysis_busy.get()
            || self.inner.pack_busy.get()
        {
            self.toast("A scan is already running");
            return;
        }
        if self.inner.session.borrow().folder().is_none() {
            self.toast("Choose a Folder first");
            return;
        }
        if self.inner.session.borrow().photo_count() == 0 {
            self.toast("This folder has no photos yet");
            return;
        }
        if phases.is_empty() {
            self.toast("Choose something to scan");
            return;
        }
        self.inner.session.borrow().prepare_analysis();
        let photos = self.inner.session.borrow().analysis_photos();
        let index = self.inner.session.borrow().index_arc();
        let folders = self.inner.session.borrow().last_folders().to_vec();
        let folder = self.inner.session.borrow().folder().map(Path::to_path_buf);
        let persist = self.inner.session.borrow().persist_host();
        let work = self.inner.session.borrow().scan_work_arc();
        let cancel = self.inner.session.borrow().analysis_cancel_flag();
        let pack = localgallery::installed_pack();
        let (geo_cache, geo_note) = localgallery::config::load_geo_cache();
        if let Some(msg) = geo_note {
            self.toast(&msg);
        }
        let ml_cache = localgallery::config::ml_cache_path();
        self.inner.analysis_busy.set(true);
        self.inner.session.borrow().begin_work("Scan Photos");
        self.mute_watch();
        self.sync_scan_progress();
        localcore_trace::event(
            "scan",
            format!(
                "analysis start photos={} tag={} face={} place={} force={force}",
                photos.len(),
                phases.tagging,
                phases.faces,
                phases.places
            ),
        );
        let (tx, rx) = mpsc::channel();
        self.inner.analysis_rx.replace(Some(rx));
        thread::spawn(move || {
            let mut geo_cache = geo_cache;
            let geo = localgallery::Gazetteer;
            let work_progress = work.clone();
            let on_progress: localgallery::ProgressFn = Arc::new(move |progress| {
                let label = progress.phase.short_label();
                let fraction =
                    (progress.total > 0).then_some(progress.done as f64 / progress.total as f64);
                let detail =
                    (progress.total > 0).then(|| format!("{} / {}", progress.done, progress.total));
                Session::touch_shared_work(&work_progress, label, detail, fraction);
            });
            let summary = localgallery::run_analysis(localgallery::AnalysisRequest {
                photos: &photos,
                pack: pack.as_ref(),
                ml_cache: Some(&ml_cache),
                geo: &geo,
                geo_cache: &mut geo_cache,
                phases,
                force,
                cancel: &cancel,
                on_progress: Some(on_progress),
                heic_decoder: Some(localgallery::linux_heic_decoder()),
            });
            if let Err(error) = localgallery::config::save_geo_cache(&geo_cache) {
                localcore_trace::event("catalog", format!("geocode cache: {error}"));
            }
            let ui = if summary.written_paths.is_empty() {
                None
            } else {
                Some(Session::overlay_analysis_writes(
                    &index,
                    folders,
                    &summary.written_paths,
                    persist.then_some(folder.as_deref()).flatten(),
                    &work,
                ))
            };
            let _ = tx.send(AnalysisEvent::Done { summary, ui });
        });
    }

    fn start_scan(&self, folder: PathBuf, show_photos: bool) {
        if self.inner.analysis_busy.get() || self.inner.pack_busy.get() {
            self.toast("Scan Photos is already running");
            return;
        }
        self.cancel_leftover();
        if self.inner.scan_busy.get() {
            self.inner.session.borrow().cancel_scan();
        }
        self.inner.scan_busy.set(true);
        self.clear_library_models();
        self.inner.root_stack.set_visible_child_name("library");
        if show_photos {
            self.inner.shell.stack.set_visible_child_name("photos");
        }
        self.sync_scan_progress();
        let scanner = self.inner.session.borrow().scanner_arc();
        let status = self.inner.session.borrow().scan_work_arc();
        let index = self.inner.session.borrow().index_arc();
        localcore_trace::event(
            "scan",
            format!(
                "start worker folder={} show_photos={show_photos}",
                folder.display()
            ),
        );
        if let Some(bench) = self.inner.bench.borrow().as_ref() {
            bench.scan_started.set(Some(Instant::now()));
        }
        let enrich_after = self.inner.bench.borrow().is_none();
        let persist = self.inner.session.borrow().persist_host();
        let (tx, rx) = mpsc::channel();
        self.inner.scan_rx.replace(Some(rx));
        thread::spawn(move || {
            let persist_folder = persist.then(|| folder.clone());
            let loaded = persist.then(|| Session::load_scan_cache(&folder));
            let snapshot_toast = loaded
                .as_ref()
                .and_then(|load| load.reuse.recovery_message());
            let cache = loaded.and_then(|load| load.document);
            if let Some(snap) = &cache {
                let warmed = Session::prepare_ui(
                    &index,
                    Session::catalog_from_snapshot(snap),
                    false,
                    persist_folder.as_deref(),
                    &status,
                );
                if tx
                    .send(ScanWire {
                        folder: folder.clone(),
                        toasts: Vec::new(),
                        result: Ok(warmed),
                    })
                    .is_err()
                {
                    return;
                }
            }
            let walk = Session::scan_with(scanner, status.clone(), &folder, cache.as_ref());
            let unsupported = gallery_vfs::take_unsupported_names();
            let mut walk_toasts = Vec::new();
            if let Some(msg) = snapshot_toast {
                walk_toasts.push(msg);
            }
            if let Some(msg) = localgallery::row::unsupported_names_message(&unsupported) {
                walk_toasts.push(msg);
            }
            let result = walk.and_then(|mut catalog| {
                Session::mark_changed_sidecars(&mut catalog, cache.as_ref());
                let mut toasts = std::mem::take(&mut walk_toasts);
                if cache.is_none() {
                    let first = Session::prepare_ui(
                        &index,
                        catalog.clone(),
                        false,
                        persist_folder.as_deref(),
                        &status,
                    );
                    if tx
                        .send(ScanWire {
                            folder: folder.clone(),
                            toasts,
                            result: Ok(first),
                        })
                        .is_err()
                    {
                        return Ok(());
                    }
                    toasts = Vec::new();
                }
                if enrich_after {
                    if catalog.needs_enrichment {
                        Session::enrich_catalog(&mut catalog);
                    }
                    if persist {
                        if let Err(error) = Session::persist_scan_snapshot(&folder, &catalog) {
                            localcore_trace::event("catalog", format!("snapshot persist: {error}"));
                        }
                    }
                    let second = Session::prepare_ui(
                        &index,
                        catalog,
                        true,
                        persist_folder.as_deref(),
                        &status,
                    );
                    let _ = tx.send(ScanWire {
                        folder: folder.clone(),
                        toasts,
                        result: Ok(second),
                    });
                } else if persist {
                    if let Err(error) = Session::persist_scan_snapshot(&folder, &catalog) {
                        localcore_trace::event("catalog", format!("snapshot persist: {error}"));
                    }
                }
                Ok(())
            });
            if let Err(error) = result {
                let _ = tx.send(ScanWire {
                    folder,
                    toasts: walk_toasts,
                    result: Err(error.to_string()),
                });
            }
        });
    }

    fn clear_library_models(&self) {
        self.inner.photos_grid.set_model(None::<&gtk::NoSelection>);
        self.inner.folders_grid.set_model(None::<&gtk::NoSelection>);
        self.inner.photos_bound_ids.replace(None);
        self.inner.photos_fill_items.replace(None);
        self.inner.explorer_fill.replace(None);
        self.inner
            .folders_sidebar
            .set_model(None::<&gtk::SingleSelection>);
        self.inner.photos_model.replace(None);
        self.inner.folders_model.replace(None);
        self.inner.media_pages.borrow_mut().replace_generation(0);
    }

    fn finish_scan(&self, ui: PreparedUi, folder: PathBuf) {
        self.inner.scan_busy.set(false);
        let _span = localcore_trace::span_always("scan", "finish_scan");
        if let Some(error) = &ui.config_error {
            self.inner.session.borrow_mut().record(
                LogLevel::Warning,
                "folder",
                format!("Could not persist leftover config: {error}"),
            );
        }
        let ids = Arc::new(
            ui.photos
                .items()
                .iter()
                .map(|item| item.id.clone())
                .collect(),
        );
        if self.inner.photos_model.borrow().is_some() {
            let generation = ui.photos.generation;
            self.inner
                .session
                .borrow_mut()
                .install_enrich(ui.catalog, Some(ids));
            let hub = ui.hub;
            self.inner.hub.replace(Some(hub.clone()));
            if let Some(model) = self.inner.photos_model.borrow_mut().as_mut() {
                model.set_generation(generation);
            }
            self.inner
                .media_pages
                .borrow_mut()
                .replace_generation(generation);
            self.refresh_photo_grid_thumbs();
            self.bind_collections(&hub);
            self.bind_tags(&hub);
            let filtered = {
                let session = self.inner.session.borrow();
                !session.query().is_empty() || !session.required_tags().is_empty()
            };
            if filtered {
                self.request_photos();
            }
            self.sync_scan_progress();
            return;
        }
        let scan_worker_ms = self.inner.bench.borrow().as_ref().and_then(|bench| {
            bench
                .scan_started
                .get()
                .map(|started| started.elapsed().as_secs_f64() * 1000.0)
        });
        let apply_ms = ui.catalog.apply_ms;
        let gtk_started = Instant::now();
        let applied =
            self.inner
                .session
                .borrow_mut()
                .install_prepared(ui.catalog, Some(&folder), Some(ids));
        let leftover_ms = self.inner.session.borrow().last_leftover_ms();
        match applied {
            Ok(()) => {
                let hub = ui.hub;
                self.inner.hub.replace(Some(hub.clone()));
                self.inner.root_stack.set_visible_child_name("library");
                self.reset_nav();
                if self.inner.bench.borrow().is_none()
                    && self.inner.leftover_rx.borrow().is_none()
                    && self.inner.memories_rx.borrow().is_none()
                {
                    self.start_watch(&folder);
                    self.start_memories();
                }
                let refill_started = Instant::now();
                self.bind_folders(&hub.folders);
                self.bind_collections(&hub);
                self.bind_tags(&hub);
                self.bind_photos(ui.photos, ui.years);
                let refill_ms = refill_started.elapsed().as_secs_f64() * 1000.0;
                let gtk_thread_ms = gtk_started.elapsed().as_secs_f64() * 1000.0;
                if self.inner.photo_fill_active.get() {
                    if let Some(bench) = self.inner.bench.borrow().as_ref() {
                        bench.pending.set(Some((
                            scan_worker_ms.unwrap_or(0.0),
                            apply_ms,
                            leftover_ms,
                            refill_ms,
                            gtk_thread_ms,
                        )));
                    }
                } else if self.complete_bench(Ok((
                    scan_worker_ms.unwrap_or(0.0),
                    apply_ms,
                    leftover_ms,
                    refill_ms,
                    gtk_thread_ms,
                ))) {
                    return;
                }
            }
            Err(error) => {
                if self.complete_bench(Err(error.to_string())) {
                    return;
                }
                self.toast(&error.to_string());
            }
        }
        self.sync_scan_progress();
    }

    fn arm_bench(&self) {
        self.inner.bench.replace(Some(BenchState {
            started: Instant::now(),
            scan_started: Cell::new(None),
            done: Cell::new(false),
            pending: Cell::new(None),
        }));
        let this = self.clone();
        let timeout_secs = std::env::var("LOCALGALLERY_GTK_BENCH_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(600_u64)
            .max(5);
        glib::timeout_add_local_once(std::time::Duration::from_secs(timeout_secs), move || {
            if this
                .inner
                .bench
                .borrow()
                .as_ref()
                .is_some_and(|bench| bench.done.get())
            {
                return;
            }
            eprintln!("[gallery-gtk-perf] timed out waiting for catalog ({timeout_secs}s)");
            std::process::exit(1);
        });
    }

    fn complete_bench(&self, result: Result<(f64, f64, f64, f64, f64), String>) -> bool {
        let (started, already) = {
            let guard = self.inner.bench.borrow();
            let Some(bench) = guard.as_ref() else {
                return false;
            };
            (bench.started, bench.done.replace(true))
        };
        if already {
            return true;
        }
        match result {
            Ok((scan_worker_ms, apply_ms, leftover_ms, refill_ms, gtk_thread_ms)) => {
                let ready_ms = started.elapsed().as_secs_f64() * 1000.0;
                let photos = self.inner.session.borrow().photo_count();
                println!("[gallery-gtk-perf] metric_photos={photos}");
                println!("[gallery-gtk-perf] metric_scan_worker_ms={scan_worker_ms:.1}");
                println!("[gallery-gtk-perf] metric_apply_catalog_ms={apply_ms:.1}");
                println!("[gallery-gtk-perf] metric_leftover_open_ms={leftover_ms:.1}");
                println!("[gallery-gtk-perf] metric_refill_all_ms={refill_ms:.1}");
                println!("[gallery-gtk-perf] metric_gtk_thread_ms={gtk_thread_ms:.1}");
                println!("[gallery-gtk-perf] metric_ready_ms={ready_ms:.1}");
                if let Some(app) = self.inner.window.application() {
                    app.quit();
                }
            }
            Err(error) => {
                eprintln!("[gallery-gtk-perf] catalog failed: {error}");
                std::process::exit(1);
            }
        }
        true
    }

    fn start_memories(&self) {
        let (index, context, generator) = {
            let mut session = self.inner.session.borrow_mut();
            (
                session.index_arc(),
                session.memory_context(),
                session.take_memory_generator(),
            )
        };
        let (tx, rx) = mpsc::channel();
        self.inner.memories_rx.replace(Some(rx));
        self.inner
            .session
            .borrow()
            .touch_work("Memories", None, None);
        localcore_trace::event("memory", "generate queued on worker");
        thread::spawn(move || {
            let _span = localcore_trace::span_always("memory", "generate");
            let items = generator.generate(index, context);
            localcore_trace::event("memory", format!("generated n={}", items.len()));
            let _ = tx.send(items);
        });
    }

    fn cancel_leftover(&self) {
        if let Some(flag) = self.inner.leftover_cancel.borrow().as_ref() {
            flag.store(true, Ordering::Relaxed);
        }
        self.inner.leftover_rx.replace(None);
    }

    fn poll_leftover(&self) {
        let received =
            self.inner
                .leftover_rx
                .borrow()
                .as_ref()
                .and_then(|rx| match rx.try_recv() {
                    Ok(result) => Some(result),
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
                });
        let Some(result) = received else {
            return;
        };
        self.inner.leftover_rx.replace(None);
        match result {
            Ok((folder, worker_ms)) => {
                self.inner
                    .session
                    .borrow_mut()
                    .mark_leftover_loaded(&folder, worker_ms);
                self.inner.session.borrow_mut().record(
                    LogLevel::Info,
                    "snapshot",
                    format!(
                        "Leftover snapshot at {} ({worker_ms:.0}ms worker)",
                        localgallery::config::snapshot_path().display()
                    ),
                );
            }
            Err(error) if error.contains("cancel") => {}
            Err(error) => {
                self.inner.session.borrow_mut().record(
                    LogLevel::Warning,
                    "snapshot",
                    format!("Leftover open_library skipped: {error}"),
                );
            }
        }
        self.finish_job_progress();
    }

    fn reset_nav(&self) {
        let _ = self.inner.folders_nav.pop_to_tag("root");
        let _ = self.inner.collections_nav.pop_to_tag("root");
        let _ = self.inner.photos_nav.pop_to_tag("root");
        self.inner.folder_stack.replace(vec![None]);
    }

    fn start_watch(&self, folder: &Path) {
        self.inner.watch.replace(None);
        self.inner.watch_rx.replace(None);
        self.inner.watch_mute.replace(None);
        let mute = localgallery::watch::MuteGate::new();
        match localgallery::watch::start(folder.to_path_buf(), mute.clone()) {
            Ok((handle, rx)) => {
                self.inner.watch.replace(Some(handle));
                self.inner.watch_rx.replace(Some(rx));
                self.inner.watch_mute.replace(Some(mute));
            }
            Err(error) => {
                self.inner.session.borrow_mut().record(
                    LogLevel::Warning,
                    "watch",
                    format!("Folder watch is unavailable: {error}"),
                );
            }
        }
    }

    fn drain_watch(&self) {
        if let Some(rx) = self.inner.watch_rx.borrow().as_ref() {
            while rx.try_recv().is_ok() {}
        }
    }

    fn apply_deleted_photos(&self, ids: &[String]) {
        let result = self.inner.session.borrow_mut().apply_photos_removed(ids);
        if result.removed_ids.is_empty() {
            return;
        }
        let drop: HashSet<String> = result.removed_ids.iter().cloned().collect();
        if let Some(model) = self.inner.photos_model.borrow_mut().as_mut() {
            model.set_generation(result.generation);
            model.remove_ids(&drop);
        }
        let next_bound = self.inner.photos_bound_ids.borrow().as_ref().map(|bound| {
            Arc::new(
                bound
                    .iter()
                    .filter(|id| !drop.contains(id.as_str()))
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        });
        if let Some(next) = next_bound {
            self.inner.photos_bound_ids.replace(Some(next));
        }
        let next_fill = self.inner.photos_fill_items.borrow().as_ref().map(|fill| {
            fill.iter()
                .filter(|item| !drop.contains(&item.id))
                .cloned()
                .collect::<Vec<_>>()
        });
        if let Some(items) = next_fill {
            if items.is_empty() {
                self.inner.photos_fill_items.replace(None);
            } else {
                self.inner.photos_fill_items.replace(Some(Rc::new(items)));
            }
        }
        self.inner
            .media_pages
            .borrow_mut()
            .replace_generation(result.generation);
        let remaining = self
            .inner
            .photos_model
            .borrow()
            .as_ref()
            .map(ViewListModel::n_items)
            .unwrap_or(0);
        let fill_left = self
            .inner
            .photos_fill_items
            .borrow()
            .as_ref()
            .map(|items| items.len())
            .unwrap_or(0);
        if remaining == 0 && fill_left == 0 {
            self.inner.photos_host.set_visible_child_name("empty");
            self.inner.photos_grid.set_model(None::<&gtk::NoSelection>);
            self.inner.photos_model.replace(None);
            self.inner.photos_bound_ids.replace(None);
            self.inner.photos_fill_items.replace(None);
        } else if remaining == 0 && fill_left > 0 {
            self.append_photo_fill_chunk();
        }
        let structure = self.inner.session.borrow().index().photo_structure();
        let years = years_from_structure(&structure);
        let this_year = self.clone();
        year_rail::refill(&self.inner.photos_year_rail, &years, move |index| {
            this_year.ensure_photos_filled_to(index);
            this_year
                .inner
                .photos_grid
                .scroll_to(index, gtk::ListScrollFlags::FOCUS, None);
        });
        self.rebuild_folder_tree();
        self.refresh_folder_explorer();
        localcore_trace::event(
            "ui",
            format!(
                "apply_deleted_photos n={} gen={} store={}",
                result.removed_ids.len(),
                result.generation,
                remaining
            ),
        );
    }

    fn poll_idle(&self) {
        let this = self.clone();
        let ticks = Rc::new(Cell::new(0u32));
        // Scan / watch / leftover stay on this 250 ms tick. Thumbnails paint
        // on a LOW idle from the decode pool — do not upload textures here
        // (this timeout shares DEFAULT with scroll/input).
        glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
            let drained = false;
            this.poll_scan();
            this.poll_analysis();
            this.poll_pack();
            this.poll_photos();
            this.poll_drill();
            this.poll_memories();
            this.poll_leftover();
            this.poll_mutate();
            let ignore = this.inner.watch_ignore_ticks.get();
            let dirty =
                this.inner
                    .watch_rx
                    .borrow()
                    .as_ref()
                    .is_some_and(|rx| match rx.try_recv() {
                        Ok(()) => true,
                        Err(TryRecvError::Empty | TryRecvError::Disconnected) => false,
                    });
            if dirty {
                this.drain_watch();
                if ignore == 0 {
                    localcore_trace::event("watch", "dirty → reload_folder on GTK thread");
                    this.reload_folder();
                } else {
                    localcore_trace::event("watch", "dirty ignored after self-delete");
                }
            }
            if ignore > 0 {
                this.inner.watch_ignore_ticks.set(ignore - 1);
            }
            this.sync_scan_progress();
            let n = ticks.get().saturating_add(1);
            ticks.set(n);
            if localcore_trace::enabled() && (drained || n % 20 == 0) {
                let snap = this.inner.thumbs.debug_snapshot();
                let session = this.inner.session.borrow();
                localcore_trace::event(
                    "idle",
                    format!(
                        "photos={} folders={} memories={} leftover={} scan_busy={} thumbs ready={} inflight={} waiting={} drained={drained}",
                        session.photo_count(),
                        session.last_folders().len(),
                        session.memories().len(),
                        session.leftover_snapshot_loaded(),
                        this.inner.scan_busy.get()
                            || this.inner.analysis_busy.get()
                            || this.inner.pack_busy.get(),
                        snap.ready,
                        snap.inflight,
                        snap.waiting
                    ),
                );
            }
            glib::ControlFlow::Continue
        });
    }

    fn poll_scan(&self) {
        let Some(rx) = self.inner.scan_rx.borrow_mut().take() else {
            return;
        };
        match rx.try_recv() {
            Ok(packet) => {
                self.inner.scan_rx.replace(Some(rx));
                for message in &packet.toasts {
                    self.toast(message);
                }
                match packet.result {
                    Ok(prepared) => self.finish_scan(prepared, packet.folder),
                    Err(error) => {
                        self.inner.scan_rx.replace(None);
                        self.inner.scan_busy.set(false);
                        if self.complete_bench(Err(error.clone())) {
                            return;
                        }
                        if error != "cancelled" {
                            self.toast(&error);
                        }
                    }
                }
            }
            Err(TryRecvError::Empty) => {
                self.inner.scan_rx.replace(Some(rx));
            }
            Err(TryRecvError::Disconnected) => {}
        }
    }

    fn poll_analysis(&self) {
        let Some(rx) = self.inner.analysis_rx.borrow_mut().take() else {
            return;
        };
        match rx.try_recv() {
            Ok(AnalysisEvent::Done { summary, ui }) => {
                self.finish_analysis(summary, ui);
            }
            Err(TryRecvError::Empty) => {
                self.inner.analysis_rx.replace(Some(rx));
            }
            Err(TryRecvError::Disconnected) => {
                self.inner.analysis_busy.set(false);
                self.unmute_watch();
                self.inner.session.borrow().clear_work();
                self.sync_scan_progress();
            }
        }
    }

    fn finish_analysis(&self, summary: localgallery::AnalysisSummary, ui: Option<PreparedUi>) {
        self.inner.analysis_busy.set(false);
        self.inner.analysis_rx.replace(None);
        self.unmute_watch();
        let line = summary.toast_line();
        self.inner
            .session
            .borrow_mut()
            .store_analysis_summary(summary);
        if let Some(ui) = ui {
            let ids = Arc::new(
                ui.photos
                    .items()
                    .iter()
                    .map(|item| item.id.clone())
                    .collect(),
            );
            let generation = ui.photos.generation;
            self.inner
                .session
                .borrow_mut()
                .install_enrich(ui.catalog, Some(ids));
            let hub = ui.hub;
            self.inner.hub.replace(Some(hub.clone()));
            if let Some(model) = self.inner.photos_model.borrow_mut().as_mut() {
                model.set_generation(generation);
            }
            self.inner
                .media_pages
                .borrow_mut()
                .replace_generation(generation);
            self.refresh_photo_grid_thumbs();
            self.bind_collections(&hub);
            self.bind_tags(&hub);
            let filtered = {
                let session = self.inner.session.borrow();
                !session.query().is_empty() || !session.required_tags().is_empty()
            };
            if filtered {
                self.request_photos();
            }
            self.refresh_people_and_memories();
        }
        self.inner.session.borrow().clear_work();
        self.sync_scan_progress();
        self.toast(&line);
    }

    fn start_pack_download(&self) {
        if self.inner.scan_busy.get()
            || self.inner.analysis_busy.get()
            || self.inner.pack_busy.get()
        {
            self.toast("A scan is already running");
            return;
        }
        if localgallery::xdg_pack_present() {
            self.toast("ML models are already installed");
            return;
        }
        self.inner.pack_busy.set(true);
        self.inner.session.borrow().begin_work("Download ML models");
        self.sync_scan_progress();
        localcore_trace::event("pack", "download start");
        let (tx, rx) = mpsc::channel();
        self.inner.pack_rx.replace(Some(rx));
        thread::spawn(move || {
            let result = localgallery::download_pack().map_err(|error| error.to_string());
            let _ = tx.send(PackEvent::Downloaded(result));
        });
    }

    fn start_pack_remove(&self) {
        if self.inner.scan_busy.get()
            || self.inner.analysis_busy.get()
            || self.inner.pack_busy.get()
        {
            self.toast("A scan is already running");
            return;
        }
        if !localgallery::xdg_pack_present() {
            self.toast("No ML models to remove");
            return;
        }
        self.inner.pack_busy.set(true);
        self.inner.session.borrow().begin_work("Remove ML models");
        self.sync_scan_progress();
        localcore_trace::event("pack", "remove start");
        let (tx, rx) = mpsc::channel();
        self.inner.pack_rx.replace(Some(rx));
        thread::spawn(move || {
            let result = localgallery::remove_installed_pack().map_err(|error| error.to_string());
            let _ = tx.send(PackEvent::Removed(result));
        });
    }

    fn poll_pack(&self) {
        let Some(rx) = self.inner.pack_rx.borrow_mut().take() else {
            return;
        };
        match rx.try_recv() {
            Ok(event) => {
                self.inner.pack_busy.set(false);
                self.inner.pack_rx.replace(None);
                self.inner.session.borrow().clear_work();
                self.sync_scan_progress();
                match event {
                    PackEvent::Downloaded(Ok(pack)) => {
                        let version = if pack.version.is_empty() {
                            pack.name
                        } else {
                            pack.version
                        };
                        localcore_trace::event("pack", format!("download ok version={version}"));
                        self.toast(&format!("ML models installed · {version}"));
                    }
                    PackEvent::Downloaded(Err(error)) => {
                        localcore_trace::event("pack", format!("download failed: {error}"));
                        self.toast(&error);
                    }
                    PackEvent::Removed(Ok(())) => {
                        localcore_trace::event("pack", "remove ok");
                        self.toast("ML models removed");
                    }
                    PackEvent::Removed(Err(error)) => {
                        localcore_trace::event("pack", format!("remove failed: {error}"));
                        self.toast(&error);
                    }
                }
                self.refresh_settings();
            }
            Err(TryRecvError::Empty) => {
                self.inner.pack_rx.replace(Some(rx));
            }
            Err(TryRecvError::Disconnected) => {
                self.inner.pack_busy.set(false);
                self.inner.session.borrow().clear_work();
                self.sync_scan_progress();
            }
        }
    }

    fn refresh_settings(&self) {
        let Some(dialog) = self.inner.settings_dialog.borrow().clone() else {
            return;
        };
        if !dialog.is_visible() {
            return;
        }
        dialog.close();
        self.inner.settings_dialog.replace(None);
        self.present_settings();
    }

    fn poll_memories(&self) {
        let received =
            self.inner
                .memories_rx
                .borrow()
                .as_ref()
                .and_then(|rx| match rx.try_recv() {
                    Ok(items) => Some(items),
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
                });
        if let Some(items) = received {
            localcore_trace::event(
                "memory",
                format!("store+refill collections n={}", items.len()),
            );
            self.inner.memories_rx.replace(None);
            self.inner.session.borrow_mut().store_memories(items);
            self.bind_memories_section();
            self.finish_job_progress();
        }
    }

    fn cancel_work(&self) {
        self.inner.session.borrow().cancel_analysis();
        self.inner.session.borrow().cancel_scan();
        self.inner
            .photo_fill_gen
            .set(self.inner.photo_fill_gen.get().saturating_add(1));
        self.inner.photo_fill_active.set(false);
        self.inner.photos_rx.replace(None);
        self.inner.drill_rx.replace(None);
        self.inner.pending_drill.replace(None);
        self.cancel_leftover();
        self.inner.memories_rx.replace(None);
        self.inner.scan_busy.set(false);
        if !self.inner.analysis_busy.get() {
            self.inner.session.borrow().clear_work();
        }
        self.sync_scan_progress();
    }

    fn work_still_running(&self) -> bool {
        self.inner.scan_busy.get()
            || self.inner.analysis_busy.get()
            || self.inner.pack_busy.get()
            || self.inner.photo_fill_active.get()
            || self.inner.photos_rx.borrow().is_some()
            || self.inner.drill_rx.borrow().is_some()
            || self.inner.leftover_rx.borrow().is_some()
            || self.inner.memories_rx.borrow().is_some()
    }

    fn finish_job_progress(&self) {
        if !self.work_still_running() {
            self.inner.session.borrow().clear_work();
        }
        self.sync_scan_progress();
    }

    fn refill_search_hits(&self) {
        let query = self.inner.photos_search.text().to_string();
        let host = &self.inner.photos_hits;
        while let Some(child) = host.first_child() {
            host.remove(&child);
        }
        if query.trim().is_empty() {
            host.set_visible(false);
            return;
        }
        let hits = self.inner.session.borrow().search_hits(&query);
        if hits.is_empty() {
            host.set_visible(false);
            return;
        }
        for hit in hits {
            let subtitle = format!(
                "{}: {}",
                gtk::glib::markup_escape_text(hit.kind.label()),
                hit.subtitle
                    .as_deref()
                    .map(|text| highlight_markup(text, &query))
                    .unwrap_or_else(|| highlight_markup(&hit.title, &query))
            );
            let row = search_hit_row(&hit.title, &subtitle, hit.kind.symbol());
            row.set_widget_name(&hit.id);
            host.append(&row);
        }
        host.set_visible(true);
    }

    fn activate_search_hit(&self, id: &str) {
        if let Some(date) = id.strip_prefix("date:") {
            self.inner.photos_search.set_text(date);
            return;
        }
        self.inner.photos_search.set_text("");
        self.add_required_tag(id);
    }

    fn queue_photos_reload(&self) {
        if self.inner.photos_reload_queued.get() {
            return;
        }
        self.inner.photos_reload_queued.set(true);
        let this = self.clone();
        glib::idle_add_local_once(move || {
            this.inner.photos_reload_queued.set(false);
            this.request_photos();
        });
    }

    fn request_photos(&self) {
        let (index, intent) = {
            let mut session = self.inner.session.borrow_mut();
            let intent = session.photos_intent().clone();
            session.set_current_intent(intent.clone());
            (session.index_arc(), intent)
        };
        self.inner.session.borrow().begin_work("Filtering");
        let (tx, rx) = mpsc::channel();
        self.inner.photos_rx.replace(Some(rx));
        thread::spawn(move || {
            let (list, years) = Session::project_photos(&index, &intent);
            let ids = Arc::new(list.items().iter().map(|item| item.id.clone()).collect());
            let _ = tx.send(PreparedPhotos { list, years, ids });
        });
        self.sync_scan_progress();
    }

    fn poll_photos(&self) {
        let Some(rx) = self.inner.photos_rx.borrow_mut().take() else {
            return;
        };
        let prepared = match rx.try_recv() {
            Ok(prepared) => prepared,
            Err(TryRecvError::Empty) => {
                self.inner.photos_rx.replace(Some(rx));
                return;
            }
            Err(TryRecvError::Disconnected) => return,
        };
        self.inner
            .session
            .borrow_mut()
            .set_visible_ids(prepared.ids);
        self.bind_photos(prepared.list, prepared.years);
    }

    fn poll_drill(&self) {
        let Some(rx) = self.inner.drill_rx.borrow_mut().take() else {
            return;
        };
        let (token, list) = match rx.try_recv() {
            Ok(payload) => payload,
            Err(TryRecvError::Empty) => {
                self.inner.drill_rx.replace(Some(rx));
                return;
            }
            Err(TryRecvError::Disconnected) => return,
        };
        let Some(pending) = self.inner.pending_drill.take() else {
            self.finish_job_progress();
            return;
        };
        if pending.token != token {
            self.inner.pending_drill.replace(Some(pending));
            self.finish_job_progress();
            return;
        }
        self.bind_photo_list_to(&pending.grid, &pending.cache, &pending.model, list);
        self.finish_job_progress();
    }

    fn bind_photos(&self, list: ViewList, years: Vec<YearMark>) {
        let _span = localcore_trace::span_always("ui", "bind_photos");
        let generation = list.generation;
        let items = list.into_items();
        let ids: Arc<Vec<String>> = Arc::new(items.iter().map(|item| item.id.clone()).collect());
        let this_year = self.clone();
        year_rail::refill(&self.inner.photos_year_rail, &years, move |index| {
            this_year.ensure_photos_filled_to(index);
            this_year
                .inner
                .photos_grid
                .scroll_to(index, gtk::ListScrollFlags::FOCUS, None);
        });
        self.inner
            .media_pages
            .borrow_mut()
            .replace_generation(generation);
        if self
            .inner
            .photos_bound_ids
            .borrow()
            .as_ref()
            .is_some_and(|bound| same_item_ids(bound, &items))
        {
            if let Some(model) = self.inner.photos_model.borrow_mut().as_mut() {
                model.set_generation(generation);
            }
            localcore_trace::event(
                "ui",
                format!(
                    "bind_photos skipped same ids n={} gen={generation}",
                    ids.len()
                ),
            );
            self.refresh_photo_grid_thumbs();
            self.inner.photo_fill_active.set(false);
            self.finish_job_progress();
            self.complete_pending_bench();
            return;
        }
        let fill_gen = self.inner.photo_fill_gen.get().saturating_add(1);
        self.inner.photo_fill_gen.set(fill_gen);
        if items.is_empty() {
            self.inner.photos_host.set_visible_child_name("empty");
            self.inner.photos_grid.set_model(None::<&gtk::NoSelection>);
            self.inner.photos_model.replace(None);
            self.inner.photos_bound_ids.replace(None);
            self.inner.photos_fill_items.replace(None);
            self.inner.photo_fill_active.set(false);
            self.finish_job_progress();
            self.complete_pending_bench();
            return;
        }
        self.inner.photos_host.set_visible_child_name("grid");
        let model = ViewListModel::empty(generation);
        let selection = gtk::NoSelection::new(Some(model.store()));
        self.inner.photos_grid.set_model(Some(&selection));
        self.inner.photos_model.replace(Some(model.clone()));
        self.inner.photos_bound_ids.replace(Some(ids));
        let first = PHOTO_FILL_AHEAD.min(items.len());
        model.append_items(&items[..first]);
        self.inner
            .photos_fill_items
            .replace((items.len() > first).then(|| Rc::new(items)));
        self.update_photo_columns(self.inner.photos_grid.width());
        self.inner.photo_fill_active.set(false);
        self.finish_job_progress();
        self.complete_pending_bench();
    }

    fn maybe_fill_photos(&self) {
        if self.inner.photos_fill_queued.get() {
            return;
        }
        if !adjustment_near_end(&self.inner.photos_scroll.vadjustment()) {
            return;
        }
        self.inner.photos_fill_queued.set(true);
        let this = self.clone();
        glib::idle_add_local_full(glib::Priority::LOW, move || {
            this.inner.photos_fill_queued.set(false);
            if adjustment_near_end(&this.inner.photos_scroll.vadjustment()) {
                this.append_photo_fill_chunk();
            }
            glib::ControlFlow::Break
        });
    }

    fn ensure_photos_filled_to(&self, index: u32) {
        let need = (index as usize).saturating_add(PHOTO_FILL_CHUNK);
        while self
            .inner
            .photos_model
            .borrow()
            .as_ref()
            .is_some_and(|model| (model.n_items() as usize) < need)
        {
            if !self.append_photo_fill_chunk() {
                break;
            }
        }
    }

    fn append_photo_fill_chunk(&self) -> bool {
        let Some(items) = self.inner.photos_fill_items.borrow().clone() else {
            return false;
        };
        let Some(model) = self.inner.photos_model.borrow().clone() else {
            return false;
        };
        let have = model.n_items() as usize;
        if have >= items.len() {
            self.inner.photos_fill_items.replace(None);
            return false;
        }
        let end = (have + PHOTO_FILL_AHEAD).min(items.len());
        model.append_items(&items[have..end]);
        if end >= items.len() {
            self.inner.photos_fill_items.replace(None);
        }
        true
    }

    fn schedule_photo_fill(
        &self,
        model: ViewListModel,
        items: Rc<Vec<ViewItem>>,
        offset: usize,
        fill_gen: u64,
    ) {
        let this = self.clone();
        glib::idle_add_local_once(move || {
            if this.inner.photo_fill_gen.get() != fill_gen {
                return;
            }
            let end = (offset + PHOTO_FILL_CHUNK).min(items.len());
            model.append_items(&items[offset..end]);
            this.inner.session.borrow().touch_work_counts(
                "Loading photos",
                end as u64,
                items.len() as u64,
            );
            this.sync_scan_progress();
            if end >= items.len() {
                this.inner.photo_fill_active.set(false);
                if this.inner.leftover_rx.borrow().is_some() {
                    this.inner
                        .session
                        .borrow()
                        .touch_work("Snapshot", None, None);
                } else if this.inner.memories_rx.borrow().is_some() {
                    this.inner
                        .session
                        .borrow()
                        .touch_work("Memories", None, None);
                }
                this.finish_job_progress();
                this.complete_pending_bench();
                return;
            }
            this.schedule_photo_fill(model, items, end, fill_gen);
        });
    }

    fn complete_pending_bench(&self) {
        let pending = self
            .inner
            .bench
            .borrow()
            .as_ref()
            .and_then(|bench| bench.pending.take());
        if let Some(metrics) = pending {
            self.complete_bench(Ok(metrics));
        }
    }

    fn bind_photo_list_to(
        &self,
        grid: &gtk::GridView,
        cache: &RefCell<PageCache<gallery_ffi::GalleryMediaItem>>,
        model_cell: &RefCell<Option<ViewListModel>>,
        list: ViewList,
    ) {
        let generation = list.generation;
        cache.borrow_mut().replace_generation(generation);
        let model = if list.n_items() as usize <= PHOTO_FILL_CHUNK {
            ViewListModel::from_list(&list)
        } else {
            let model = ViewListModel::empty(generation);
            model.append_items(&list.items()[..PHOTO_FILL_CHUNK.min(list.items().len())]);
            let rest = list
                .into_items()
                .into_iter()
                .skip(PHOTO_FILL_CHUNK)
                .collect::<Vec<_>>();
            if !rest.is_empty() {
                let fill_gen = self.inner.photo_fill_gen.get().saturating_add(1);
                self.inner.photo_fill_gen.set(fill_gen);
                self.inner.photo_fill_active.set(true);
                self.inner.session.borrow().touch_work_counts(
                    "Loading photos",
                    PHOTO_FILL_CHUNK as u64,
                    (PHOTO_FILL_CHUNK + rest.len()) as u64,
                );
                self.schedule_photo_fill(model.clone(), Rc::new(rest), 0, fill_gen);
            }
            model
        };
        let selection = gtk::NoSelection::new(Some(model.store()));
        grid.set_model(Some(&selection));
        model_cell.replace(Some(model));
    }

    fn queue_photo_projection(
        &self,
        grid: gtk::GridView,
        cache: Rc<RefCell<PageCache<gallery_ffi::GalleryMediaItem>>>,
        model: Rc<RefCell<Option<ViewListModel>>>,
    ) {
        let (index, intent) = {
            let session = self.inner.session.borrow();
            (session.index_arc(), session.current_intent().clone())
        };
        let token = self.inner.drill_token.get().saturating_add(1);
        self.inner.drill_token.set(token);
        self.inner.pending_drill.replace(Some(PendingDrill {
            token,
            grid,
            cache,
            model,
        }));
        self.inner
            .session
            .borrow()
            .touch_work("Loading photos", None, None);
        let (tx, rx) = mpsc::channel();
        self.inner.drill_rx.replace(Some(rx));
        thread::spawn(move || {
            let (list, _) = Session::project_photos(&index, &intent);
            let _ = tx.send((token, list));
        });
        self.sync_scan_progress();
    }

    fn bind_folders(&self, page: &TextPage) {
        self.inner.folder_page.replace(Some(page.clone()));
        self.rebuild_folder_tree();
        self.refresh_folder_explorer();
    }

    fn bind_tags(&self, hub: &PreparedHub) {
        let selected = self.inner.session.borrow().required_tags().to_vec();
        self.refill_tag_chips(&hub.tags.rows, &hub.people.rows, &selected);
    }

    fn current_folder_id(&self) -> Option<String> {
        self.inner.folder_stack.borrow().last().cloned().flatten()
    }

    fn show_folder(&self, id: Option<&str>) {
        let _span = localcore_trace::span_always("folders", "show_folder")
            .extra("id", id.unwrap_or("root"));
        let stack = self.inner.session.borrow().folder_path_stack(id);
        self.inner.folder_stack.replace(stack);
        self.refresh_folder_explorer();
    }

    fn folder_back(&self) {
        let parent = {
            let stack = self.inner.folder_stack.borrow();
            if stack.len() < 2 {
                None
            } else {
                stack[stack.len() - 2].clone()
            }
        };
        self.show_folder(parent.as_deref());
    }

    fn refresh_folder_explorer(&self) {
        let current = self.current_folder_id();
        self.inner
            .folders_host
            .set_widget_name(if current.is_some() {
                route_id(GalleryScreen::Folder)
            } else {
                route_id(GalleryScreen::Folders)
            });
        self.refill_folder_crumbs(current.as_deref());
        self.select_sidebar_folder(current.as_deref());
        self.bind_explorer_grid(current.as_deref());
    }

    fn refill_folder_crumbs(&self, current: Option<&str>) {
        let host = &self.inner.folders_crumbs;
        while let Some(child) = host.first_child() {
            host.remove(&child);
        }
        self.inner.folders_back.set_visible(current.is_some());
        let trail = self.inner.session.borrow().folder_crumbs(current);
        let root = crumb_button("Folders", current.is_none());
        let this = self.clone();
        root.connect_clicked(move |_| this.show_folder(None));
        host.append(&root);
        for (index, entry) in trail.iter().enumerate() {
            host.append(&crumb_sep());
            let last = index + 1 == trail.len();
            let button = crumb_button(&entry.name, last);
            if !last {
                let this = self.clone();
                let id = entry.id.clone();
                button.connect_clicked(move |_| this.show_folder(Some(&id)));
            }
            host.append(&button);
        }
    }

    fn rebuild_folder_tree(&self) {
        let folders = self.inner.session.borrow().last_folders().to_vec();
        let folders = Rc::new(folders);
        let roots = crate::folders::listing_entries(&folders, None);
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        for entry in roots {
            store.append(&glib::BoxedAnyObject::new(entry));
        }
        let tree_folders = folders.clone();
        let tree = gtk::TreeListModel::new(store, false, false, move |item| {
            let boxed = item.downcast_ref::<glib::BoxedAnyObject>()?;
            let entry = boxed.borrow::<FolderEntry>().clone();
            if !entry.has_children {
                return None;
            }
            let children = crate::folders::listing_entries(&tree_folders, Some(&entry.id));
            if children.is_empty() {
                return None;
            }
            let store = gio::ListStore::new::<glib::BoxedAnyObject>();
            for child in children {
                store.append(&glib::BoxedAnyObject::new(child));
            }
            Some(store.upcast::<gio::ListModel>())
        });
        let selection = gtk::SingleSelection::new(Some(tree));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);
        self.inner.folders_sidebar.set_model(Some(&selection));
    }

    fn select_sidebar_folder(&self, id: Option<&str>) {
        let Some(selection) = self
            .inner
            .folders_sidebar
            .model()
            .and_downcast::<gtk::SingleSelection>()
        else {
            return;
        };
        let Some(id) = id else {
            selection.set_selected(gtk::INVALID_LIST_POSITION);
            return;
        };
        for index in 0..selection.n_items() {
            if sidebar_folder_id(&self.inner.folders_sidebar, index).as_deref() == Some(id) {
                selection.set_selected(index);
                return;
            }
        }
    }

    fn bind_explorer_grid(&self, current: Option<&str>) {
        let fill_gen = self.inner.folders_fill_gen.get().saturating_add(1);
        self.inner.folders_fill_gen.set(fill_gen);
        let session = self.inner.session.borrow();
        let folders = session.folder_entries(current);
        let photo_folder = session.explorer_photo_folder_id(current);
        let photo_count = photo_folder
            .as_deref()
            .map(|id| session.folder_own_photo_ids(id).len())
            .unwrap_or(0);
        drop(session);
        if folders.is_empty() && photo_count == 0 {
            self.inner.folders_host.set_visible_child_name("empty");
            self.inner.folders_grid.set_model(None::<&gtk::NoSelection>);
            self.inner.folders_model.replace(None);
            self.inner.explorer_fill.replace(None);
            return;
        }
        self.inner.folders_host.set_visible_child_name("list");
        let mut items = Vec::with_capacity(folders.len() + PHOTO_FILL_CHUNK.min(photo_count));
        for (index, folder) in folders.iter().enumerate() {
            items.push(ViewItem {
                id: folder.id.clone(),
                section: "folders".into(),
                index: index as u32,
            });
        }
        let folder_n = items.len();
        self.inner.explorer_fill.replace(None);
        if self.folders_tab_is_visible() {
            if let Some(photo_folder) = photo_folder.clone() {
                let first_ids = {
                    let session = self.inner.session.borrow();
                    let ids = session.folder_own_photo_ids(&photo_folder);
                    let first = PHOTO_FILL_AHEAD.min(ids.len());
                    ids[..first].to_vec()
                };
                for (offset, id) in first_ids.iter().enumerate() {
                    items.push(ViewItem {
                        id: id.clone(),
                        section: "photos".into(),
                        index: (folder_n + offset) as u32,
                    });
                }
                if photo_count > first_ids.len() {
                    self.inner.explorer_fill.replace(Some(ExplorerFill {
                        folder_id: photo_folder,
                        folder_n,
                        total: photo_count,
                    }));
                }
            }
        }
        let model = ViewListModel::empty(fill_gen);
        model.append_items(&items);
        let selection = gtk::NoSelection::new(Some(model.store()));
        self.inner.folders_grid.set_model(Some(&selection));
        self.inner.folders_model.replace(Some(model.clone()));
    }

    fn folders_tab_is_visible(&self) -> bool {
        self.inner
            .shell
            .stack
            .visible_child_name()
            .is_some_and(|name| name == "folders")
    }

    fn ensure_explorer_photos(&self) {
        if !self.folders_tab_is_visible() {
            return;
        }
        let has_photos = self
            .inner
            .folders_model
            .borrow()
            .as_ref()
            .is_some_and(|model| {
                (0..model.n_items()).any(|index| {
                    model
                        .item(index)
                        .is_some_and(|item| item.section == "photos")
                })
            });
        if !has_photos {
            self.refresh_folder_explorer();
        }
    }

    fn maybe_fill_explorer(&self) {
        if !self.folders_tab_is_visible() {
            return;
        }
        if self.inner.explorer_fill_queued.get() {
            return;
        }
        if !adjustment_near_end(&self.inner.folders_scroll.vadjustment()) {
            return;
        }
        self.inner.explorer_fill_queued.set(true);
        let this = self.clone();
        glib::idle_add_local_full(glib::Priority::LOW, move || {
            this.inner.explorer_fill_queued.set(false);
            if this.folders_tab_is_visible()
                && adjustment_near_end(&this.inner.folders_scroll.vadjustment())
            {
                this.append_explorer_fill_chunk();
            }
            glib::ControlFlow::Break
        });
    }

    fn append_explorer_fill_chunk(&self) -> bool {
        let Some(fill) = self.inner.explorer_fill.borrow().clone() else {
            return false;
        };
        let Some(model) = self.inner.folders_model.borrow().clone() else {
            return false;
        };
        let have_photos = (model.n_items() as usize).saturating_sub(fill.folder_n);
        if have_photos >= fill.total {
            self.inner.explorer_fill.replace(None);
            return false;
        }
        let session = self.inner.session.borrow();
        let ids = session.folder_own_photo_ids(&fill.folder_id);
        let end = (have_photos + PHOTO_FILL_AHEAD).min(ids.len());
        let chunk: Vec<ViewItem> = ids[have_photos..end]
            .iter()
            .enumerate()
            .map(|(index, id)| ViewItem {
                id: id.clone(),
                section: "photos".into(),
                index: (fill.folder_n + have_photos + index) as u32,
            })
            .collect();
        drop(session);
        model.append_items(&chunk);
        if end >= fill.total {
            self.inner.explorer_fill.replace(None);
        }
        true
    }

    fn activate_explorer_photo(&self, id: &str) {
        let current = self.current_folder_id();
        if let Some(folder_id) = self
            .inner
            .session
            .borrow()
            .explorer_photo_folder_id(current.as_deref())
        {
            let ids = Arc::new(
                self.inner
                    .session
                    .borrow()
                    .folder_own_photo_ids(&folder_id)
                    .to_vec(),
            );
            self.inner.session.borrow_mut().set_visible_ids(ids);
        }
        self.activate_photo(id, ViewerHost::Folders);
    }

    fn bind_folder_tile(&self, root: &gtk::Widget, entry: &FolderEntry) {
        let cover = self.inner.session.borrow().folder_cover(&entry.id);
        self.bind_cover(
            root,
            &entry.name,
            &photo_count_caption(entry.total_photo_count.max(0) as usize),
            cover
                .as_ref()
                .map(|(id, host)| (id.as_str(), host.path.as_str())),
        );
    }

    fn unbind_explorer_tile(&self, root: &gtk::Widget) {
        self.unbind_cover(root);
        if let Some(picture) = find_named_widget::<gtk::Picture>(root, "photo-pic") {
            self.inner.thumbs.recycle(&picture);
        }
    }

    fn cover_scale(&self) -> u32 {
        self.inner.window.scale_factor().max(1) as u32
    }

    fn bind_cover(
        &self,
        root: &gtk::Widget,
        title: &str,
        subtitle: &str,
        photo: Option<(&str, &str)>,
    ) {
        set_cover_captions(root, title, subtitle);
        if let Some((id, path)) = photo {
            self.bind_cover_photo(root, id, path, None);
        }
    }

    fn bind_cover_photo(
        &self,
        root: &gtk::Widget,
        id: &str,
        path: &str,
        region: Option<&gallery_model::photo::FaceRegion>,
    ) {
        if let Some(picture) = find_named_widget::<gtk::Picture>(root, "cover-pic") {
            if let Some(region) = region {
                self.inner
                    .thumbs
                    .bind_face(&picture, path, id, self.cover_scale(), region);
            } else {
                self.inner
                    .thumbs
                    .bind_grid(&picture, path, id, self.cover_scale());
            }
        }
    }

    fn unbind_cover(&self, root: &gtk::Widget) {
        if let Some(picture) = find_named_widget::<gtk::Picture>(root, "cover-pic") {
            self.inner.thumbs.recycle(&picture);
        }
    }

    fn refill_photo_grid(
        &self,
        grid: gtk::GridView,
        cache: Rc<RefCell<PageCache<gallery_ffi::GalleryMediaItem>>>,
        model_cell: Rc<RefCell<Option<ViewListModel>>>,
    ) {
        self.queue_photo_projection(grid, cache, model_cell);
    }

    fn refill_tag_chips(
        &self,
        tags: &[gallery_ffi::GalleryTextRow],
        people: &[gallery_ffi::GalleryTextRow],
        selected: &[String],
    ) {
        let host = &self.inner.photos_chips;
        while let Some(child) = host.first_child() {
            host.remove(&child);
        }
        if selected.is_empty() {
            host.set_visible(false);
            return;
        }
        let chips: Vec<(String, Chip)> = selected
            .iter()
            .map(|id| {
                let title = tags
                    .iter()
                    .chain(people)
                    .find(|row| row.id == *id)
                    .map(|row| row.title.clone())
                    .filter(|title| !title.is_empty())
                    .unwrap_or_else(|| display_filter_label(id));
                (
                    id.clone(),
                    Chip {
                        label: title,
                        mode: ChipMode::Removable,
                        icon: Some(filter_chip_icon(id).into()),
                    },
                )
            })
            .collect();
        let bar = chip_bar(
            &chips
                .iter()
                .map(|(_, chip)| chip.clone())
                .collect::<Vec<_>>(),
        );
        let mut child = bar.first_child();
        for (id, _) in &chips {
            let Some(button) = child.and_downcast::<gtk::Button>() else {
                break;
            };
            let next = button.next_sibling();
            let this = self.clone();
            let id = id.clone();
            button.connect_clicked(move |_| this.remove_required_tag(&id));
            child = next;
        }
        host.append(&bar);
        host.set_visible(true);
    }

    fn add_required_tag(&self, id: &str) {
        let mut tags = self.inner.session.borrow().required_tags().to_vec();
        if tags.iter().any(|tag| tag == id) {
            return;
        }
        tags.push(id.to_string());
        self.inner.session.borrow_mut().set_required_tags(tags);
        let hub = self.inner.hub.borrow().clone();
        if let Some(hub) = hub {
            self.bind_tags(&hub);
        }
        self.request_photos();
    }

    fn remove_required_tag(&self, id: &str) {
        let mut tags = self.inner.session.borrow().required_tags().to_vec();
        tags.retain(|tag| tag != id);
        self.inner.session.borrow_mut().set_required_tags(tags);
        let hub = self.inner.hub.borrow().clone();
        if let Some(hub) = hub {
            self.bind_tags(&hub);
        }
        self.request_photos();
    }

    fn bind_collections(&self, hub: &PreparedHub) {
        let _span = localcore_trace::span_always("ui", "bind_collections");
        let width = self.collections_content_width();
        let people_limit = people_hub_preview_limit(width);
        let events_limit = events_hub_preview_limit(width);
        self.inner.hub_people_limit.set(people_limit);
        self.inner.hub_events_limit.set(events_limit);
        let host = &self.inner.collections_box;
        while let Some(child) = host.first_child() {
            host.remove(&child);
        }
        let memories = self.inner.session.borrow().memories().to_vec();
        if !memories.is_empty() {
            host.append(&hub_section(
                "Memories",
                None,
                self.memories_rail(&memories),
            ));
        }
        let people = self.live_people_page();
        let people_see_all = self.people_see_all_needed(people.rows.len(), people_limit);
        if !people.rows.is_empty() || people_see_all {
            let see_all = people_see_all.then(|| {
                let this = self.clone();
                see_all_button(move || this.push_people())
            });
            host.append(&hub_section(
                "People",
                see_all,
                self.people_hub(&people.rows, people_limit),
            ));
        }
        if !hub.events.is_empty() {
            let preview = &hub.events[..hub.events.len().min(events_limit)];
            let see_all = (hub.events.len() > events_limit).then(|| {
                let this = self.clone();
                see_all_button(move || this.push_events())
            });
            host.append(&hub_section("Events", see_all, self.events_rail(preview)));
        }
        if host.first_child().is_none() {
            localcore_trace::event(
                "collections",
                "empty hub — people need sidecar tags; memories arrive after worker",
            );
            let empty = empty_state(
                EmptyKind::EmptyFolder,
                &EmptyCopy {
                    title: "No collections".into(),
                    description: Some("Memories and people appear after a scan.".into()),
                    action: None,
                },
            );
            empty.page.set_widget_name("hub-empty");
            host.append(&empty.page);
        } else {
            localcore_trace::event(
                "collections",
                format!("hub memories={} people_ok", memories.len()),
            );
        }
    }

    fn bind_memories_section(&self) {
        let _span = localcore_trace::span_always("ui", "bind_memories_section");
        let host = &self.inner.collections_box;
        let memories = self.inner.session.borrow().memories().to_vec();
        let mut child = host.first_child();
        while let Some(widget) = child {
            let next = widget.next_sibling();
            let name = widget.widget_name();
            if name == "hub-memories" || name == "hub-empty" {
                host.remove(&widget);
            }
            child = next;
        }
        if memories.is_empty() {
            if host.first_child().is_none() {
                let empty = empty_state(
                    EmptyKind::EmptyFolder,
                    &EmptyCopy {
                        title: "No collections".into(),
                        description: Some("Memories and people appear after a scan.".into()),
                        action: None,
                    },
                );
                empty.page.set_widget_name("hub-empty");
                host.append(&empty.page);
            }
            return;
        }
        host.prepend(&hub_section(
            "Memories",
            None,
            self.memories_rail(&memories),
        ));
    }

    fn schedule_collections_reflow(&self) {
        if self.inner.hub_reflow_queued.get() {
            return;
        }
        self.inner.hub_reflow_queued.set(true);
        let this = self.clone();
        glib::idle_add_local_once(move || {
            this.inner.hub_reflow_queued.set(false);
            this.reflow_collections_hub();
        });
    }

    fn reflow_collections_hub(&self) {
        let (people_n, events_n) = {
            let hub = self.inner.hub.borrow();
            let Some(hub) = hub.as_ref() else {
                return;
            };
            (hub.people.rows.len(), hub.events.len())
        };
        let width = self.collections_content_width();
        let people = if people_n == 0 {
            self.inner.hub_people_limit.get()
        } else {
            people_hub_preview_limit(width)
        };
        let events = if events_n == 0 {
            self.inner.hub_events_limit.get()
        } else {
            events_hub_preview_limit(width)
        };
        let people_changed = people != self.inner.hub_people_limit.get();
        let events_changed = events != self.inner.hub_events_limit.get();
        if !people_changed && !events_changed {
            return;
        }
        localcore_trace::event(
            "collections",
            format!("reflow width={width} people={people} events={events}"),
        );
        if people_changed {
            self.inner.hub_people_limit.set(people);
            let rows = self
                .inner
                .hub
                .borrow()
                .as_ref()
                .map(|hub| hub.people.rows.clone())
                .unwrap_or_default();
            if !rows.is_empty() {
                let see_all = self.people_see_all_needed(rows.len(), people).then(|| {
                    let this = self.clone();
                    see_all_button(move || this.push_people())
                });
                self.replace_hub_section(
                    "hub-people",
                    hub_section("People", see_all, self.people_hub(&rows, people)),
                );
            }
        }
        if events_changed {
            self.inner.hub_events_limit.set(events);
            let (preview, total) = self
                .inner
                .hub
                .borrow()
                .as_ref()
                .map(|hub| {
                    (
                        hub.events.iter().take(events).cloned().collect::<Vec<_>>(),
                        hub.events.len(),
                    )
                })
                .unwrap_or_default();
            if !preview.is_empty() {
                let see_all = (total > events).then(|| {
                    let this = self.clone();
                    see_all_button(move || this.push_events())
                });
                self.replace_hub_section(
                    "hub-events",
                    hub_section("Events", see_all, self.events_rail(&preview)),
                );
            }
        }
    }

    fn put_hub_section(&self, name: &str, section: gtk::Widget) {
        let host = &self.inner.collections_box;
        let mut child = host.first_child();
        let mut prev: Option<gtk::Widget> = None;
        while let Some(widget) = child {
            let next = widget.next_sibling();
            if widget.widget_name() == name {
                host.insert_child_after(&section, prev.as_ref());
                host.remove(&widget);
                return;
            }
            prev = Some(widget);
            child = next;
        }
        if let Some(memories) = first_named_child(host, "hub-memories") {
            host.insert_child_after(&section, Some(&memories));
        } else {
            host.prepend(&section);
        }
    }

    fn remove_hub_section(&self, name: &str) {
        let host = &self.inner.collections_box;
        let mut child = host.first_child();
        while let Some(widget) = child {
            let next = widget.next_sibling();
            if widget.widget_name() == name {
                host.remove(&widget);
                return;
            }
            child = next;
        }
    }

    fn replace_hub_section(&self, name: &str, section: gtk::Widget) {
        let host = &self.inner.collections_box;
        let mut child = host.first_child();
        let mut prev: Option<gtk::Widget> = None;
        while let Some(widget) = child {
            let next = widget.next_sibling();
            if widget.widget_name() == name {
                host.insert_child_after(&section, prev.as_ref());
                host.remove(&widget);
                return;
            }
            prev = Some(widget);
            child = next;
        }
        host.append(&section);
    }

    fn collections_content_width(&self) -> i32 {
        const H_MARGIN: i32 = 32;
        if self.inner.collections_scroll.width() > 1 {
            return (self.inner.collections_scroll.width() - H_MARGIN).max(PERSON_TILE_PX);
        }
        if self.inner.window.width() > 1 {
            return (self.inner.window.width() - H_MARGIN).max(PERSON_TILE_PX);
        }
        (self.inner.window.default_width() - H_MARGIN).max(PERSON_TILE_PX)
    }

    fn memories_rail(&self, memories: &[gallery_ffi::MemoryStructure]) -> gtk::Widget {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        for memory in memories {
            store.append(&glib::BoxedAnyObject::new(memory.clone()));
        }
        let factory = gtk::SignalListItemFactory::new();
        let this = self.clone();
        factory.connect_setup(move |_, obj| {
            let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
            item.set_child(Some(&memory_hero_tile()));
        });
        factory.connect_bind({
            let this = this.clone();
            move |_, obj| {
                let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
                let Some(boxed) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
                    return;
                };
                let memory = boxed.borrow::<gallery_ffi::MemoryStructure>().clone();
                let Some(root) = item.child() else {
                    return;
                };
                let subtitle = memory
                    .subtitle
                    .clone()
                    .unwrap_or_else(|| photo_count_caption(memory.photo_ids.len()));
                let path = this
                    .inner
                    .session
                    .borrow()
                    .path_for_photo(&memory.cover_photo_id)
                    .map(ToOwned::to_owned);
                this.bind_cover(
                    &root,
                    &memory.title,
                    &subtitle,
                    path.as_deref()
                        .map(|path| (memory.cover_photo_id.as_str(), path)),
                );
            }
        });
        factory.connect_unbind({
            let this = this.clone();
            move |_, obj| {
                let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
                if let Some(child) = item.child() {
                    this.unbind_cover(&child);
                }
            }
        });
        let selection = gtk::NoSelection::new(Some(store));
        let list = gtk::ListView::new(Some(selection), Some(factory));
        list.add_css_class("gallery-memories");
        list.set_orientation(gtk::Orientation::Horizontal);
        list.set_single_click_activate(true);
        let this = self.clone();
        let memories = memories.to_vec();
        list.connect_activate(move |_, pos| {
            if let Some(memory) = memories.get(pos as usize) {
                this.push_memory(memory);
            }
        });
        cover_rail(list)
    }

    fn events_rail(&self, events: &[EventFolder]) -> gtk::Widget {
        let rail = gtk::Box::new(gtk::Orientation::Horizontal, EVENT_GAP_PX);
        rail.add_css_class("gallery-events-hub");
        rail.set_overflow(gtk::Overflow::Hidden);
        for event in events {
            rail.append(&self.event_hub_card(event));
        }
        clip_hub_rail(rail)
    }

    fn event_hub_card(&self, event: &EventFolder) -> gtk::Widget {
        let tile = cover_overlay_tile();
        tile.set_hexpand(false);
        tile.set_vexpand(false);
        tile.set_halign(gtk::Align::Start);
        tile.set_valign(gtk::Align::Start);
        self.bind_event_cover(&tile, event);
        let btn = gtk::Button::new();
        btn.set_child(Some(&tile));
        btn.add_css_class("flat");
        btn.add_css_class("event-card");
        btn.set_hexpand(false);
        btn.set_vexpand(false);
        btn.set_halign(gtk::Align::Start);
        btn.set_valign(gtk::Align::Start);
        btn.set_tooltip_text(Some(&format!(
            "{} · {}",
            event.name,
            photo_count_caption(event.photo_count as usize)
        )));
        let this = self.clone();
        let id = event.id.clone();
        let name = event.name.clone();
        btn.connect_clicked(move |_| this.push_event_folder(&id, &name));
        btn.upcast()
    }

    fn bind_event_cover(&self, root: &gtk::Widget, event: &EventFolder) {
        self.bind_cover(
            root,
            &event.name,
            &photo_count_caption(event.photo_count as usize),
            event
                .cover_photo_id
                .as_deref()
                .zip(event.cover_path.as_deref()),
        );
    }

    fn events_tiles(&self, events: &[EventFolder]) -> gtk::Widget {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        for event in events {
            store.append(&glib::BoxedAnyObject::new(event.clone()));
        }
        let factory = gtk::SignalListItemFactory::new();
        let this = self.clone();
        factory.connect_setup(move |_, obj| {
            let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
            item.set_child(Some(&cover_overlay_tile()));
        });
        factory.connect_bind({
            let this = this.clone();
            move |_, obj| {
                let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
                let Some(boxed) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
                    return;
                };
                let event = boxed.borrow::<EventFolder>().clone();
                let Some(root) = item.child() else {
                    return;
                };
                this.bind_event_cover(&root, &event);
            }
        });
        factory.connect_unbind({
            let this = this.clone();
            move |_, obj| {
                let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
                if let Some(child) = item.child() {
                    this.unbind_cover(&child);
                }
            }
        });
        let selection = gtk::NoSelection::new(Some(store));
        let grid = gtk::GridView::new(Some(selection), Some(factory));
        grid.add_css_class("gallery-events");
        grid.set_single_click_activate(true);
        let this = self.clone();
        let events = events.to_vec();
        grid.connect_activate(move |_, pos| {
            if let Some(event) = events.get(pos as usize) {
                this.push_event_folder(&event.id, &event.name);
            }
        });
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_child(Some(&grid));
        bind_cover_grid_columns(&scroll, &grid, EVENT_TILE_PX, EVENT_GAP_PX);
        scroll.upcast()
    }

    fn people_hub(&self, rows: &[gallery_ffi::GalleryTextRow], limit: usize) -> gtk::Widget {
        let preview: Vec<_> = rows.iter().take(limit).collect();
        let rail = gtk::Box::new(gtk::Orientation::Horizontal, PERSON_GAP_PX);
        rail.add_css_class("gallery-people-hub");
        rail.set_overflow(gtk::Overflow::Hidden);
        let mut column: Option<gtk::Box> = None;
        let mut covers = Vec::with_capacity(preview.len());
        for (index, row) in preview.iter().enumerate() {
            if index % 2 == 0 {
                let stack = gtk::Box::new(gtk::Orientation::Vertical, PERSON_GAP_PX);
                rail.append(&stack);
                column = Some(stack);
            }
            if let Some(stack) = &column {
                let card = self.person_hub_card(row);
                covers.push((card.clone(), row.id.clone(), row.title.clone()));
                stack.append(&card);
            }
        }
        self.schedule_person_covers(covers);
        clip_hub_rail(rail)
    }

    fn schedule_person_covers(&self, jobs: Vec<(gtk::Widget, String, String)>) {
        self.pump_person_covers(jobs, 0);
    }

    fn pump_person_covers(&self, jobs: Vec<(gtk::Widget, String, String)>, start: usize) {
        const CHUNK: usize = 4;
        if start >= jobs.len() {
            return;
        }
        let end = (start + CHUNK).min(jobs.len());
        for (card, id, title) in &jobs[start..end] {
            self.bind_person_cover(card, id, title);
        }
        if end < jobs.len() {
            let this = self.clone();
            glib::idle_add_local_once(move || this.pump_person_covers(jobs, end));
        }
    }

    fn person_hub_card(&self, row: &gallery_ffi::GalleryTextRow) -> gtk::Widget {
        let tile = person_card_tile();
        tile.set_hexpand(false);
        tile.set_vexpand(false);
        tile.set_halign(gtk::Align::Start);
        tile.set_valign(gtk::Align::Start);
        self.bind_cover(
            &tile,
            &row.title,
            row.trailing.as_deref().unwrap_or_default(),
            None,
        );
        let btn = gtk::Button::new();
        btn.set_child(Some(&tile));
        btn.add_css_class("flat");
        btn.add_css_class("person-card");
        btn.set_hexpand(false);
        btn.set_vexpand(false);
        btn.set_halign(gtk::Align::Start);
        btn.set_valign(gtk::Align::Start);
        btn.set_tooltip_text(Some(&format!(
            "{} · {}",
            row.title,
            row.trailing.as_deref().unwrap_or_default()
        )));
        let this = self.clone();
        let id = row.id.clone();
        let title = row.title.clone();
        btn.connect_clicked(move |_| this.push_person(&id, &title));
        let menu_id = row.id.clone();
        let menu_title = row.title.clone();
        self.attach_person_context_menu(&btn, move || Some((menu_id.clone(), menu_title.clone())));
        self.set_person_badges(&tile, &row.id, &row.title);
        btn.upcast()
    }

    fn bind_person_cover(&self, root: &gtk::Widget, tag_id: &str, title: &str) {
        let Some(id) = self.inner.session.borrow().person_cover_photo_id(tag_id) else {
            return;
        };
        let path = self
            .inner
            .session
            .borrow()
            .path_for_photo(&id)
            .map(ToOwned::to_owned);
        let Some(path) = path else {
            return;
        };
        let region = self.inner.session.borrow().named_cover_region(&id, title);
        self.bind_cover_photo(root, &id, &path, region.as_ref());
    }

    fn people_tiles(&self, rows: &[gallery_ffi::GalleryTextRow]) -> gtk::Widget {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        for row in rows {
            store.append(&glib::BoxedAnyObject::new(row.clone()));
        }
        let factory = gtk::SignalListItemFactory::new();
        let this = self.clone();
        factory.connect_setup({
            let this = this.clone();
            move |_, obj| {
                let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
                let tile = person_card_tile();
                this.attach_person_context_menu(&tile, {
                    let item = item.clone();
                    move || person_row_from_list_item(&item)
                });
                item.set_child(Some(&tile));
            }
        });
        factory.connect_bind({
            let this = this.clone();
            move |_, obj| {
                let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
                let Some(boxed) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
                    return;
                };
                let row = boxed.borrow::<gallery_ffi::GalleryTextRow>().clone();
                let Some(root) = item.child() else {
                    return;
                };
                let cover = this.inner.session.borrow().person_cover_photo_id(&row.id);
                let path = cover.as_ref().and_then(|id| {
                    this.inner
                        .session
                        .borrow()
                        .path_for_photo(id)
                        .map(ToOwned::to_owned)
                });
                let region = cover.as_ref().and_then(|id| {
                    this.inner
                        .session
                        .borrow()
                        .named_cover_region(id, &row.title)
                });
                set_cover_captions(
                    &root,
                    &row.title,
                    row.trailing.as_deref().unwrap_or_default(),
                );
                if let Some((id, path)) = cover.as_deref().zip(path.as_deref()) {
                    this.bind_cover_photo(&root, id, path, region.as_ref());
                }
                this.set_person_badges(&root, &row.id, &row.title);
            }
        });
        factory.connect_unbind({
            let this = this.clone();
            move |_, obj| {
                let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
                if let Some(child) = item.child() {
                    this.unbind_cover(&child);
                }
            }
        });
        let selection = gtk::NoSelection::new(Some(store));
        let grid = gtk::GridView::new(Some(selection), Some(factory));
        grid.add_css_class("gallery-people");
        grid.set_single_click_activate(true);
        let this = self.clone();
        let rows = rows.to_vec();
        grid.connect_activate(move |_, pos| {
            if let Some(row) = rows.get(pos as usize) {
                this.push_person(&row.id, &row.title);
            }
        });
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_child(Some(&grid));
        bind_cover_grid_columns(&scroll, &grid, PERSON_TILE_PX, PERSON_GAP_PX);
        scroll.upcast()
    }

    fn collection_rows(&self, section: &str, rows: &[gallery_ffi::GalleryTextRow]) -> gtk::Widget {
        let list = gtk::ListBox::new();
        list.add_css_class("boxed-list");
        list.set_selection_mode(gtk::SelectionMode::None);
        for row in rows {
            let nav = nav_row(&NavRowData {
                label: row.title.clone(),
                trailing: row.trailing.clone(),
            });
            let this = self.clone();
            let id = row.id.clone();
            let title = row.title.clone();
            let section = section.to_string();
            nav.connect_activated(move |_| {
                if section == "events" {
                    this.push_album(&id, &title);
                } else {
                    this.push_album(&id, &title);
                }
            });
            list.append(&nav);
        }
        list.upcast()
    }

    fn photo_grid_page(&self, host: ViewerHost) -> gtk::Widget {
        let cache = Rc::new(RefCell::new(PageCache::default()));
        let model_cell = Rc::new(RefCell::new(None::<ViewListModel>));
        let model = ViewListModel::empty(0);
        let grid = gtk::GridView::new(
            Some(gtk::NoSelection::new(Some(model.store()))),
            None::<gtk::SignalListItemFactory>,
        );
        model_cell.replace(Some(model));
        grid.add_css_class("gallery-photos");
        pack_photo_grid(&grid);
        grid.set_single_click_activate(true);
        self.queue_photo_projection(grid.clone(), cache.clone(), model_cell.clone());
        let factory = gtk::SignalListItemFactory::new();
        let thumbs = self.inner.thumbs.clone();
        let this = self.clone();
        factory.connect_setup(move |_, obj| {
            let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
            item.set_child(Some(&photo_tile()));
        });
        factory.connect_bind({
            let thumbs = thumbs.clone();
            let this = this.clone();
            let cache = cache.clone();
            let model_cell = model_cell.clone();
            let grid = grid.clone();
            move |_, obj| {
                let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
                let Some(picture) = picture_from_item(item) else {
                    return;
                };
                let Some(object) = item.item() else { return };
                let Some(view) = view_item_from_object(&object) else {
                    return;
                };
                let generation = model_cell
                    .borrow()
                    .as_ref()
                    .map(ViewListModel::generation)
                    .unwrap_or(0);
                let this_stale = this.clone();
                let cache_stale = cache.clone();
                let model_stale = model_cell.clone();
                let grid_stale = grid.clone();
                this.bind_tile_selection(item, &view.id);
                if let Some(is_video) = this.bind_grid_thumb(&thumbs, &picture, &view.id) {
                    set_tile_video_badge(item, is_video);
                    return;
                }
                if let Some(media) = this.fetch_media(&view, generation, &cache, move || {
                    this_stale.refill_photo_grid(grid_stale, cache_stale, model_stale);
                }) {
                    let scale = this.inner.window.scale_factor().max(1) as u32;
                    thumbs.bind_grid(&picture, &media.thumbnail_ref, &media.id, scale);
                    set_tile_video_badge(item, media.badge.as_deref() == Some("Video"));
                }
            }
        });
        factory.connect_unbind({
            let thumbs = thumbs.clone();
            move |_, obj| {
                let item = obj.downcast_ref::<gtk::ListItem>().expect("list item");
                if let Some(picture) = picture_from_item(item) {
                    thumbs.recycle(&picture);
                }
                set_tile_video_badge(item, false);
                set_tile_select_badge(item, false);
            }
        });
        grid.set_factory(Some(&factory));
        self.attach_photo_grid_menu(&grid, host);
        let this = self.clone();
        grid.connect_activate(move |_, pos| {
            let item = model_cell
                .borrow()
                .as_ref()
                .and_then(|model| model.item(pos));
            let Some(item) = item else {
                return;
            };
            this.activate_photo(&item.id, host);
        });
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_vexpand(true);
        scroll.set_child(Some(&grid));
        scroll.upcast()
    }

    fn push_people(&self) {
        let Ok(page_data) = self.inner.session.borrow().people_page() else {
            return;
        };
        if page_data.rows.is_empty() {
            return;
        }
        let body = self.people_tiles(&page_data.rows);
        let page = push_page("People", &body);
        page.set_tag(Some("people"));
        page.set_widget_name(route_id(GalleryScreen::People));
        self.inner.collections_nav.push(&page);
    }

    fn push_person(&self, tag_id: &str, title: &str) {
        let ids = self.inner.session.borrow().photo_ids_for_tag(tag_id);
        self.inner
            .session
            .borrow_mut()
            .set_current_intent(PhotoIntent::Ids {
                view_id: format!("person:{tag_id}"),
                ids,
                query: String::new(),
                tags: Vec::new(),
            });
        let body = self.photo_grid_page(ViewerHost::Collections);
        let overflow = overflow_button();
        overflow.set_tooltip_text(Some("Person Menu"));
        overflow.set_widget_name("person-overflow");
        overflow.set_popover(Some(&self.person_menu_popover(tag_id, title)));
        let page = page(
            title,
            &body,
            PageChrome {
                start: None,
                end: Some(overflow.upcast()),
                root: false,
            },
        );
        page.set_tag(Some(&format!("person:{tag_id}")));
        page.set_widget_name(route_id(GalleryScreen::Person));
        self.inner.collections_nav.push(&page);
    }

    fn push_events(&self) {
        let hub = self.inner.hub.borrow().clone();
        let events = hub
            .as_ref()
            .map(|hub| hub.events.clone())
            .unwrap_or_default();
        if !events.is_empty() {
            let grid = self.events_tiles(&events);
            let page = push_page("Events", &grid);
            page.set_widget_name(route_id(GalleryScreen::Events));
            self.inner.collections_nav.push(&page);
            return;
        }
        let Some(page_data) = hub.and_then(|hub| {
            hub.collections
                .into_iter()
                .find(|section| section.id == "events")
        }) else {
            return;
        };
        let list = self.collection_rows("events", &page_data.rows);
        let page = push_page("Events", &list);
        page.set_widget_name(route_id(GalleryScreen::Events));
        self.inner.collections_nav.push(&page);
    }

    fn push_event_folder(&self, id: &str, title: &str) {
        let ids = self.inner.session.borrow().folder_photo_ids(id);
        self.inner
            .session
            .borrow_mut()
            .set_current_intent(PhotoIntent::Ids {
                view_id: format!("event:{id}"),
                ids,
                query: String::new(),
                tags: Vec::new(),
            });
        let body = self.photo_grid_page(ViewerHost::Collections);
        let page = push_page(title, &body);
        page.set_widget_name(route_id(GalleryScreen::Events));
        self.inner.collections_nav.push(&page);
    }

    fn push_album(&self, tag_id: &str, title: &str) {
        let ids = self.inner.session.borrow().photo_ids_for_tag(tag_id);
        self.inner
            .session
            .borrow_mut()
            .set_current_intent(PhotoIntent::Ids {
                view_id: format!("album:{tag_id}"),
                ids,
                query: String::new(),
                tags: Vec::new(),
            });
        let body = self.photo_grid_page(ViewerHost::Collections);
        let page = push_page(title, &body);
        page.set_widget_name(route_id(GalleryScreen::Album));
        self.inner.collections_nav.push(&page);
    }

    fn push_memory(&self, memory: &gallery_ffi::MemoryStructure) {
        self.inner
            .session
            .borrow_mut()
            .set_current_intent(PhotoIntent::Ids {
                view_id: memory.id.clone(),
                ids: memory.photo_ids.clone(),
                query: String::new(),
                tags: Vec::new(),
            });
        let body = gtk::Box::new(gtk::Orientation::Vertical, 8);
        if let Some(subtitle) = &memory.subtitle {
            let label = gtk::Label::new(Some(subtitle));
            label.add_css_class("dim-label");
            label.set_xalign(0.0);
            label.set_margin_start(12);
            body.append(&label);
        }
        body.append(&self.photo_grid_page(ViewerHost::Collections));
        let page = push_page(&memory.title, &body);
        page.set_widget_name(route_id(GalleryScreen::Memory));
        self.inner.collections_nav.push(&page);
    }

    fn search_entry_focused(&self) -> bool {
        if self.inner.photos_search.has_focus() {
            return true;
        }
        self.inner
            .logs_search
            .borrow()
            .as_ref()
            .is_some_and(|entry| entry.has_focus())
    }

    fn visible_nav(&self) -> &adw::NavigationView {
        match self.inner.shell.stack.visible_child_name().as_deref() {
            Some("folders") => &self.inner.folders_nav,
            Some("collections") => &self.inner.collections_nav,
            _ => &self.inner.photos_nav,
        }
    }

    fn pop_visible_nav(&self) -> bool {
        self.visible_nav().pop()
    }

    fn viewer_nav(&self, host: ViewerHost) -> &adw::NavigationView {
        match host {
            ViewerHost::Photos => &self.inner.photos_nav,
            ViewerHost::Folders => &self.inner.folders_nav,
            ViewerHost::Collections => &self.inner.collections_nav,
        }
    }

    fn push_viewer(&self, id: &str, host: ViewerHost) {
        let _span = localcore_trace::span_always("viewer", "push_viewer")
            .extra("id", id)
            .extra("host", format!("{host:?}"));
        self.inner.last_viewer_host.set(host);
        let ids = self.inner.session.borrow().visible_photo_ids();
        let index = ids.iter().position(|item| item == id).unwrap_or(0);
        localcore_trace::event(
            "viewer",
            format!("resolved index={index} of {} ids", ids.len()),
        );
        let page = self.viewer_page(&ids, index);
        self.viewer_nav(host).push(&page);
    }

    fn first_photo_id(&self) -> Option<String> {
        self.inner
            .session
            .borrow()
            .visible_photo_ids()
            .first()
            .cloned()
    }

    fn first_folder_id(&self) -> Option<String> {
        self.inner
            .hub
            .borrow()
            .as_ref()
            .and_then(|hub| hub.folders.rows.first().map(|row| row.id.clone()))
    }

    fn viewer_page(&self, ids: &[String], index: usize) -> adw::NavigationPage {
        let _span = localcore_trace::span_always("viewer", "viewer_page")
            .extra("index", index)
            .extra("n", ids.len());
        let id = ids.get(index).cloned().unwrap_or_default();
        let path = self
            .inner
            .session
            .borrow()
            .path_for_photo(&id)
            .map(str::to_string);
        let title = self
            .inner
            .session
            .borrow()
            .host_for_photo(&id)
            .map(|host| host.filename.clone())
            .unwrap_or_else(|| "Photo".into());
        let is_video = viewer_is_video(
            self.inner.session.borrow().host_for_photo(&id),
            path.as_deref(),
        );
        let max_side = self.inner.thumbs.viewer_long_side(
            self.inner.window.width().max(1) as u32,
            self.inner.window.height().max(1) as u32,
            self.inner.window.scale_factor().max(1) as u32,
        );
        let (media, still) =
            viewer_media_surface(&self.inner.thumbs, &id, path.as_deref(), is_video, max_side);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&media));
        let menu = gio::Menu::new();
        menu.append(Some("Info"), Some("viewer.info"));
        menu.append(Some("Open With"), Some("viewer.open-with"));
        menu.append(Some("Show in Folder"), Some("viewer.show-folder"));
        menu.append(Some("Share"), Some("viewer.share"));
        menu.append(Some("Move"), Some("viewer.move"));
        menu.append(Some("Delete"), Some("viewer.delete"));
        let person_path = match self.inner.session.borrow().current_intent() {
            PhotoIntent::Ids { view_id, .. } => view_id.strip_prefix("person:").map(str::to_string),
            _ => None,
        };
        if person_path.is_some() {
            menu.append(Some("Set as Featured Image"), Some("viewer.feature"));
        }
        let overflow = overflow(&menu);
        overflow.add_css_class("osd");
        let chrome = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        chrome.set_halign(gtk::Align::End);
        chrome.set_valign(gtk::Align::Start);
        chrome.set_margin_top(8);
        chrome.set_margin_end(8);
        if viewer_shows_sequence_osd(ids.len(), self.inner.window.height()) {
            let prev = gtk::Button::from_icon_name("go-previous-symbolic");
            let next = gtk::Button::from_icon_name("go-next-symbolic");
            prev.add_css_class("circular");
            prev.add_css_class("osd");
            next.add_css_class("circular");
            next.add_css_class("osd");
            prev.set_tooltip_text(Some("Previous"));
            next.set_tooltip_text(Some("Next"));
            let this_p = self.clone();
            let ids_p = ids.to_vec();
            prev.connect_clicked(move |_| {
                if index > 0 {
                    let host = this_p.inner.last_viewer_host.get();
                    this_p.replace_viewer(&ids_p, index - 1, host);
                }
            });
            let this_n = self.clone();
            let ids_n = ids.to_vec();
            next.connect_clicked(move |_| {
                if index + 1 < ids_n.len() {
                    let host = this_n.inner.last_viewer_host.get();
                    this_n.replace_viewer(&ids_n, index + 1, host);
                }
            });
            chrome.append(&prev);
            chrome.append(&next);
        }
        chrome.append(&overflow);
        overlay.add_overlay(&chrome);
        let mut hideables = vec![chrome.upcast::<gtk::Widget>()];
        if viewer_shows_sequence_osd(ids.len(), self.inner.window.height()) {
            let strip = self.filmstrip(ids, index);
            overlay.add_overlay(&strip);
            hideables.push(strip);
        }
        attach_tap_to_hide(&media, hideables);
        if let Some(still) = still {
            attach_still_pinch_zoom(&still);
        }

        let group = gio::SimpleActionGroup::new();
        let info = gio::SimpleAction::new("info", None);
        let this = self.clone();
        let info_id = id.clone();
        info.connect_activate(move |_, _| {
            let host = this.inner.last_viewer_host.get();
            this.present_photo_info(&info_id, host);
        });
        group.add_action(&info);
        let open = gio::SimpleAction::new("open-with", None);
        let window = self.inner.window.clone();
        let open_path = path.clone();
        open.connect_activate(move |_, _| {
            if let Some(path) = &open_path {
                let file = gio::File::for_path(path);
                gtk::FileLauncher::new(Some(&file)).launch(
                    Some(&window),
                    gio::Cancellable::NONE,
                    |_| {},
                );
            }
        });
        group.add_action(&open);
        let show = gio::SimpleAction::new("show-folder", None);
        let window = self.inner.window.clone();
        show.connect_activate(move |_, _| {
            if let Some(path) = &path {
                let file = gio::File::for_path(path);
                gtk::FileLauncher::new(Some(&file)).open_containing_folder(
                    Some(&window),
                    gio::Cancellable::NONE,
                    |_| {},
                );
            }
        });
        group.add_action(&show);
        let share = gio::SimpleAction::new("share", None);
        let this = self.clone();
        let share_id = id.clone();
        share.connect_activate(move |_, _| this.share_viewer_item(&share_id));
        group.add_action(&share);
        let mover = gio::SimpleAction::new("move", None);
        let this = self.clone();
        let move_id = id.clone();
        mover.connect_activate(move |_, _| this.move_viewer_item(&move_id));
        group.add_action(&mover);
        let deleter = gio::SimpleAction::new("delete", None);
        let this = self.clone();
        let delete_id = id.clone();
        deleter.connect_activate(move |_, _| this.delete_viewer_item(&delete_id));
        group.add_action(&deleter);
        if let Some(person_path) = person_path {
            let feature = gio::SimpleAction::new("feature", None);
            let this = self.clone();
            let photo_id = id.clone();
            feature.connect_activate(move |_, _| {
                this.inner
                    .session
                    .borrow_mut()
                    .set_featured_photo(&person_path, &photo_id);
                this.refresh_people_ui();
                this.toast("Featured image updated");
            });
            group.add_action(&feature);
        }
        overflow.insert_action_group("viewer", Some(&group));

        if ids.len() > 1 {
            let swipe = gtk::GestureSwipe::new();
            let this = self.clone();
            let ids = ids.to_vec();
            swipe.connect_swipe(move |_, vx, _| {
                let host = this.inner.last_viewer_host.get();
                if vx < -180.0 && index + 1 < ids.len() {
                    this.replace_viewer(&ids, index + 1, host);
                } else if vx > 180.0 && index > 0 {
                    this.replace_viewer(&ids, index - 1, host);
                }
            });
            media.add_controller(swipe);
        }

        let wide = self.inner.window.width() > COMPACT_WIDTH;
        let content: gtk::Widget = if wide {
            let split = adw::OverlaySplitView::new();
            split.set_content(Some(&overlay));
            split.set_sidebar(Some(&self.photo_info_widget(&id)));
            split.set_sidebar_position(gtk::PackType::End);
            split.set_collapsed(true);
            split.upcast()
        } else {
            overlay.upcast()
        };
        let page = page(&title, &content, PageChrome::pushed());
        page.set_widget_name(route_id(GalleryScreen::Viewer));
        page
    }

    fn filmstrip(&self, ids: &[String], idx: usize) -> gtk::Widget {
        #[derive(Clone)]
        struct FilmstripItem {
            id: String,
            path: String,
            is_video: bool,
            current: bool,
        }
        let session = self.inner.session.borrow();
        let items: Vec<FilmstripItem> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let host = session.host_for_photo(id);
                FilmstripItem {
                    id: id.clone(),
                    path: host.map(|host| host.path.clone()).unwrap_or_default(),
                    is_video: viewer_is_video(host, host.map(|h| h.path.as_str())),
                    current: i == idx,
                }
            })
            .collect();
        drop(session);
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        for item in &items {
            store.append(&glib::BoxedAnyObject::new(item.clone()));
        }
        let factory = gtk::SignalListItemFactory::new();
        let thumbs = self.inner.thumbs.clone();
        let scale = self.inner.window.scale_factor().max(1) as u32;
        factory.connect_setup(move |_, obj| {
            let item = obj
                .downcast_ref::<gtk::ListItem>()
                .expect("factory item")
                .clone();
            let picture = gtk::Picture::new();
            picture.set_content_fit(gtk::ContentFit::Cover);
            picture.set_size_request(48, 48);
            item.set_child(Some(&picture));
        });
        factory.connect_bind({
            let thumbs = thumbs.clone();
            move |_, obj| {
                let item = obj
                    .downcast_ref::<gtk::ListItem>()
                    .expect("factory item")
                    .clone();
                let Some(boxed) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
                    return;
                };
                let Some(picture) = item.child().and_downcast::<gtk::Picture>() else {
                    return;
                };
                let cell = boxed.borrow::<FilmstripItem>().clone();
                if cell.current {
                    picture.add_css_class("suggested-action");
                } else {
                    picture.remove_css_class("suggested-action");
                }
                if cell.is_video || cell.path.is_empty() {
                    picture.set_paintable(Option::<&gtk::gdk::Paintable>::None);
                } else {
                    thumbs.bind_grid(&picture, &cell.path, &cell.id, scale);
                }
            }
        });
        let thumbs_unbind = thumbs.clone();
        factory.connect_unbind(move |_, obj| {
            let item = obj
                .downcast_ref::<gtk::ListItem>()
                .expect("factory item")
                .clone();
            if let Some(picture) = item.child().and_downcast::<gtk::Picture>() {
                thumbs_unbind.recycle(&picture);
            }
        });
        let sel = gtk::SingleSelection::new(Some(store));
        sel.set_selected(idx as u32);
        let list = gtk::ListView::new(Some(sel), Some(factory));
        list.set_orientation(gtk::Orientation::Horizontal);
        list.set_single_click_activate(true);
        list.add_css_class("osd");
        let this = self.clone();
        let ids = ids.to_vec();
        list.connect_activate(move |_, pos| {
            let host = this.inner.last_viewer_host.get();
            this.replace_viewer(&ids, pos as usize, host);
        });
        let list_scroll = list.clone();
        let target = idx as u32;
        glib::idle_add_local_once(move || {
            list_scroll.scroll_to(target, gtk::ListScrollFlags::NONE, None::<gtk::ScrollInfo>);
        });
        let sw = gtk::ScrolledWindow::new();
        sw.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Never);
        sw.set_propagate_natural_height(true);
        sw.set_halign(gtk::Align::Fill);
        sw.set_valign(gtk::Align::End);
        sw.add_css_class("osd");
        sw.set_child(Some(&list));
        sw.upcast()
    }

    fn replace_viewer(&self, ids: &[String], index: usize, host: ViewerHost) {
        self.inner.last_viewer_host.set(host);
        self.viewer_nav(host).pop();
        let page = self.viewer_page(ids, index);
        self.viewer_nav(host).push(&page);
    }

    fn present_photo_info(&self, id: &str, host: ViewerHost) {
        let body = self.photo_info_widget(id);
        if self.inner.window.width() <= COMPACT_WIDTH {
            let dialog = sheet("Photo", &body, SheetSize::Form);
            dialog.set_widget_name(route_id(GalleryScreen::PhotoInfo));
            dialog.present(Some(&self.inner.window));
        } else {
            let page = push_page("Photo", &body);
            page.set_widget_name(route_id(GalleryScreen::PhotoInfo));
            self.viewer_nav(host).push(&page);
        }
    }

    fn photo_info_widget(&self, id: &str) -> gtk::Widget {
        let _span = localcore_trace::span_always("viewer", "photo_info").extra("id", id);
        let session = self.inner.session.borrow();
        let path = session.path_for_photo(id).map(str::to_string);
        let host = session.host_for_photo(id).cloned();
        let regions = session.face_regions_for(id);
        drop(session);
        localcore_trace::event(
            "viewer",
            format!("photo_info path={}", path.as_deref().unwrap_or("<missing>")),
        );
        let meta = path
            .as_ref()
            .map(|path| gallery_ffi::read_image_metadata(path.clone()));
        let group = gtk::ListBox::new();
        group.add_css_class("boxed-list");
        group.set_selection_mode(gtk::SelectionMode::None);
        let date = meta
            .as_ref()
            .and_then(|meta| meta.capture_wall_clock.as_ref())
            .map(|clock| {
                format!(
                    "{:04}-{:02}-{:02}  {:02}:{:02}",
                    clock.year, clock.month, clock.day, clock.hour, clock.minute
                )
            })
            .unwrap_or_else(|| "—".into());
        group.append(&field_row(&FieldRowData {
            label: "Date".into(),
            value: date,
            editable: false,
        }));
        group.append(&field_row(&FieldRowData {
            label: "File".into(),
            value: host
                .as_ref()
                .map(|host| host.filename.clone())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "—".into()),
            editable: false,
        }));
        if host.as_ref().is_some_and(|host| host.is_video) {
            group.append(&field_row(&FieldRowData {
                label: "Type".into(),
                value: "Video".into(),
                editable: false,
            }));
        }
        group.append(&field_row(&FieldRowData {
            label: "Camera".into(),
            value: camera_label(
                meta.as_ref().and_then(|meta| meta.make.as_deref()),
                meta.as_ref().and_then(|meta| meta.model.as_deref()),
            ),
            editable: false,
        }));
        if let (Some(lat), Some(lon)) = (
            meta.as_ref().and_then(|meta| meta.gps_latitude),
            meta.as_ref().and_then(|meta| meta.gps_longitude),
        ) {
            group.append(&field_row(&FieldRowData {
                label: "GPS".into(),
                value: format!("{lat:.5}, {lon:.5}"),
                editable: false,
            }));
        }
        if let Some(country) = meta
            .as_ref()
            .and_then(|meta| meta.country_code.as_deref())
            .filter(|code| !code.is_empty())
        {
            group.append(&field_row(&FieldRowData {
                label: "Country".into(),
                value: country.to_string(),
                editable: false,
            }));
        }
        let location = meta
            .as_ref()
            .and_then(|meta| {
                meta.country_code.clone().or_else(|| {
                    match (meta.gps_latitude, meta.gps_longitude) {
                        (Some(lat), Some(lon)) => Some(format!("{lat:.5}, {lon:.5}")),
                        _ => None,
                    }
                })
            })
            .unwrap_or_else(|| "—".into());
        group.append(&field_row(&FieldRowData {
            label: "Location".into(),
            value: location,
            editable: false,
        }));
        let named_faces: Vec<_> = regions
            .iter()
            .filter_map(|region| region.name.as_deref())
            .collect();
        let faces = if !named_faces.is_empty() {
            named_faces.join(", ")
        } else if !regions.is_empty() {
            format!("{} unnamed", regions.len())
        } else {
            "—".into()
        };
        group.append(&field_row(&FieldRowData {
            label: "Faces".into(),
            value: faces,
            editable: false,
        }));
        let tags = meta
            .as_ref()
            .map(|meta| meta.hierarchical_tags.clone())
            .unwrap_or_default();
        let people: Vec<_> = tags
            .iter()
            .filter(|tag| tag.namespace.as_deref() == Some("People"))
            .cloned()
            .collect();
        let other: Vec<_> = tags
            .iter()
            .filter(|tag| tag.namespace.as_deref() != Some("People"))
            .cloned()
            .collect();
        let tags_value = if other.is_empty() {
            "—".into()
        } else {
            other
                .iter()
                .map(|tag| tag.display_name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let people_value = if people.is_empty() {
            "—".into()
        } else {
            people
                .iter()
                .map(|tag| tag.display_name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        group.append(&field_row(&FieldRowData {
            label: "Tags".into(),
            value: tags_value,
            editable: false,
        }));
        group.append(&field_row(&FieldRowData {
            label: "People".into(),
            value: people_value,
            editable: false,
        }));
        let sidecar = match path.as_deref() {
            Some(path) => match gallery_meta::sidecar_exists(&gallery_vfs::StdVfs::new(), path) {
                Ok(true) => gallery_meta::sidecar_path(path),
                Ok(false) => "No sidecar on disk".into(),
                Err(_) => "Sidecar unavailable".into(),
            },
            None => "Sidecar unavailable".into(),
        };
        group.append(&field_row(&FieldRowData {
            label: "Sidecar".into(),
            value: sidecar,
            editable: false,
        }));
        let box_ = gtk::Box::new(gtk::Orientation::Vertical, 12);
        box_.set_margin_top(18);
        box_.set_margin_bottom(18);
        box_.set_margin_start(18);
        box_.set_margin_end(18);
        box_.append(&group);
        box_.upcast()
    }

    fn update_photo_columns(&self, _width: i32) {
        pack_photo_grid(&self.inner.photos_grid);
    }

    fn live_people_page(&self) -> TextPage {
        let (page, featured, hidden, me) = {
            let session = self.inner.session.borrow();
            let state = session.person_state();
            (
                session.people_rail_page(),
                state.featured.len(),
                state.hidden.len(),
                state.me.clone(),
            )
        };
        localcore_trace::event(
            "people",
            format!(
                "live people n={} featured={featured} hidden={hidden} me={}",
                page.rows.len(),
                if me.is_empty() { "-" } else { me.as_str() }
            ),
        );
        if let Some(hub) = self.inner.hub.borrow_mut().as_mut() {
            hub.people = page.clone();
        }
        page
    }

    fn people_see_all_needed(&self, rail_len: usize, limit: usize) -> bool {
        match self.inner.session.borrow().people_page() {
            Ok(page) => page.rows.len() > rail_len.min(limit),
            Err(_) => rail_len > limit,
        }
    }

    fn refresh_people_ui(&self) {
        let people = self.live_people_page();
        let limit = self.inner.hub_people_limit.get();
        let see_all_needed = self.people_see_all_needed(people.rows.len(), limit);
        if people.rows.is_empty() && !see_all_needed {
            self.remove_hub_section("hub-people");
        } else {
            let see_all = see_all_needed.then(|| {
                let this = self.clone();
                see_all_button(move || this.push_people())
            });
            self.put_hub_section(
                "hub-people",
                hub_section("People", see_all, self.people_hub(&people.rows, limit)),
            );
        }
        self.sync_people_pages();
    }

    fn refresh_people_and_memories(&self) {
        self.refresh_people_ui();
        self.start_memories();
    }

    fn sync_people_pages(&self) {
        let people_name = route_id(GalleryScreen::People);
        let person_name = route_id(GalleryScreen::Person);
        let nav = &self.inner.collections_nav;
        if let Some(visible) = nav.visible_page() {
            if visible.widget_name() == person_name {
                if let Some(tag) = visible.tag() {
                    if let Some(path) = tag.strip_prefix("person:") {
                        let title = visible.title();
                        if self
                            .inner
                            .session
                            .borrow()
                            .person_marks(path, &title)
                            .hidden
                        {
                            nav.pop();
                        } else {
                            self.refresh_person_overflow(&visible, path, &title);
                        }
                    }
                }
            }
        }
        let stack = nav.navigation_stack();
        for index in 0..stack.n_items() {
            let Some(page) = stack.item(index).and_downcast::<adw::NavigationPage>() else {
                continue;
            };
            if page.widget_name() != people_name {
                continue;
            }
            let Ok(data) = self.inner.session.borrow().people_page() else {
                continue;
            };
            if let Some(toolbar) = page.child().and_downcast::<adw::ToolbarView>() {
                toolbar.set_content(Some(&self.people_tiles(&data.rows)));
            }
        }
    }

    fn refresh_person_overflow(&self, page: &adw::NavigationPage, path: &str, title: &str) {
        let Some(overflow) = find_named_widget::<gtk::MenuButton>(page, "person-overflow") else {
            return;
        };
        overflow.set_popover(Some(&self.person_menu_popover(path, title)));
    }

    fn set_person_badges(&self, root: &gtk::Widget, path: &str, title: &str) {
        let marks = self.inner.session.borrow().person_marks(path, title);
        set_named_visible(root, "person-link", marks.linked);
        set_named_visible(root, "person-me", marks.me);
        set_named_visible(root, "person-star", marks.featured);
        if marks.hidden {
            root.add_css_class("person-hidden");
        } else {
            root.remove_css_class("person-hidden");
        }
    }

    fn person_menu_popover(&self, path: &str, title: &str) -> gtk::Popover {
        let popover = gtk::Popover::new();
        popover.add_css_class("menu");
        popover.add_css_class("person-menu");
        popover.set_has_arrow(false);
        popover.set_halign(gtk::Align::Start);
        popover.set_child(Some(&self.person_menu_body(path, title, popover.clone())));
        popover
    }

    fn person_menu_body(&self, path: &str, title: &str, popover: gtk::Popover) -> gtk::Widget {
        let session = self.inner.session.borrow();
        let path = session.resolve_person_path(path);
        let marks = session.person_marks(&path, title);
        drop(session);
        localcore_trace::event(
            "people",
            format!(
                "person menu open path={path} title={title} featured={} me={} hidden={} link={}",
                marks.featured, marks.me, marks.hidden, marks.link_label
            ),
        );

        let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
        list.add_css_class("person-menu-list");

        let this = self.clone();
        let feature_path = path.clone();
        let feature_popover = popover.clone();
        list.append(&person_menu_item(
            if marks.featured {
                "Unfeature"
            } else {
                "Feature"
            },
            move || {
                localcore_trace::event("people", format!("menu feature click path={feature_path}"));
                let this = this.clone();
                let feature_path = feature_path.clone();
                after_person_menu(&feature_popover, move || {
                    if !this
                        .inner
                        .session
                        .borrow_mut()
                        .toggle_feature_person(&feature_path)
                    {
                        this.toast("Could not update favorite");
                        return;
                    }
                    this.refresh_people_ui();
                });
            },
        ));

        let this = self.clone();
        let me_path = path.clone();
        let is_me = marks.me;
        let me_popover = popover.clone();
        list.append(&person_menu_item(
            if is_me { "Unmark as Me" } else { "Mark as Me" },
            move || {
                localcore_trace::event(
                    "people",
                    format!("menu me click path={me_path} currently_me={is_me}"),
                );
                let this = this.clone();
                let me_path = me_path.clone();
                after_person_menu(&me_popover, move || {
                    let ok = this.inner.session.borrow_mut().set_me_person(if is_me {
                        None
                    } else {
                        Some(me_path.as_str())
                    });
                    if !ok {
                        this.toast("Could not update Me");
                        return;
                    }
                    this.refresh_people_and_memories();
                });
            },
        ));

        let this = self.clone();
        let hide_path = path.clone();
        let hide_popover = popover.clone();
        let hidden = marks.hidden;
        list.append(&person_menu_item(
            if hidden { "Unhide" } else { "Hide" },
            move || {
                localcore_trace::event(
                    "people",
                    format!("menu hide click path={hide_path} hidden={hidden}"),
                );
                let this = this.clone();
                let hide_path = hide_path.clone();
                after_person_menu(&hide_popover, move || {
                    let ok = if hidden {
                        this.inner.session.borrow_mut().unhide_person(&hide_path)
                    } else {
                        this.inner.session.borrow_mut().hide_person(&hide_path)
                    };
                    if !ok {
                        this.toast(if hidden {
                            "Could not unhide this person"
                        } else {
                            "Could not hide this person"
                        });
                        return;
                    }
                    this.refresh_people_and_memories();
                });
            },
        ));

        list.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        let this = self.clone();
        let link_path = path;
        let link_title = title.to_string();
        let link_popover = popover;
        list.append(&person_menu_item(&marks.link_label, move || {
            localcore_trace::event(
                "people",
                format!("menu link click path={link_path} title={link_title}"),
            );
            let this = this.clone();
            let link_path = link_path.clone();
            let link_title = link_title.clone();
            after_person_menu(&link_popover, move || {
                this.present_link_contact(&link_path, &link_title);
            });
        }));

        list.upcast()
    }

    fn attach_person_context_menu<F>(&self, widget: &impl IsA<gtk::Widget>, lookup: F)
    where
        F: Fn() -> Option<(String, String)> + Clone + 'static,
    {
        let widget = widget.clone().upcast::<gtk::Widget>();

        let click = gtk::GestureClick::new();
        click.set_button(0);
        let this = self.clone();
        let lookup_click = lookup.clone();
        let click_widget = widget.clone();
        click.connect_pressed(move |gesture, n_press, x, y| {
            if n_press != 1 {
                return;
            }
            let Some(event) = gesture.current_event() else {
                return;
            };
            if !event.triggers_context_menu() {
                return;
            }
            gesture.set_state(gtk::EventSequenceState::Claimed);
            let Some((path, title)) = lookup_click() else {
                return;
            };
            this.popup_person_menu(&click_widget, x, y, &path, &title);
        });
        widget.add_controller(click);

        let long = gtk::GestureLongPress::new();
        let this = self.clone();
        let lookup_long = lookup.clone();
        let long_widget = widget.clone();
        long.connect_pressed(move |gesture, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            let Some((path, title)) = lookup_long() else {
                return;
            };
            this.popup_person_menu(&long_widget, x, y, &path, &title);
        });
        widget.add_controller(long);

        let keys = gtk::EventControllerKey::new();
        let this = self.clone();
        let key_widget = widget.clone();
        keys.connect_key_pressed(move |_, keyval, _, modifier| {
            let menu_key = keyval == gtk::gdk::Key::Menu
                || (keyval == gtk::gdk::Key::F10
                    && modifier.contains(gtk::gdk::ModifierType::SHIFT_MASK));
            if !menu_key {
                return glib::Propagation::Proceed;
            }
            let Some((path, title)) = lookup() else {
                return glib::Propagation::Proceed;
            };
            this.popup_person_menu(
                &key_widget,
                f64::from(key_widget.width()) / 2.0,
                f64::from(key_widget.height()) / 2.0,
                &path,
                &title,
            );
            glib::Propagation::Stop
        });
        widget.add_controller(keys);
    }

    fn popup_person_menu(
        &self,
        parent: &impl IsA<gtk::Widget>,
        x: f64,
        y: f64,
        path: &str,
        title: &str,
    ) {
        let parent = parent.as_ref().clone();
        let popover = self.person_menu_popover(path, title);
        popover.set_parent(&parent);
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
            x.round() as i32,
            y.round() as i32,
            1,
            1,
        )));
        popover.connect_closed(move |popover| {
            localcore_trace::event("people", "person context menu closed");
            popover.unparent();
        });
        popover.popup();
    }

    fn present_link_contact(&self, path: &str, title: &str) {
        let session = self.inner.session.borrow();
        let contacts = session.contacts().to_vec();
        let folder = session.config().contacts_root.clone();
        drop(session);
        localcore_trace::event(
            "people",
            format!(
                "present_link_contact path={path} title={title} contacts={} folder={}",
                contacts.len(),
                folder
                    .as_deref()
                    .map(Path::display)
                    .map(|path| path.to_string())
                    .unwrap_or_else(|| "-".into())
            ),
        );

        let this = self.clone();
        let pick_path = path.to_string();
        let picker =
            crate::link_contact::sheet_body(contacts, folder.as_deref(), move |id, dialog| {
                this.apply_contact_link(&pick_path, Some(Some(id.to_string())), dialog);
            });
        let dialog = sheet(&format!("Link {title}"), &picker.body, SheetSize::Picker);

        let this = self.clone();
        let reset_path = path.to_string();
        let reset_dialog = dialog.clone();
        picker.reset.connect_activated(move |_| {
            this.apply_contact_link(&reset_path, None, &reset_dialog);
        });
        let this = self.clone();
        let disable_path = path.to_string();
        let disable_dialog = dialog.clone();
        picker.disable.connect_activated(move |_| {
            this.apply_contact_link(&disable_path, Some(None), &disable_dialog);
        });
        if let Some(choose) = picker.choose {
            let this = self.clone();
            let choose_dialog = dialog.clone();
            choose.connect_activated(move |_| {
                localcore_trace::event("people", "link sheet choose contacts folder");
                choose_dialog.close();
                this.pick_contacts_folder();
            });
        }
        dialog.present(Some(&self.inner.window));
        localcore_trace::event("people", format!("link sheet presented path={path}"));
    }

    fn apply_contact_link(
        &self,
        path: &str,
        contact_id: Option<Option<String>>,
        dialog: &adw::Dialog,
    ) {
        localcore_trace::event(
            "people",
            format!(
                "link sheet apply path={path} decision={}",
                match &contact_id {
                    None => "reset".into(),
                    Some(None) => "disabled".into(),
                    Some(Some(id)) => format!("manual:{id}"),
                }
            ),
        );
        if !self
            .inner
            .session
            .borrow_mut()
            .set_contact_link(path, contact_id)
        {
            self.toast("Could not update contact link");
            return;
        }
        self.refresh_people_and_memories();
        dialog.close();
    }

    fn pick_contacts_folder(&self) {
        let dialog = gtk::FileDialog::builder()
            .title("Choose Contacts Folder")
            .modal(true)
            .build();
        let window = self.inner.window.clone();
        let this = self.clone();
        dialog.select_folder(
            Some(&window),
            gio::Cancellable::NONE,
            move |result| match result {
                Ok(file) => {
                    if let Some(path) = file.path() {
                        if let Err(error) = this
                            .inner
                            .session
                            .borrow_mut()
                            .set_contacts_folder(Some(path))
                        {
                            this.toast(&error.to_string());
                        } else {
                            this.toast("Contacts folder updated");
                        }
                    }
                }
                Err(err) => {
                    let msg = err.to_string();
                    if !msg.contains("Dismissed") && !msg.contains("dismissed") {
                        this.toast(&msg);
                    }
                }
            },
        );
    }

    fn present_about(&self) {
        about_dialog(crate::APP_ID, APP_TITLE, env!("CARGO_PKG_VERSION"))
            .present(Some(&self.inner.window));
    }

    fn present_conflicts(&self) {
        let result = self.inner.session.borrow().conflict_rows();
        let rows = match result {
            Ok(rows) => rows,
            Err(error) => {
                self.inner.session.borrow_mut().record(
                    LogLevel::Error,
                    "conflict",
                    "Could not load Sync Conflicts",
                );
                self.toast(&error.to_string());
                return;
            }
        };
        self.inner.session.borrow_mut().record(
            LogLevel::Info,
            "conflict",
            format!("Reviewing {} sync conflict groups", rows.len()),
        );
        let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
        column.set_margin_top(12);
        column.set_margin_bottom(12);
        column.set_margin_start(12);
        column.set_margin_end(12);
        let intro = gtk::Label::new(Some(
            "Two devices wrote the same photo. Sidecar keywords merge automatically. Image copies are keep-one — pick the file that stays. Losing copies are deleted only after you confirm.",
        ));
        intro.set_wrap(true);
        intro.set_xalign(0.0);
        intro.add_css_class("dim-label");
        column.append(&intro);
        if rows.is_empty() {
            column.append(&status_row(&StatusRowData {
                message: "No Sync Conflicts".into(),
                severity: StatusSeverity::Info,
            }));
        }
        let dialog = sheet(
            "Sync Conflicts",
            &gtk::ScrolledWindow::builder().child(&column).build(),
            SheetSize::Picker,
        );
        dialog.set_widget_name(route_id(GalleryScreen::SyncConflictGroup));
        for row in rows {
            column.append(&text_row(&TextRowData {
                title: row.title.clone(),
                subtitle: Some(row.subtitle.clone()),
                trailing: Some(row.trailing.clone()),
                leading: None,
            }));
            let label = if row.kind == ConflictKind::Image {
                "Choose Copy…"
            } else {
                "Review Diff…"
            };
            let review = action_row(&ActionRowData {
                label: label.into(),
                role: ActionRole::Normal,
                enabled: true,
            });
            let this = self.clone();
            let host = dialog.clone();
            let group_id = row.id.clone();
            if row.kind == ConflictKind::Image {
                review.connect_clicked(move |_| {
                    this.present_image_keep(&group_id, Some(&host));
                });
            } else {
                review.connect_clicked(move |_| {
                    this.present_xmp_diff(&group_id, Some(&host));
                });
            }
            column.append(&review);
        }
        dialog.present(Some(&self.inner.window));
    }

    fn present_xmp_diff(&self, group_id: &str, parent: Option<&adw::Dialog>) {
        let preview = match self.inner.session.borrow().conflict_preview(group_id) {
            Ok(preview) => preview,
            Err(error) => {
                self.inner.session.borrow_mut().record(
                    LogLevel::Error,
                    "conflict",
                    "Could not load conflict diff",
                );
                self.toast(&error.to_string());
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
        let group = adw::PreferencesGroup::new();
        group.set_title("Merged Sidecar");
        for field in &preview.merged_fields {
            group.add(&field_row(&FieldRowData {
                label: field.label.clone(),
                value: field.value.clone(),
                editable: false,
            }));
        }
        let page = adw::PreferencesPage::new();
        page.add(&group);
        column.append(&page);
        let confirm = action_row(&ActionRowData {
            label: "Keep This Version".into(),
            role: ActionRole::Normal,
            enabled: preview.kind != MergeKind::Choice,
        });
        column.append(&confirm);
        let dialog = sheet("Sync Conflict", &column, SheetSize::Picker);
        dialog.set_widget_name(route_id(GalleryScreen::SyncConflictGroup));
        let this = self.clone();
        let group_id = preview.id.clone();
        let parent = parent.cloned();
        let host = dialog.clone();
        confirm.connect_clicked(move |_| {
            this.confirm_resolve_xmp(group_id.clone(), parent.as_ref(), &host);
        });
        dialog.present(Some(&self.inner.window));
    }

    fn present_image_keep(&self, group_id: &str, parent: Option<&adw::Dialog>) {
        let preview = match self.inner.session.borrow().image_preview(group_id) {
            Ok(preview) => preview,
            Err(error) => {
                self.inner.session.borrow_mut().record(
                    LogLevel::Error,
                    "conflict",
                    "Could not load image copies",
                );
                self.toast(&error.to_string());
                return;
            }
        };
        let selected = Rc::new(RefCell::new(preview.copies.first().cloned()));
        let checks = Rc::new(RefCell::new(Vec::<(String, gtk::Image)>::new()));
        let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
        column.set_margin_top(12);
        column.set_margin_bottom(12);
        column.set_margin_start(12);
        column.set_margin_end(12);
        let intro = gtk::Label::new(Some(
            "Confirming deletes the copies you do not keep. Image bytes are not rewritten.",
        ));
        intro.set_wrap(true);
        intro.set_xalign(0.0);
        intro.add_css_class("dim-label");
        column.append(&intro);
        for (index, name) in preview.copies.iter().enumerate() {
            let row = text_row(&TextRowData {
                title: name.clone(),
                subtitle: None,
                trailing: None,
                leading: None,
            });
            row.set_activatable(true);
            let check = gtk::Image::from_icon_name("object-select-symbolic");
            check.set_visible(index == 0);
            row.add_suffix(&check);
            checks.borrow_mut().push((name.clone(), check));
            let selected = selected.clone();
            let checks = checks.clone();
            let name = name.clone();
            row.connect_activated(move |_| {
                selected.replace(Some(name.clone()));
                for (choice, indicator) in checks.borrow().iter() {
                    indicator.set_visible(choice == &name);
                }
            });
            column.append(&row);
        }
        let confirm = action_row(&ActionRowData {
            label: "Keep This Copy".into(),
            role: ActionRole::Normal,
            enabled: !preview.copies.is_empty(),
        });
        column.append(&confirm);
        let dialog = sheet("Sync Conflict", &column, SheetSize::Picker);
        dialog.set_widget_name(route_id(GalleryScreen::SyncConflictGroup));
        let this = self.clone();
        let group_id = preview.id.clone();
        let parent = parent.cloned();
        let host = dialog.clone();
        confirm.connect_clicked(move |_| {
            let Some(surviving) = selected.borrow().clone() else {
                this.toast("Choose one copy");
                return;
            };
            this.confirm_keep_image(group_id.clone(), surviving, parent.as_ref(), &host);
        });
        dialog.present(Some(&self.inner.window));
    }

    fn confirm_resolve_xmp(
        &self,
        group_id: String,
        parent: Option<&adw::Dialog>,
        host: &adw::Dialog,
    ) {
        let dialog = confirm_dialog(&ConfirmData {
            question: "Merge this sidecar and remove losing copies?".into(),
            destructive_label: "Resolve".into(),
        });
        let this = self.clone();
        let parent = parent.cloned();
        let host = host.clone();
        dialog.connect_response(None, move |_, response| {
            if response != "confirm" {
                return;
            }
            match this.inner.session.borrow().resolve_group(&group_id) {
                Ok(()) => {
                    this.inner.session.borrow_mut().record(
                        LogLevel::Info,
                        "conflict",
                        "Resolved XMP sync conflict",
                    );
                    host.close();
                    this.finish_conflict_review(parent.as_ref());
                }
                Err(error) => {
                    this.inner.session.borrow_mut().record(
                        LogLevel::Error,
                        "conflict",
                        "Could not resolve XMP sync conflict",
                    );
                    this.toast(&error.to_string());
                }
            }
        });
        dialog.present(Some(&self.inner.window));
    }

    fn confirm_keep_image(
        &self,
        group_id: String,
        surviving: String,
        parent: Option<&adw::Dialog>,
        host: &adw::Dialog,
    ) {
        let dialog = confirm_dialog(&ConfirmData {
            question: "Keep this copy and remove the others?".into(),
            destructive_label: "Keep".into(),
        });
        let this = self.clone();
        let parent = parent.cloned();
        let host = host.clone();
        dialog.connect_response(None, move |_, response| {
            if response != "confirm" {
                return;
            }
            match this
                .inner
                .session
                .borrow()
                .keep_image_copy(&group_id, &surviving)
            {
                Ok(()) => {
                    this.inner.session.borrow_mut().record(
                        LogLevel::Info,
                        "conflict",
                        "Kept one image copy",
                    );
                    host.close();
                    this.finish_conflict_review(parent.as_ref());
                    this.reload_folder();
                }
                Err(error) => {
                    this.inner.session.borrow_mut().record(
                        LogLevel::Error,
                        "conflict",
                        "Could not keep image copy",
                    );
                    this.toast(&error.to_string());
                }
            }
        });
        dialog.present(Some(&self.inner.window));
    }

    fn finish_conflict_review(&self, parent: Option<&adw::Dialog>) {
        if let Some(parent) = parent {
            parent.close();
        }
        match self.inner.session.borrow().conflict_rows() {
            Ok(rows) if rows.is_empty() => {}
            Ok(_) => self.present_conflicts(),
            Err(error) => self.toast(&error.to_string()),
        }
    }

    fn present_settings(&self) {
        let dialog = preferences_dialog("Settings", settings_screen(self.settings_spec()));
        dialog.set_widget_name(route_id(GalleryScreen::Settings));
        self.inner.settings_dialog.replace(Some(dialog.clone()));
        dialog.present(Some(&self.inner.window));
    }

    fn present_logs(&self) {
        self.inner.session.borrow_mut().record(
            LogLevel::Info,
            "diagnostics",
            "Opened app diagnostics",
        );

        let ui = list_screen(&ListScreen {
            search: true,
            filter: Some(shell_kit_gtk::Filter::Scope(vec![
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
        self.inner.logs_search.replace(Some(search.clone()));
        let level = match ui.filter.clone().expect("logs filter") {
            shell_kit_gtk::FilterControl::Scope(group) => group,
            shell_kit_gtk::FilterControl::Choice(_) => {
                panic!("logs filter is a fixed Scope, not Choice")
            }
        };
        let clear = action_row(&ActionRowData {
            label: "Clear".into(),
            role: ActionRole::Destructive,
            enabled: true,
        });
        ui.controls.append(&clear);

        let query = Rc::new(RefCell::new(String::new()));
        let selected_level = Rc::new(RefCell::new(None::<LogLevel>));
        self.refill_logs(&ui, "", None);

        let searched = self.clone();
        let searched_ui = ui.clone();
        let searched_query = query.clone();
        let searched_level = selected_level.clone();
        search.connect_search_changed(move |entry| {
            searched_query.replace(entry.text().to_string());
            searched.refill_logs(
                &searched_ui,
                &searched_query.borrow(),
                *searched_level.borrow(),
            );
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
            *filtered_level.borrow_mut() = selected;
            filtered.refill_logs(&filtered_ui, &filtered_query.borrow(), selected);
        });

        let cleared = self.clone();
        let cleared_ui = ui.clone();
        let cleared_query = query;
        let cleared_level = selected_level;
        clear.connect_clicked(move |_| {
            cleared.inner.session.borrow_mut().diagnostics_mut().clear();
            cleared.refill_logs(
                &cleared_ui,
                &cleared_query.borrow(),
                *cleared_level.borrow(),
            );
        });

        self.push_settings_page("Logs", route_id(GalleryScreen::Logs), &ui.root);
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
        clear_list(list);
        let session = self.inner.session.borrow();
        let entries = session.diagnostics().filtered(query, level);
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

    fn settings_spec(&self) -> SettingsScreen {
        let session = self.inner.session.borrow();
        let folder = session.folder().map(|path| path.display().to_string());
        let photo_count = session.photo_count();
        let people_count = session.tag_namespace_count("people");
        let places_count = session.tag_namespace_count("places");
        let objects_count = session.tag_namespace_count("objects");
        let scenes_count = session.tag_namespace_count("scenes");
        let scan = session.settings_progress();
        let contacts_path = session
            .config()
            .contacts_root
            .as_ref()
            .map(|path| path.display().to_string());
        let hidden = session.person_state().hidden.clone();
        let conflict_count = session.conflict_rows().map(|rows| rows.len()).unwrap_or(0);
        drop(session);
        let pack = localgallery::installed_pack();
        let pack_summary = match &pack {
            Some(pack) => {
                let version = if pack.version.is_empty() {
                    pack.name.as_str()
                } else {
                    pack.version.as_str()
                };
                if pack.has_faces {
                    format!("{version} · faces")
                } else {
                    version.to_string()
                }
            }
            None => {
                if localgallery::ml_enabled() {
                    "None installed".into()
                } else {
                    "This build has no ONNX".into()
                }
            }
        };
        let xdg = localgallery::xdg_pack_present();
        let pack_busy = self.inner.pack_busy.get();

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

        let scan_row = progress_row(&ProgressRowData::from(&scan));
        if let Some(cancel) = find_widget::<gtk::Button>(&scan_row) {
            let this = self.clone();
            cancel.connect_clicked(move |_| this.cancel_work());
        }
        self.inner.scan_progress.replace(Some(scan_row.clone()));
        let busy = self.inner.scan_busy.get() || self.inner.analysis_busy.get() || pack_busy;
        let scan_action = action_row(&ActionRowData {
            label: "Scan Photos".into(),
            role: ActionRole::Normal,
            enabled: folder.is_some() && photo_count > 0 && !busy,
        });
        let this = self.clone();
        scan_action.connect_clicked(move |_| this.scan_photos());

        let everything = toggle_row(&ToggleRowData {
            label: "Everything".into(),
            on: self.inner.analysis_everything.get(),
        });
        let tagging = toggle_row(&ToggleRowData {
            label: "Objects & Scenes".into(),
            on: self.inner.analysis_tagging.get(),
        });
        let faces = toggle_row(&ToggleRowData {
            label: "Face tagging".into(),
            on: self.inner.analysis_faces.get(),
        });
        let geotag = toggle_row(&ToggleRowData {
            label: "Geotagging".into(),
            on: self.inner.analysis_places.get(),
        });
        let syncing = Rc::new(Cell::new(false));
        let this = self.clone();
        let tagging_sync = tagging.clone();
        let faces_sync = faces.clone();
        let geotag_sync = geotag.clone();
        let syncing_all = syncing.clone();
        everything.connect_active_notify(move |row| {
            if syncing_all.get() {
                return;
            }
            let on = row.is_active();
            this.inner.analysis_everything.set(on);
            syncing_all.set(true);
            this.inner.analysis_tagging.set(on);
            this.inner.analysis_faces.set(on);
            this.inner.analysis_places.set(on);
            tagging_sync.set_active(on);
            faces_sync.set_active(on);
            geotag_sync.set_active(on);
            syncing_all.set(false);
        });
        let this = self.clone();
        let everything_tag = everything.clone();
        let syncing_tag = syncing.clone();
        tagging.connect_active_notify(move |row| {
            if syncing_tag.get() {
                return;
            }
            this.inner.analysis_tagging.set(row.is_active());
            syncing_tag.set(true);
            everything_tag.set_active(
                this.inner.analysis_tagging.get()
                    && this.inner.analysis_faces.get()
                    && this.inner.analysis_places.get(),
            );
            this.inner
                .analysis_everything
                .set(everything_tag.is_active());
            syncing_tag.set(false);
        });
        let this = self.clone();
        let everything_face = everything.clone();
        let syncing_face = syncing.clone();
        faces.connect_active_notify(move |row| {
            if syncing_face.get() {
                return;
            }
            this.inner.analysis_faces.set(row.is_active());
            syncing_face.set(true);
            everything_face.set_active(
                this.inner.analysis_tagging.get()
                    && this.inner.analysis_faces.get()
                    && this.inner.analysis_places.get(),
            );
            this.inner
                .analysis_everything
                .set(everything_face.is_active());
            syncing_face.set(false);
        });
        let this = self.clone();
        let everything_geo = everything.clone();
        let syncing_geo = syncing;
        geotag.connect_active_notify(move |row| {
            if syncing_geo.get() {
                return;
            }
            this.inner.analysis_places.set(row.is_active());
            syncing_geo.set(true);
            everything_geo.set_active(
                this.inner.analysis_tagging.get()
                    && this.inner.analysis_faces.get()
                    && this.inner.analysis_places.get(),
            );
            this.inner
                .analysis_everything
                .set(everything_geo.is_active());
            syncing_geo.set(false);
        });

        let download = action_row(&ActionRowData {
            label: "Download ML models".into(),
            role: ActionRole::Normal,
            enabled: !busy && !xdg,
        });
        let this = self.clone();
        download.connect_clicked(move |_| this.start_pack_download());
        let remove = action_row(&ActionRowData {
            label: "Remove ML models".into(),
            role: ActionRole::Destructive,
            enabled: !busy && xdg,
        });
        let this = self.clone();
        remove.connect_clicked(move |_| this.start_pack_remove());

        let logs = nav_row(&NavRowData {
            label: "Logs".into(),
            trailing: Some("Info, warnings, and errors".into()),
        });
        let this = self.clone();
        logs.connect_activated(move |_| this.present_logs());
        let conflicts = nav_row(&NavRowData {
            label: "Sync Conflicts".into(),
            trailing: Some(if conflict_count == 0 {
                "None".into()
            } else {
                conflict_count.to_string()
            }),
        });
        let this = self.clone();
        conflicts.connect_activated(move |_| this.present_conflicts());

        let contacts = adw::ActionRow::builder()
            .title("Contacts folder")
            .subtitle(contacts_path.unwrap_or_else(|| "None — files only".into()))
            .activatable(true)
            .build();
        contacts.add_suffix(&gtk::Image::from_icon_name("folder-open-symbolic"));
        let this = self.clone();
        contacts.connect_activated(move |_| this.pick_contacts_folder());

        let mut people_rows: Vec<gtk::Widget> = vec![contacts.upcast()];
        for path in hidden {
            let leaf = path.rsplit('/').next().unwrap_or(path.as_str()).to_string();
            let unhide = action_row(&ActionRowData {
                label: format!("Unhide {leaf}"),
                role: ActionRole::Normal,
                enabled: true,
            });
            let this = self.clone();
            let hide_path = path;
            unhide.connect_clicked(move |_| {
                localcore_trace::event("people", format!("settings unhide path={hide_path}"));
                if !this.inner.session.borrow_mut().unhide_person(&hide_path) {
                    this.toast("Could not unhide this person");
                    return;
                }
                this.refresh_people_and_memories();
            });
            people_rows.push(unhide.upcast());
        }

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
                    id: "scan".into(),
                    title: "On-device Tagging".into(),
                    rows: vec![
                        text_row(&TextRowData {
                            title: "Model Pack".into(),
                            subtitle: Some(pack_summary),
                            trailing: None,
                            leading: None,
                        })
                        .upcast(),
                        scan_row.upcast(),
                        everything.upcast(),
                        tagging.upcast(),
                        faces.upcast(),
                        geotag.upcast(),
                        scan_action.upcast(),
                        download.upcast(),
                        remove.upcast(),
                    ],
                },
                SettingsGroup {
                    id: "people".into(),
                    title: "People".into(),
                    rows: people_rows,
                },
                SettingsGroup {
                    id: "diagnostics".into(),
                    title: "Diagnostics".into(),
                    rows: vec![conflicts.upcast(), logs.upcast()],
                },
                SettingsGroup {
                    id: "info".into(),
                    title: "Info".into(),
                    rows: vec![
                        text_row(&TextRowData {
                            title: "Photos".into(),
                            subtitle: None,
                            trailing: Some(photo_count.to_string()),
                            leading: None,
                        })
                        .upcast(),
                        text_row(&TextRowData {
                            title: "People".into(),
                            subtitle: None,
                            trailing: Some(people_count.to_string()),
                            leading: None,
                        })
                        .upcast(),
                        text_row(&TextRowData {
                            title: "Places".into(),
                            subtitle: None,
                            trailing: Some(places_count.to_string()),
                            leading: None,
                        })
                        .upcast(),
                        text_row(&TextRowData {
                            title: "Objects".into(),
                            subtitle: None,
                            trailing: Some(objects_count.to_string()),
                            leading: None,
                        })
                        .upcast(),
                        text_row(&TextRowData {
                            title: "Scenes".into(),
                            subtitle: None,
                            trailing: Some(scenes_count.to_string()),
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
                    ],
                },
            ],
        }
    }

    fn sync_scan_progress(&self) {
        let Some(row) = self.inner.scan_progress.borrow().clone() else {
            return;
        };
        let scan = self.inner.session.borrow().settings_progress();
        apply_progress_row(&row, &ProgressRowData::from(&scan));
        self.sync_chrome_progress();
    }

    fn sync_chrome_progress(&self) {
        let progress = self
            .inner
            .session
            .borrow()
            .chrome_progress()
            .map(|display| ProgressRowData::from(&display));
        self.inner.chrome_progress.apply(progress.as_ref());
    }
}

fn explorer_tile() -> gtk::Widget {
    let stack = gtk::Stack::new();
    stack.set_widget_name("explorer-tile");
    stack.add_named(&cover_overlay_tile(), Some("folder"));
    stack.add_named(&photo_tile(), Some("photo"));
    stack.upcast()
}

fn sidebar_folder_row() -> gtk::Widget {
    let icon = gtk::Image::from_icon_name("folder-symbolic");
    icon.set_pixel_size(16);
    let name = gtk::Label::new(None);
    name.set_widget_name("folder-name");
    name.set_xalign(0.0);
    name.set_hexpand(true);
    name.set_ellipsize(pango::EllipsizeMode::End);
    let count = gtk::Label::new(None);
    count.set_widget_name("folder-count");
    count.add_css_class("dim-label");
    count.add_css_class("numeric");
    count.add_css_class("caption");
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.append(&icon);
    row.append(&name);
    row.append(&count);
    let expander = gtk::TreeExpander::new();
    expander.set_child(Some(&row));
    expander.upcast()
}

fn sidebar_folder_id(list: &gtk::ListView, pos: u32) -> Option<String> {
    let selection = list.model().and_downcast::<gtk::SingleSelection>()?;
    let row = selection.item(pos).and_downcast::<gtk::TreeListRow>()?;
    let boxed = row.item().and_downcast::<glib::BoxedAnyObject>()?;
    let id = boxed.borrow::<FolderEntry>().id.clone();
    Some(id)
}

fn show_named_stack_child(root: &gtk::Widget, name: &str) {
    if let Some(stack) = find_named_widget::<gtk::Stack>(root, "explorer-tile") {
        stack.set_visible_child_name(name);
    } else if let Ok(stack) = root.clone().downcast::<gtk::Stack>() {
        stack.set_visible_child_name(name);
    }
}

fn crumb_button(label: &str, current: bool) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.add_css_class("flat");
    button.add_css_class("folder-crumb");
    button.set_has_frame(false);
    button.set_sensitive(!current);
    if current {
        button.add_css_class("heading");
    }
    button
}

fn crumb_sep() -> gtk::Label {
    let label = gtk::Label::new(Some("›"));
    label.add_css_class("dim-label");
    label
}

fn display_font_candidates() -> Vec<PathBuf> {
    let file = localcore_ui::gallery::DISPLAY_FILE;
    let mut paths = Vec::new();
    // Compile-time checkout (container `/work` is wrong on the host).
    paths.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../design/fonts")
            .join(file),
    );
    // `shells/target/{debug,release}/localgallery` → repo `design/fonts`.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(shells) = exe
            .parent()
            .and_then(|dir| dir.parent())
            .and_then(|dir| dir.parent())
        {
            paths.push(shells.join("../design/fonts").join(file));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join("design/fonts").join(file));
        paths.push(cwd.join("../design/fonts").join(file));
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            paths.push(PathBuf::from(xdg).join("fonts").join(file));
        }
    } else if let Some(home) = std::env::var_os("HOME") {
        paths.push(Path::new(&home).join(".local/share/fonts").join(file));
    }
    paths.push(PathBuf::from("/app/share/fonts").join(file));
    paths
}

fn display_font_path() -> Option<PathBuf> {
    display_font_candidates()
        .into_iter()
        .find(|path| path.is_file())
}

fn load_display_font() {
    let Some(path) = display_font_path() else {
        eprintln!(
            "display font {} not found",
            localcore_ui::gallery::DISPLAY_FILE
        );
        return;
    };
    let Some(font_map) = gtk::Label::new(None).pango_context().font_map() else {
        eprintln!("no default Pango font map; skipping {}", path.display());
        return;
    };
    if let Err(error) = font_map.add_font_file(&path) {
        eprintln!("could not load display font {}: {error}", path.display());
    }
}

fn first_named_child(host: &impl IsA<gtk::Widget>, name: &str) -> Option<gtk::Widget> {
    let mut child = host.as_ref().first_child();
    while let Some(widget) = child {
        if widget.widget_name() == name {
            return Some(widget);
        }
        child = widget.next_sibling();
    }
    None
}

fn find_named_widget<T: IsA<gtk::Widget>>(root: &impl IsA<gtk::Widget>, name: &str) -> Option<T> {
    let root = root.as_ref();
    if let Ok(found) = root.clone().downcast::<T>() {
        if found.widget_name() == name {
            return Some(found);
        }
    }
    let mut child = root.first_child();
    while let Some(node) = child {
        if let Some(found) = find_named_widget::<T>(&node, name) {
            return Some(found);
        }
        child = node.next_sibling();
    }
    None
}

fn overlay_tile(width: i32, height: i32, card_class: &str) -> gtk::Widget {
    let picture = gtk::Picture::new();
    picture.set_content_fit(gtk::ContentFit::Cover);
    picture.set_can_shrink(true);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    picture.set_widget_name("cover-pic");
    if card_class == "memory-card" {
        picture.add_css_class("memory-tile");
    } else {
        picture.add_css_class("cover-tile");
    }
    let title = gtk::Label::new(None);
    title.add_css_class("cover-title");
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_widget_name("cover-title");
    let subtitle = gtk::Label::new(None);
    subtitle.add_css_class("caption");
    subtitle.add_css_class("cover-subtitle");
    subtitle.set_xalign(0.0);
    subtitle.set_ellipsize(gtk::pango::EllipsizeMode::End);
    subtitle.set_widget_name("cover-subtitle");
    let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
    labels.add_css_class("cover-captions");
    labels.set_widget_name("cover-captions");
    labels.set_valign(gtk::Align::End);
    labels.set_halign(gtk::Align::Fill);
    labels.set_can_target(false);
    labels.append(&title);
    labels.append(&subtitle);
    let scrim = gtk::Box::new(gtk::Orientation::Vertical, 0);
    scrim.add_css_class("cover-scrim");
    scrim.set_halign(gtk::Align::Fill);
    scrim.set_valign(gtk::Align::Fill);
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);
    scrim.set_can_target(false);
    let overlay = gtk::Overlay::new();
    overlay.add_css_class(card_class);
    overlay.set_widget_name("cover-overlay");
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);
    overlay.set_overflow(gtk::Overflow::Hidden);
    overlay.set_child(Some(&picture));
    overlay.add_overlay(&scrim);
    overlay.add_overlay(&labels);
    let frame = gtk::AspectFrame::builder()
        .ratio(width as f32 / height as f32)
        .obey_child(false)
        .build();
    frame.add_css_class(card_class);
    frame.set_size_request(width, height);
    frame.set_hexpand(true);
    frame.set_vexpand(true);
    frame.set_overflow(gtk::Overflow::Hidden);
    frame.set_child(Some(&overlay));
    frame.upcast()
}

fn memory_hero_tile() -> gtk::Widget {
    let root = overlay_tile(MEMORY_CARD_W, MEMORY_CARD_H, "memory-card");
    if let Some(title) = find_named_widget::<gtk::Label>(&root, "cover-title") {
        title.add_css_class("title-2");
        title.add_css_class("memory-title");
        title.set_wrap(true);
        title.set_lines(2);
    }
    root
}

fn filter_chip_icon(id: &str) -> &'static str {
    let namespace = if id.starts_with("date:") {
        Some("date")
    } else {
        id.split('/').next()
    };
    gallery_ffi::SearchKind::from_namespace(namespace)
        .map(gallery_ffi::SearchKind::symbol)
        .unwrap_or("tag-symbolic")
}

fn display_filter_label(id: &str) -> String {
    id.strip_prefix("date:")
        .or_else(|| id.rsplit('/').next())
        .unwrap_or(id)
        .to_string()
}

fn person_row_from_list_item(item: &gtk::ListItem) -> Option<(String, String)> {
    let boxed = item.item().and_downcast::<glib::BoxedAnyObject>()?;
    let row = boxed.borrow::<gallery_ffi::GalleryTextRow>().clone();
    Some((row.id, row.title))
}

fn person_card_tile() -> gtk::Widget {
    let root = overlay_tile(PERSON_TILE_PX, PERSON_TILE_PX, "person-card");
    if let Some(title) = find_named_widget::<gtk::Label>(&root, "cover-title") {
        title.add_css_class("caption-heading");
    }
    if let Some(overlay) = find_named_widget::<gtk::Overlay>(&root, "cover-overlay") {
        let badges = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        badges.set_widget_name("person-badges");
        badges.add_css_class("person-badges");
        badges.set_halign(gtk::Align::End);
        badges.set_valign(gtk::Align::Start);
        badges.set_can_target(false);
        // iOS order: contact, me, featured last.
        badges.append(&person_badge(
            "person-link",
            "x-office-address-book-symbolic",
        ));
        badges.append(&person_badge("person-me", "avatar-default-symbolic"));
        badges.append(&person_badge("person-star", "starred-symbolic"));
        overlay.add_overlay(&badges);
    }
    root
}

fn person_badge(name: &str, icon: &str) -> gtk::Widget {
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(12);
    image.set_halign(gtk::Align::Center);
    image.set_valign(gtk::Align::Center);
    image.set_hexpand(true);
    image.set_vexpand(true);
    image.set_can_target(false);
    let badge = gtk::CenterBox::new();
    badge.set_widget_name(name);
    badge.add_css_class("person-badge");
    badge.set_overflow(gtk::Overflow::Hidden);
    badge.set_halign(gtk::Align::Center);
    badge.set_valign(gtk::Align::Center);
    badge.set_hexpand(false);
    badge.set_vexpand(false);
    badge.set_size_request(20, 20);
    badge.set_can_target(false);
    badge.set_visible(false);
    badge.set_center_widget(Some(&image));
    badge.upcast()
}

fn after_person_menu(popover: &gtk::Popover, work: impl FnOnce() + 'static) {
    popover.popdown();
    glib::idle_add_local_once(work);
}

fn person_menu_item(label: &str, activate: impl Fn() + 'static) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.set_has_frame(false);
    button.add_css_class("flat");
    button.add_css_class("person-menu-item");
    button.set_hexpand(true);
    if let Some(child) = button.child() {
        if let Ok(title) = child.downcast::<gtk::Label>() {
            title.set_xalign(0.0);
            title.set_hexpand(true);
        }
    }
    button.connect_clicked(move |_| activate());
    button
}

fn set_named_visible(root: &gtk::Widget, name: &str, visible: bool) {
    if let Some(widget) = find_named_widget::<gtk::Widget>(root, name) {
        widget.set_visible(visible);
    }
}

fn cover_overlay_tile() -> gtk::Widget {
    let root = overlay_tile(EVENT_TILE_PX, EVENT_TILE_PX, "event-card");
    if let Some(title) = find_named_widget::<gtk::Label>(&root, "cover-title") {
        title.add_css_class("heading");
    }
    root
}

fn set_cover_captions(root: &gtk::Widget, title: &str, subtitle: &str) {
    if let Some(label) = find_named_widget::<gtk::Label>(root, "cover-title") {
        label.set_text(title);
    }
    if let Some(label) = find_named_widget::<gtk::Label>(root, "cover-subtitle") {
        label.set_text(subtitle);
        label.set_visible(!subtitle.is_empty());
    }
}

fn photo_count_caption(count: usize) -> String {
    if count == 1 {
        "1 photo".into()
    } else {
        format!("{count} photos")
    }
}

fn hub_section_label(title: &str) -> gtk::Label {
    let label = section_label(title);
    label.remove_css_class("title-4");
    label.add_css_class("title-3");
    label
}

fn hub_section(title: &str, see_all: Option<gtk::Button>, body: gtk::Widget) -> gtk::Widget {
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.append(&hub_section_label(title));
    if let Some(see) = see_all {
        header.append(&see);
    }
    let block = gtk::Box::new(gtk::Orientation::Vertical, 10);
    block.set_widget_name(&format!("hub-{}", title.to_ascii_lowercase()));
    block.append(&header);
    block.append(&body);
    block.upcast()
}

fn see_all_button(activate: impl Fn() + 'static) -> gtk::Button {
    let see = gtk::Button::with_label("See all");
    see.add_css_class("flat");
    see.set_valign(gtk::Align::Center);
    see.connect_clicked(move |_| activate());
    see
}

fn cover_rail(list: gtk::ListView) -> gtk::Widget {
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Never);
    scroll.set_child(Some(&list));
    scroll.set_propagate_natural_height(true);
    scroll.upcast()
}

/// Hub rail that may be wider than the viewport. Extra tiles clip so resize
/// reveals more instead of leaving an empty track.
///
/// Horizontal policy must be `External`, not `Never`: `Never` sizes the
/// scroller to the child, so peek columns would widen the window and shove
/// header chrome off-screen.
fn clip_hub_rail(child: impl IsA<gtk::Widget>) -> gtk::Widget {
    let child = child.upcast::<gtk::Widget>();
    child.set_hexpand(false);
    child.set_halign(gtk::Align::Start);
    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("gallery-hub-clip");
    scroll.set_policy(gtk::PolicyType::External, gtk::PolicyType::Never);
    scroll.set_overflow(gtk::Overflow::Hidden);
    scroll.set_propagate_natural_width(false);
    scroll.set_propagate_natural_height(true);
    scroll.set_min_content_width(PERSON_TILE_PX);
    scroll.set_hexpand(true);
    scroll.set_child(Some(&child));
    scroll.upcast()
}

fn photo_tile() -> gtk::AspectFrame {
    let picture = gtk::Picture::new();
    picture.set_widget_name("photo-pic");
    picture.set_content_fit(gtk::ContentFit::Cover);
    picture.add_css_class("gallery-tile");
    picture.set_can_shrink(true);
    picture.set_hexpand(true);
    picture.set_vexpand(false);
    let icon = gtk::Image::from_icon_name("media-playback-start-symbolic");
    icon.set_pixel_size(12);
    let badge = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    badge.set_widget_name("video-badge");
    badge.add_css_class("video-badge");
    badge.set_halign(gtk::Align::End);
    badge.set_valign(gtk::Align::End);
    badge.set_visible(false);
    badge.append(&icon);
    let check = gtk::Image::from_icon_name("object-select-symbolic");
    check.set_pixel_size(14);
    let select = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    select.set_widget_name("select-badge");
    select.add_css_class("select-badge");
    select.set_halign(gtk::Align::Start);
    select.set_valign(gtk::Align::Start);
    select.set_visible(false);
    select.append(&check);
    let tile = gtk::Overlay::new();
    tile.set_child(Some(&picture));
    tile.add_overlay(&badge);
    tile.add_overlay(&select);
    let frame = gtk::AspectFrame::builder()
        .ratio(1.0)
        .obey_child(false)
        .halign(gtk::Align::Fill)
        .valign(gtk::Align::Fill)
        .hexpand(true)
        .vexpand(false)
        .build();
    frame.set_size_request(PHOTO_TILE_PX, PHOTO_TILE_PX);
    frame.set_child(Some(&tile));
    frame
}

/// iOS `PhotoPageView`: movies play inline. Host flag wins; the path
/// extension is the fallback when a row was not in the host map.
fn viewer_height_is_short(height: i32) -> bool {
    height > 0 && height <= VIEWER_SHORT_HEIGHT
}

fn viewer_shows_sequence_osd(count: usize, height: i32) -> bool {
    count > 1 && !viewer_height_is_short(height)
}

fn camera_label(make: Option<&str>, model: Option<&str>) -> String {
    match (
        make.map(str::trim).filter(|value| !value.is_empty()),
        model.map(str::trim).filter(|value| !value.is_empty()),
    ) {
        (Some(make), Some(model)) => format!("{make} {model}"),
        (Some(make), None) => make.to_string(),
        (None, Some(model)) => model.to_string(),
        (None, None) => "—".into(),
    }
}

fn viewer_is_video(host: Option<&crate::session::PhotoHost>, path: Option<&str>) -> bool {
    host.is_some_and(|host| host.is_video) || path.is_some_and(localgallery::video::is_video_path)
}

fn viewer_media_surface(
    thumbs: &ThumbCache,
    id: &str,
    path: Option<&str>,
    is_video: bool,
    max_side: u32,
) -> (gtk::Widget, Option<gtk::Picture>) {
    let stage = gtk::Overlay::new();
    stage.set_hexpand(true);
    stage.set_vexpand(true);
    let picture = gtk::Picture::new();
    picture.set_content_fit(gtk::ContentFit::Contain);
    picture.set_hexpand(true);
    picture.set_vexpand(true);
    if let Some(path) = path {
        localcore_trace::event(
            "viewer",
            format!("bind_viewer id={id} path={path} max_side={max_side} video={is_video}"),
        );
        thumbs.bind_viewer(&picture, path, id, max_side);
    } else {
        localcore_trace::event(
            "viewer",
            format!("EMPTY picture — no host path for id={id} (detail view stays blank)"),
        );
    }
    stage.set_child(Some(&picture));
    if is_video {
        if let Some(path) = path {
            let play = viewer_play_button();
            let stage_play = stage.clone();
            let hide = play.clone();
            let movie = path.to_string();
            play.connect_clicked({
                let stage_play = stage_play.clone();
                let hide = hide.clone();
                let movie = movie.clone();
                move |_| start_inline_playback(&stage_play, &hide, &movie)
            });
            let tap = gtk::GestureClick::new();
            tap.connect_released(move |_, _, _, _| {
                start_inline_playback(&stage_play, &hide, &movie);
            });
            picture.add_controller(tap);
            play.set_halign(gtk::Align::Center);
            play.set_valign(gtk::Align::Center);
            stage.add_overlay(&play);
        }
    }
    let still = (!is_video).then(|| picture.clone());
    (stage.upcast(), still)
}

const VIEWER_MAX_ZOOM: f64 = 4.0;

thread_local! {
    static STILL_ZOOM_CSS: Cell<u32> = const { Cell::new(0) };
}

fn clamp_viewer_zoom(current: f64, factor: f64) -> f64 {
    (current * factor).clamp(1.0, VIEWER_MAX_ZOOM)
}

fn attach_tap_to_hide(target: &impl IsA<gtk::Widget>, hideables: Vec<gtk::Widget>) {
    if hideables.is_empty() {
        return;
    }
    let shown = Rc::new(Cell::new(true));
    let origin = Rc::new(Cell::new((0.0, 0.0)));
    let tap = gtk::GestureClick::new();
    tap.set_button(1);
    tap.connect_pressed({
        let origin = origin.clone();
        move |_, _, x, y| origin.set((x, y))
    });
    tap.connect_released(move |_, n_press, x, y| {
        if n_press != 1 {
            return;
        }
        let (ox, oy) = origin.get();
        if (x - ox).hypot(y - oy) > 16.0 {
            return;
        }
        let next = !shown.get();
        shown.set(next);
        for widget in &hideables {
            widget.set_visible(next);
        }
    });
    target.add_controller(tap);
}

fn attach_still_pinch_zoom(picture: &gtk::Picture) {
    let token = STILL_ZOOM_CSS.with(|cell| {
        let next = cell.get().wrapping_add(1);
        cell.set(next);
        next
    });
    let class = format!("gallery-zoom-{token}");
    picture.add_css_class(&class);
    let scale = Rc::new(Cell::new(1.0_f64));
    let begin = Rc::new(Cell::new(1.0_f64));
    let provider = gtk::CssProvider::new();
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    let apply = {
        let provider = provider.clone();
        let class = class.clone();
        move |value: f64| {
            provider.load_from_string(&format!(".{class} {{ transform: scale({value}); }}"));
        }
    };
    let zoom = gtk::GestureZoom::new();
    zoom.connect_begin({
        let scale = scale.clone();
        let begin = begin.clone();
        move |_, _| begin.set(scale.get())
    });
    zoom.connect_scale_changed(move |_, factor| {
        let next = clamp_viewer_zoom(begin.get(), factor);
        scale.set(next);
        apply(next);
    });
    picture.add_controller(zoom);
}

fn viewer_play_button() -> gtk::Button {
    let play = gtk::Button::from_icon_name("media-playback-start-symbolic");
    play.add_css_class("circular");
    play.add_css_class("osd");
    play.add_css_class("gallery-play");
    play.set_tooltip_text(Some("Play"));
    play
}

fn start_inline_playback(stage: &gtk::Overlay, play: &gtk::Button, path: &str) {
    if !play.is_visible() {
        return;
    }
    play.set_visible(false);
    localcore_trace::event("viewer", format!("play {path}"));
    stage.set_child(Some(&playing_video(path)));
}

fn playing_video(path: &str) -> gtk::Video {
    let video = gtk::Video::for_filename(Some(path));
    video.set_autoplay(true);
    video.set_loop(false);
    video.set_hexpand(true);
    video.set_vexpand(true);
    video.add_css_class("gallery-viewer-video");
    video.connect_unrealize(|video| {
        if let Some(stream) = video.media_stream() {
            stream.pause();
        }
    });
    video
}

fn set_tile_video_badge(item: &gtk::ListItem, on: bool) {
    if let Some(child) = item.child() {
        if let Some(badge) = find_named_widget::<gtk::Box>(&child, "video-badge") {
            badge.set_visible(on);
        }
    }
}

/// True when the viewport is close to the loaded tail *and* the
/// adjustment has a real content height. `upper <= page` means the
/// GridView has not allocated rows yet (or is not virtualizing) —
/// appending then would idle-fill the whole library again.
fn adjustment_near_end(adj: &gtk::Adjustment) -> bool {
    let page = adj.page_size();
    if page <= 0.0 {
        return false;
    }
    let upper = adj.upper();
    if adj.value() <= 0.0 && upper <= page + 1.0 {
        return false;
    }
    upper - (adj.value() + page) < page * 2.0
}

/// Let GridView pack as many 108px tiles as fit. Do not force min=max
/// columns from the window width — that leaves empty cell space around
/// a picture that has not measured yet.
fn pack_photo_grid(grid: &gtk::GridView) {
    grid.set_min_columns(2);
    grid.set_max_columns(24);
}

fn pack_cover_grid(grid: &gtk::GridView, tile_px: i32, gap_px: i32, width: i32) {
    let stride = tile_px + gap_px;
    let columns = (width.max(stride) / stride).clamp(2, 24) as u32;
    grid.set_min_columns(columns);
    grid.set_max_columns(columns);
}

fn bind_cover_grid_columns(
    scroll: &gtk::ScrolledWindow,
    grid: &gtk::GridView,
    tile_px: i32,
    gap_px: i32,
) {
    pack_cover_grid(grid, tile_px, gap_px, scroll.width().max(tile_px));
    let grid = grid.clone();
    scroll.add_tick_callback(move |scroll, _| {
        let width = scroll.width();
        if width > 1 {
            pack_cover_grid(&grid, tile_px, gap_px, width);
        }
        glib::ControlFlow::Continue
    });
}

fn apply_shell_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(SHELL_CSS);
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("a display"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn section_label(title: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(title));
    label.add_css_class("title-4");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label
}

fn picture_from_item(item: &gtk::ListItem) -> Option<gtk::Picture> {
    find_widget(&item.child()?)
}

fn find_widget<T: IsA<gtk::Widget>>(root: &impl IsA<gtk::Widget>) -> Option<T> {
    let root = root.as_ref();
    if let Ok(found) = root.clone().downcast::<T>() {
        return Some(found);
    }
    let mut child = root.first_child();
    while let Some(node) = child {
        if let Some(found) = find_widget::<T>(&node) {
            return Some(found);
        }
        child = node.next_sibling();
    }
    None
}

fn clear_list(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

#[cfg(test)]
mod tests {
    use super::{display_filter_label, filter_chip_icon, viewer_is_video};
    use crate::session::PhotoHost;

    #[test]
    fn filter_chips_reuse_search_hit_icons() {
        assert_eq!(filter_chip_icon("People/Ada"), "system-users-symbolic");
        assert_eq!(filter_chip_icon("Places/Italy"), "mark-location-symbolic");
        assert_eq!(
            filter_chip_icon("date:2024-06"),
            "x-office-calendar-symbolic"
        );
        assert_eq!(filter_chip_icon("Vacation"), "tag-symbolic");
        assert_eq!(display_filter_label("People/Ada"), "Ada");
        assert_eq!(display_filter_label("date:2024-06"), "2024-06");
    }

    #[test]
    fn pinch_zoom_is_capped_on_the_existing_still() {
        assert_eq!(super::clamp_viewer_zoom(1.0, 2.0), 2.0);
        assert_eq!(super::clamp_viewer_zoom(3.0, 2.0), 4.0);
        assert_eq!(super::clamp_viewer_zoom(1.0, 0.2), 1.0);
    }

    #[test]
    fn filmstrip_and_arrows_follow_leftover_short_rule() {
        assert!(super::viewer_shows_sequence_osd(2, 800));
        assert!(!super::viewer_shows_sequence_osd(2, 700));
        assert!(!super::viewer_shows_sequence_osd(2, 540));
        assert!(!super::viewer_shows_sequence_osd(1, 800));
        assert!(
            super::viewer_shows_sequence_osd(3, 0),
            "unallocated height is not short"
        );
    }

    #[test]
    fn camera_label_joins_make_and_model() {
        assert_eq!(
            super::camera_label(Some("Canon"), Some("EOS R5")),
            "Canon EOS R5"
        );
        assert_eq!(super::camera_label(Some("Canon"), None), "Canon");
        assert_eq!(super::camera_label(None, Some("EOS R5")), "EOS R5");
        assert_eq!(super::camera_label(None, None), "—");
        assert_eq!(super::camera_label(Some("  "), Some("")), "—");
    }

    #[test]
    fn movies_use_the_inline_player() {
        let host = PhotoHost {
            path: "/lib/Clip.MOV".into(),
            filename: "Clip.MOV".into(),
            is_video: true,
            live_photo_video_path: None,
            face_regions: Vec::new(),
        };
        assert!(viewer_is_video(Some(&host), Some(&host.path)));
        assert!(viewer_is_video(None, Some("/lib/a.mp4")));
        assert!(!viewer_is_video(None, Some("/lib/a.jpg")));
        let still = PhotoHost {
            path: "/lib/a.jpg".into(),
            filename: "a.jpg".into(),
            is_video: false,
            live_photo_video_path: None,
            face_regions: Vec::new(),
        };
        assert!(!viewer_is_video(Some(&still), Some(&still.path)));
    }
}
