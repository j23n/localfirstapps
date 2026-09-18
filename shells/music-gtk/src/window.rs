//! Native GTK4/libadwaita assembly for every generated Music screen.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Instant;

use adw::prelude::*;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use music_core::{
    ConfinedVfs, ConflictDisposition, LibraryContentState, SortOption,
    StatusSeverity as CoreStatusSeverity, StdVfs, Store, TEMP_PREFIX,
};
use shell_kit_gtk::{
    about_dialog, action_row, adaptive_shell, apply_progress_row, banner, choice_dropdown,
    chrome_progress, confirm_dialog, count_detail, empty_state, field_row, flush_media_list,
    form_sheet, found_detail, header_action, highlight_markup, init_style, list_box_page,
    list_screen, media_item, nav_row, navigation_view, overflow, page, pill_primary,
    preferences_dialog, primary_action, primary_menu, progress_row, push_settings_subpage,
    search_entry, search_hit_row, settings_screen, sheet, status_row, text_row, ActionRole,
    ActionRowData, AdaptiveShell, ChoiceData, ChromeProgress, ConfirmData, EmptyCopy, EmptyKind,
    EmptyState, FieldRowData, Filter, FilterControl, FormSheet, Leading, ListScreen,
    ListScreenBuilt, ListSection, LogLevel, MediaItemData, MenuCommand, MusicScreen, NavRowData,
    PageChrome, PrimaryMenu, ProgressDisplay, ProgressRowData, RootPage, SettingsGroup,
    SettingsScreen, SheetSize, StatusRowData, StatusSeverity, TextRowData, WorkProgress,
};

use crate::mpris::MprisState;
use crate::routing::route_id;
use crate::{
    hostname, system_transport, MprisHost, Paths, RemoteCommand, Session, ShellError, APP_TITLE,
};

const TOKEN_CSS: &str = include_str!("../../../design/tokens/generated/music.css");
const DIAGNOSTIC_CAPACITY: usize = 5_000;
const NOW_PLAYING_WIDTH: i32 = 400;

type AppSession = Session<Box<dyn crate::TransportPort>>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct PlaylistEntryRef {
    playlist_id: String,
    entry_id: String,
}

#[derive(Clone, Default, PartialEq, Eq)]
enum Drill {
    #[default]
    None,
    Artist(String),
    Album(String),
    Playlist(String),
}

#[derive(Clone)]
pub struct Window {
    inner: Rc<Inner>,
}

struct Inner {
    window: adw::ApplicationWindow,
    shell: AdaptiveShell,
    root_stack: gtk::Stack,
    songs_list: gtk::ListBox,
    artists_list: gtk::ListBox,
    albums_list: gtk::ListBox,
    playlist_list: gtk::ListBox,
    playlist_pages: gtk::Stack,
    artists_nav: adw::NavigationView,
    albums_nav: adw::NavigationView,
    playlists_nav: adw::NavigationView,
    sort: gtk::DropDown,
    add_playlist: gtk::Button,
    now_column: gtk::Box,
    #[allow(dead_code)]
    menu: PrimaryMenu,
    settings_dialog: RefCell<Option<adw::PreferencesDialog>>,
    conflict_banner: adw::Banner,
    toast: adw::ToastOverlay,
    now_title: gtk::Label,
    now_badge: gtk::Label,
    now_art: gtk::Image,
    play_pause: gtk::Button,
    mini_title: gtk::Label,
    mini_badge: gtk::Label,
    mini_art: gtk::Image,
    mini_play: gtk::Button,
    query: RefCell<String>,
    sort_option: Cell<SortOption>,
    artwork: RefCell<HashMap<String, gdk::Texture>>,
    search_entry: gtk::SearchEntry,
    search_results: gtk::ListBox,
    content_pages: gtk::Stack,
    drill: RefCell<Drill>,
    drill_list: RefCell<Option<gtk::ListBox>>,
    session: RefCell<AppSession>,
    paths: Paths,
    mpris: RefCell<Option<MprisHost>>,
    chrome_progress: ChromeProgress,
    work: Arc<Mutex<Option<WorkProgress>>>,
    work_rx: RefCell<Option<mpsc::Receiver<LibraryWork>>>,
    work_busy: Cell<bool>,
    work_cancel: RefCell<Option<Arc<AtomicBool>>>,
    work_tick: Cell<bool>,
    scan_progress_row: RefCell<Option<adw::ActionRow>>,
}

enum LibraryWork {
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
        let skip_folder = route == Some(route_id(MusicScreen::FolderPicker));
        let wait = launch.snapshot.is_some() || launch.route.is_some();
        if let Some(folder) = launch.folder.as_ref().filter(|_| !skip_folder) {
            match folder.to_str() {
                Some(path) => self.open_folder(path, wait),
                None => self.toast("The selected Folder is not a local UTF-8 path"),
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
                    .set_visible_child_name(route_id(MusicScreen::FolderPicker));
            }
            "library" => self.show_tab("songs"),
            "playlist-list" => self.show_tab("playlists"),
            "playlist-detail" => {
                self.show_tab("playlists");
                if let Some(id) = self.first_playlist_id() {
                    self.open_playlist_detail(&id);
                }
            }
            "add-tracks" => {
                if let Some(id) = self.first_playlist_id() {
                    let detail = self.inner.session.borrow().playlist_detail(&id);
                    if let Ok(detail) = detail {
                        self.present_add_tracks(detail);
                    }
                }
            }
            "now-playing" => self.present_now_playing_sheet(),
            "settings" => self.present_settings(),
            "logs" => {
                self.present_settings();
                self.present_logs();
            }
            "sync-conflict-group" => self.present_conflicts(),
            _ => {}
        }
    }

    fn first_playlist_id(&self) -> Option<String> {
        self.inner
            .session
            .borrow()
            .playlist_rows()
            .ok()?
            .into_iter()
            .next()
            .map(|row| row.id)
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

        let picker = gtk::Box::new(gtk::Orientation::Vertical, 12);
        picker.set_margin_top(24);
        picker.set_margin_bottom(24);
        picker.set_margin_start(18);
        picker.set_margin_end(18);
        picker.set_widget_name(route_id(MusicScreen::FolderPicker));
        picker.append(&status_row(&StatusRowData {
            message: "Choose a Folder containing local audio files.".into(),
            severity: StatusSeverity::Info,
        }));
        let choose = primary_action("Choose Folder");
        picker.append(&choose);

        let conflict_banner = banner("Sync Conflicts need review");
        conflict_banner.set_revealed(false);
        conflict_banner.set_button_label(Some("Review"));
        let (songs_scroll, songs_list) = flush_media_list();
        songs_list.set_selection_mode(gtk::SelectionMode::Single);
        let songs = gtk::Box::new(gtk::Orientation::Vertical, 0);
        songs.set_hexpand(true);
        songs.set_vexpand(true);
        songs.set_widget_name(route_id(MusicScreen::Library));
        songs.append(&conflict_banner);
        songs.append(&songs_scroll);

        let (artists_scroll, artists_list) = flush_media_list();
        let artists_root = page("Artists", &artists_scroll, PageChrome::root());
        artists_root.set_tag(Some("root"));
        let artists_nav = navigation_view();
        artists_nav.set_hexpand(true);
        artists_nav.set_vexpand(true);
        artists_nav.add(&artists_root);

        let (albums_scroll, albums_list) = flush_media_list();
        let albums_root = page("Albums", &albums_scroll, PageChrome::root());
        albums_root.set_tag(Some("root"));
        let albums_nav = navigation_view();
        albums_nav.set_hexpand(true);
        albums_nav.set_vexpand(true);
        albums_nav.add(&albums_root);

        let (playlist_scroll, playlist_list) = flush_media_list();
        playlist_scroll.set_widget_name(route_id(MusicScreen::PlaylistList));
        let playlist_empty = empty_state(
            EmptyKind::EmptyFolder,
            &EmptyCopy {
                title: "No playlists".into(),
                description: Some("Create a playlist to group local tracks.".into()),
                action: None,
            },
        );
        let playlist_pages = gtk::Stack::new();
        playlist_pages.add_named(&playlist_scroll, Some("list"));
        playlist_pages.add_named(&playlist_empty.page, Some("empty"));
        let playlists_root = page("Playlists", &playlist_pages, PageChrome::root());
        playlists_root.set_tag(Some("root"));
        let playlists_nav = navigation_view();
        playlists_nav.set_hexpand(true);
        playlists_nav.set_vexpand(true);
        playlists_nav.add(&playlists_root);

        let now_playing = gtk::Box::new(gtk::Orientation::Vertical, 12);
        now_playing.set_widget_name(route_id(MusicScreen::NowPlaying));
        now_playing.set_valign(gtk::Align::Center);
        now_playing.set_halign(gtk::Align::Fill);
        now_playing.set_hexpand(true);
        now_playing.set_vexpand(true);
        now_playing.set_margin_top(24);
        now_playing.set_margin_bottom(24);
        now_playing.set_margin_start(24);
        now_playing.set_margin_end(24);
        let artwork = gtk::Image::from_icon_name("audio-x-generic-symbolic");
        artwork.set_pixel_size(240);
        let now_title = gtk::Label::new(Some("Nothing Playing"));
        now_title.add_css_class("title-1");
        now_title.set_wrap(true);
        now_title.set_justify(gtk::Justification::Center);
        let now_badge = gtk::Label::new(None);
        now_badge.add_css_class("dim-label");
        now_badge.set_wrap(true);
        now_badge.set_justify(gtk::Justification::Center);
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        controls.set_halign(gtk::Align::Center);
        let previous = gtk::Button::from_icon_name("media-skip-backward-symbolic");
        let play_pause = primary_action("Play");
        let next = gtk::Button::from_icon_name("media-skip-forward-symbolic");
        let stop = gtk::Button::from_icon_name("media-playback-stop-symbolic");
        controls.append(&previous);
        controls.append(&play_pause);
        controls.append(&next);
        controls.append(&stop);
        now_playing.append(&artwork);
        now_playing.append(&now_title);
        now_playing.append(&now_badge);
        now_playing.append(&controls);

        let now_column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        now_column.add_css_class("background");
        now_column.set_hexpand(false);
        now_column.set_vexpand(true);
        now_column.set_width_request(NOW_PLAYING_WIDTH);
        now_column.append(&now_playing);

        let mini_art = gtk::Image::from_icon_name("audio-x-generic-symbolic");
        mini_art.set_pixel_size(40);
        let mini_title = gtk::Label::new(Some("Nothing Playing"));
        mini_title.add_css_class("heading");
        mini_title.set_xalign(0.0);
        mini_title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        mini_title.set_hexpand(true);
        let mini_badge = gtk::Label::new(None);
        mini_badge.add_css_class("dim-label");
        mini_badge.add_css_class("caption");
        mini_badge.set_xalign(0.0);
        mini_badge.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let mini_copy = gtk::Box::new(gtk::Orientation::Vertical, 0);
        mini_copy.set_hexpand(true);
        mini_copy.set_valign(gtk::Align::Center);
        mini_copy.append(&mini_title);
        mini_copy.append(&mini_badge);
        let mini_play = gtk::Button::from_icon_name("media-playback-start-symbolic");
        mini_play.set_tooltip_text(Some("Play"));
        mini_play.add_css_class("flat");
        let mini_next = gtk::Button::from_icon_name("media-skip-forward-symbolic");
        mini_next.set_tooltip_text(Some("Next"));
        mini_next.add_css_class("flat");
        let mini_player = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        mini_player.set_margin_top(6);
        mini_player.set_margin_bottom(6);
        mini_player.set_margin_start(10);
        mini_player.set_margin_end(10);
        mini_player.add_css_class("toolbar");
        mini_player.set_visible(false);
        mini_player.append(&mini_art);
        mini_player.append(&mini_copy);
        mini_player.append(&mini_play);
        mini_player.append(&mini_next);
        let mini_open = gtk::GestureClick::new();
        mini_copy.add_controller(mini_open.clone());

        let sort = choice_dropdown(&ChoiceData {
            labels: vec![
                "Title".into(),
                "Artist".into(),
                "Album".into(),
                "Duration".into(),
            ],
            selected: 0,
        });
        sort.set_tooltip_text(Some("Sort Songs"));
        let add_playlist = header_action("list-add-symbolic", "Create Playlist");
        add_playlist.set_visible(false);

        let shell = adaptive_shell(
            APP_TITLE,
            &[
                RootPage {
                    id: "songs",
                    title: "Songs",
                    icon: "audio-x-generic-symbolic",
                    child: songs.upcast(),
                },
                RootPage {
                    id: "artists",
                    title: "Artists",
                    icon: "system-users-symbolic",
                    child: artists_nav.clone().upcast(),
                },
                RootPage {
                    id: "albums",
                    title: "Albums",
                    icon: "media-optical-symbolic",
                    child: albums_nav.clone().upcast(),
                },
                RootPage {
                    id: "playlists",
                    title: "Playlists",
                    icon: "view-list-symbolic",
                    child: playlists_nav.clone().upcast(),
                },
            ],
        );
        shell.stack.set_hexpand(true);
        shell.stack.set_vexpand(true);
        shell.stack.set_halign(gtk::Align::Fill);
        shell.stack.set_valign(gtk::Align::Fill);
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
        let search = search_entry();
        search.set_placeholder_text(Some("Title, album, artist, or playlist"));
        search.set_hexpand(true);
        let search_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        search_box.set_margin_top(6);
        search_box.set_margin_bottom(6);
        search_box.set_margin_start(8);
        search_box.set_margin_end(8);
        search_box.append(&search);
        let chrome_progress = chrome_progress();
        shell.header.pack_start(&chrome_progress.root);
        shell.header.pack_start(&sort);
        shell.header.pack_start(&add_playlist);
        shell.header.pack_end(&menu.button);
        shell.toolbar.add_top_bar(&search_box);
        let (search_scroll, search_results) = list_box_page();
        let content_pages = gtk::Stack::new();
        content_pages.set_hexpand(true);
        content_pages.set_vexpand(true);
        content_pages.set_halign(gtk::Align::Fill);
        content_pages.set_valign(gtk::Align::Fill);
        content_pages.set_width_request(320);
        shell.toolbar.set_content(gtk::Widget::NONE);
        content_pages.add_named(&shell.stack, Some("tabs"));
        content_pages.add_named(&search_scroll, Some("search"));
        content_pages.set_visible_child_name("tabs");
        shell.stack.set_visible(true);
        let stage = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        stage.set_hexpand(true);
        stage.set_vexpand(true);
        stage.append(&content_pages);
        stage.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        stage.append(&now_column);
        shell.toolbar.set_content(Some(&stage));
        shell.toolbar.remove(&shell.switcher_bar);
        shell.toolbar.add_bottom_bar(&mini_player);
        shell.toolbar.add_bottom_bar(&shell.switcher_bar);

        let root_stack = gtk::Stack::new();
        root_stack.add_named(&picker, Some(route_id(MusicScreen::FolderPicker)));
        root_stack.add_named(&shell.toolbar, Some("music"));
        root_stack.set_visible_child_name(route_id(MusicScreen::FolderPicker));
        let toast = adw::ToastOverlay::new();
        toast.set_child(Some(&root_stack));
        window.set_content(Some(&toast));
        shell.install(&window);
        let compact_stage = adw::Breakpoint::new(
            adw::BreakpointCondition::parse("max-width: 860sp").expect("stage collapse"),
        );
        compact_stage.add_setter(&now_column, "visible", Some(&false.to_value()));
        compact_stage.add_setter(&mini_player, "visible", Some(&true.to_value()));
        window.add_breakpoint(compact_stage);
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
        let session = Session::new(
            StdVfs::new(TEMP_PREFIX),
            device,
            system_transport(),
            DIAGNOSTIC_CAPACITY,
        );
        let inner = Rc::new(Inner {
            window: window.clone(),
            shell,
            root_stack,
            songs_list: songs_list.clone(),
            artists_list: artists_list.clone(),
            albums_list: albums_list.clone(),
            playlist_list: playlist_list.clone(),
            playlist_pages,
            artists_nav: artists_nav.clone(),
            albums_nav: albums_nav.clone(),
            playlists_nav: playlists_nav.clone(),
            sort: sort.clone(),
            add_playlist: add_playlist.clone(),
            now_column: now_column.clone(),
            menu: menu.clone(),
            settings_dialog: RefCell::new(None),
            conflict_banner: conflict_banner.clone(),
            toast,
            now_title,
            now_badge,
            now_art: artwork,
            play_pause: play_pause.clone(),
            mini_title,
            mini_badge,
            mini_art,
            mini_play: mini_play.clone(),
            query: RefCell::new(String::new()),
            sort_option: Cell::new(SortOption::Title),
            artwork: RefCell::new(HashMap::new()),
            search_entry: search.clone(),
            search_results: search_results.clone(),
            content_pages,
            drill: RefCell::new(Drill::None),
            drill_list: RefCell::new(None),
            session: RefCell::new(session),
            paths,
            mpris: RefCell::new(None),
            chrome_progress,
            work: Arc::new(Mutex::new(None)),
            work_rx: RefCell::new(None),
            work_busy: Cell::new(false),
            work_cancel: RefCell::new(None),
            work_tick: Cell::new(false),
            scan_progress_row: RefCell::new(None),
        });
        let this = Self { inner };

        let picker = this.clone();
        choose.connect_clicked(move |_| picker.pick_folder());
        let searched = this.clone();
        search.connect_search_changed(move |entry| {
            searched.inner.query.replace(entry.text().to_string());
            searched.refill_search();
        });
        let hits = this.clone();
        search_results.connect_row_activated(move |_, row| {
            let id = row.widget_name();
            if !id.is_empty() {
                hits.activate_search_hit(&id);
            }
        });
        let sorted = this.clone();
        sort.connect_selected_notify(move |dropdown| {
            let option = match dropdown.selected() {
                1 => SortOption::Artist,
                2 => SortOption::Album,
                3 => SortOption::Duration,
                _ => SortOption::Title,
            };
            sorted.inner.sort_option.set(option);
            sorted.refill_songs();
        });
        let played = this.clone();
        songs_list.connect_row_activated(move |list, row| {
            let id = row.widget_name();
            if !id.is_empty() {
                played.play_from_list(list, &id);
            }
        });
        let artists = this.clone();
        artists_list.connect_row_activated(move |_, row| {
            let id = row.widget_name();
            if !id.is_empty() {
                artists.open_artist(&id);
            }
        });
        let albums = this.clone();
        albums_list.connect_row_activated(move |_, row| {
            let id = row.widget_name();
            if !id.is_empty() {
                albums.open_album(&id);
            }
        });
        let opened = this.clone();
        playlist_list.connect_row_activated(move |_, row| {
            let id = row.widget_name();
            if !id.is_empty() {
                opened.open_playlist_detail(&id);
            }
        });
        let created = this.clone();
        add_playlist.connect_clicked(move |_| created.present_create_playlist());
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
            .connect_clicked(move |_| cancel.cancel_library_work());
        if let Some(choose_folder) = menu.extra("choose-folder") {
            let chooser = this.clone();
            choose_folder.connect_activate(move |_, _| chooser.pick_folder());
        }
        let popped = this.clone();
        artists_nav.connect_popped(move |_, _| popped.on_nav_popped());
        let popped = this.clone();
        albums_nav.connect_popped(move |_, _| popped.on_nav_popped());
        let popped = this.clone();
        playlists_nav.connect_popped(move |_, _| popped.on_nav_popped());
        let tabs = this.clone();
        this.inner
            .shell
            .stack
            .connect_visible_child_notify(move |_| tabs.sync_chrome());
        let conflicts = this.clone();
        conflict_banner.connect_button_clicked(move |_| conflicts.present_conflicts());
        let previous_window = this.clone();
        previous.connect_clicked(move |_| {
            previous_window.playback_command(|session| session.previous())
        });
        let toggled = this.clone();
        play_pause
            .connect_clicked(move |_| toggled.playback_command(|session| session.play_pause()));
        let next_window = this.clone();
        next.connect_clicked(move |_| next_window.playback_command(|session| session.next_track()));
        let stopped = this.clone();
        stop.connect_clicked(move |_| stopped.playback_command(|session| session.stop()));
        let mini_toggled = this.clone();
        mini_play.connect_clicked(move |_| {
            mini_toggled.playback_command(|session| session.play_pause())
        });
        let mini_next_window = this.clone();
        mini_next.connect_clicked(move |_| {
            mini_next_window.playback_command(|session| session.next_track())
        });
        let opened_now = this.clone();
        mini_open.connect_released(move |_, _, _, _| opened_now.present_now_playing_sheet());
        let keys = this.clone();
        let key = gtk::EventControllerKey::new();
        key.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gdk::Key::space && !keys.inner.search_entry.has_focus() {
                keys.playback_command(|session| session.play_pause());
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        window.add_controller(key);

        this.record(LogLevel::Info, "app", "Application started");
        this.start_mpris();
        this.refill_settings();
        this
    }

    fn start_mpris(&self) {
        let weak = Rc::downgrade(&self.inner);
        let command = Rc::new(move |command| {
            if let Some(inner) = weak.upgrade() {
                Window { inner }.handle_remote(command);
            }
        });
        let weak = Rc::downgrade(&self.inner);
        let state = Rc::new(move || {
            let Some(inner) = weak.upgrade() else {
                return MprisState::default();
            };
            let session = inner.session.borrow();
            MprisState {
                transport: session.transport_snapshot(),
                title: session.current_item().and_then(|item| item.label.clone()),
            }
        });
        self.inner
            .mpris
            .replace(Some(MprisHost::start(command, state)));
        self.record(LogLevel::Info, "mpris", "MPRIS host requested");
    }

    fn handle_remote(&self, command: RemoteCommand) {
        match command {
            RemoteCommand::Raise => self.inner.window.present(),
            RemoteCommand::Quit => self.inner.window.close(),
            RemoteCommand::Next => self.playback_command(|session| session.next_track()),
            RemoteCommand::Previous => self.playback_command(|session| session.previous()),
            RemoteCommand::Pause => self.playback_command(|session| session.pause()),
            RemoteCommand::PlayPause => self.playback_command(|session| session.play_pause()),
            RemoteCommand::Stop => self.playback_command(|session| session.stop()),
            RemoteCommand::Play => self.playback_command(|session| session.play()),
            RemoteCommand::Seek(offset) => {
                self.playback_command(|session| session.seek_relative(offset))
            }
            RemoteCommand::SetPosition(position) => {
                self.playback_command(|session| session.set_position(position))
            }
            RemoteCommand::SetVolume(volume) => {
                self.playback_command(|session| session.set_volume(volume))
            }
        }
    }

    fn load_persisted(&self, wait: bool) {
        if let Some(folder) = self.inner.paths.load_folder() {
            if std::path::Path::new(&folder).is_dir() {
                self.open_folder(&folder, wait);
            } else {
                self.record(LogLevel::Warning, "folder", "Saved Folder is unavailable");
            }
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
                Ok(file) => match file
                    .path()
                    .and_then(|path| path.to_str().map(str::to_owned))
                {
                    Some(path) => this.open_folder(&path, false),
                    None => this.toast("The selected Folder is not a local UTF-8 path"),
                },
                Err(error) => {
                    let message = error.to_string();
                    if !message.to_lowercase().contains("dismissed") {
                        this.report("folder", "Folder picker failed", &message);
                    }
                }
            },
        );
    }

    fn open_folder(&self, folder: &str, wait: bool) {
        let _span = localcore_trace::span_always("music", "window.open_folder");
        self.start_library_work(folder.to_string(), None);
        if wait {
            self.drain_library_work();
        }
    }

    fn reload_folder(&self) {
        let Some(folder) = self.inner.session.borrow().folder().map(str::to_owned) else {
            self.toast("Choose a Folder first");
            return;
        };
        let store = self.inner.session.borrow().clone_store();
        self.start_library_work(folder, store);
    }

    fn start_library_work(&self, folder: String, existing: Option<Store>) {
        if self.inner.work_busy.get() {
            self.cancel_library_work();
        }
        self.inner.work_busy.set(true);
        if let Ok(mut work) = self.inner.work.lock() {
            *work = Some(WorkProgress::new("Scanning"));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.inner.work_cancel.replace(Some(cancel.clone()));
        let status = self.inner.work.clone();
        let (tx, rx) = mpsc::channel();
        self.inner.work_rx.replace(Some(rx));
        let vfs = match ConfinedVfs::new(TEMP_PREFIX, &folder) {
            Ok(vfs) => vfs,
            Err(error) => {
                let _ = tx.send(LibraryWork::Failed(error.to_string()));
                return;
            }
        };
        thread::spawn(move || {
            let report = |discovered: usize| {
                if let Ok(mut guard) = status.lock() {
                    if let Some(progress) = guard.as_mut() {
                        progress.update("Scanning", Some(found_detail(discovered as u64)), None);
                    }
                }
            };
            let cancelled = || cancel.load(Ordering::Relaxed);
            let result = match existing {
                Some(mut store) => store
                    .reload_with_hooks(&vfs, Some(&report), Some(&cancelled))
                    .map(|done| done.map(|()| store)),
                None => Store::open_with_hooks(&vfs, &folder, Some(&report), Some(&cancelled)),
            };
            let outcome = match result {
                Ok(Some(store)) => LibraryWork::Ready { folder, store },
                Ok(None) => LibraryWork::Cancelled,
                Err(error) => LibraryWork::Failed(error.to_string()),
            };
            let _ = tx.send(outcome);
        });
        self.ensure_work_tick();
        self.sync_chrome_progress();
    }

    fn cancel_library_work(&self) {
        if let Some(flag) = self.inner.work_cancel.borrow().as_ref() {
            flag.store(true, Ordering::Relaxed);
        }
    }

    fn drain_library_work(&self) {
        let ctx = gtk::glib::MainContext::default();
        while self.inner.work_busy.get() {
            self.poll_library_work();
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
            this.poll_library_work();
            this.sync_chrome_progress();
            if this.inner.work_busy.get() {
                gtk::glib::ControlFlow::Continue
            } else {
                this.inner.work_tick.set(false);
                gtk::glib::ControlFlow::Break
            }
        });
    }

    fn poll_library_work(&self) {
        let Some(rx) = self.inner.work_rx.borrow_mut().take() else {
            return;
        };
        match rx.try_recv() {
            Ok(LibraryWork::Ready { folder, store }) => {
                let vfs = match ConfinedVfs::new(TEMP_PREFIX, &folder) {
                    Ok(vfs) => vfs,
                    Err(error) => {
                        self.inner.work_busy.set(false);
                        if let Ok(mut work) = self.inner.work.lock() {
                            *work = None;
                        }
                        self.inner.work_cancel.replace(None);
                        self.sync_chrome_progress();
                        self.record(LogLevel::Error, "folder", "Could not confine music folder");
                        self.toast(&error.to_string());
                        return;
                    }
                };
                self.inner.paths.save_folder(&folder);
                {
                    let mut session = self.inner.session.borrow_mut();
                    session.replace_vfs(vfs);
                    session.install_store(store, folder);
                }
                self.inner.root_stack.set_visible_child_name("music");
                self.inner.work_busy.set(false);
                if self.inner.session.borrow().pending_metadata_count() == 0 {
                    if let Ok(mut work) = self.inner.work.lock() {
                        *work = None;
                    }
                    self.inner.work_cancel.replace(None);
                    self.sync_chrome_progress();
                }
                self.apply_host_metadata();
                self.refill_all();
            }
            Ok(LibraryWork::Cancelled) => {
                self.inner.work_busy.set(false);
                if let Ok(mut work) = self.inner.work.lock() {
                    *work = None;
                }
                self.inner.work_cancel.replace(None);
                self.sync_chrome_progress();
                self.toast("Reload cancelled");
            }
            Ok(LibraryWork::Failed(error)) => {
                self.inner.work_busy.set(false);
                if let Ok(mut work) = self.inner.work.lock() {
                    *work = None;
                }
                self.inner.work_cancel.replace(None);
                self.sync_chrome_progress();
                self.report("folder", "Could not open Folder", &error);
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.inner.work_rx.replace(Some(rx));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.inner.work_busy.set(false);
            }
        }
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
                .unwrap_or_else(idle_music_progress);
            apply_progress_row(&row, &ProgressRowData::from(&display));
        }
    }

    fn update_metadata_progress(&self) {
        let pending = self.inner.session.borrow().pending_metadata_count();
        let total = self.inner.session.borrow().track_count();
        let done = total.saturating_sub(pending);
        if let Ok(mut guard) = self.inner.work.lock() {
            let progress = guard.get_or_insert_with(|| WorkProgress::new("Metadata"));
            let started = progress.started();
            let fraction = (total > 0).then(|| done as f64 / total as f64);
            progress.update(
                "Metadata",
                Some(count_detail(
                    done as u64,
                    total as u64,
                    started,
                    Instant::now(),
                )),
                fraction,
            );
        }
        self.sync_chrome_progress();
    }

    fn apply_host_metadata(&self) {
        localcore_trace::detail("music", "apply_host_metadata idle tick");
        let this = self.clone();
        gtk::glib::idle_add_local(move || {
            if this
                .inner
                .work_cancel
                .borrow()
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Relaxed))
            {
                if let Ok(mut work) = this.inner.work.lock() {
                    *work = None;
                }
                this.sync_chrome_progress();
                return gtk::glib::ControlFlow::Break;
            }
            let requests = match this.inner.session.borrow().pending_metadata(24) {
                Ok(requests) => requests,
                Err(_) => return gtk::glib::ControlFlow::Break,
            };
            if requests.is_empty() {
                if let Ok(mut work) = this.inner.work.lock() {
                    *work = None;
                }
                this.sync_chrome_progress();
                this.refill_browse();
                this.update_now_playing();
                return gtk::glib::ControlFlow::Break;
            }
            this.update_metadata_progress();
            let reads: Vec<_> = requests.iter().map(crate::metadata::read_or_mark).collect();
            for read in &reads {
                if let Some(bytes) = &read.artwork {
                    if let Ok(texture) =
                        gdk::Texture::from_bytes(&gtk::glib::Bytes::from(bytes.as_slice()))
                    {
                        this.inner
                            .artwork
                            .borrow_mut()
                            .insert(read.update.id.clone(), texture);
                    }
                }
            }
            let updates = reads.into_iter().map(|read| read.update).collect();
            if let Err(error) = this
                .inner
                .session
                .borrow_mut()
                .apply_metadata_batch(updates)
            {
                this.report(
                    "metadata",
                    "Could not apply song metadata",
                    &error.to_string(),
                );
                return gtk::glib::ControlFlow::Break;
            }
            gtk::glib::ControlFlow::Continue
        });
    }

    fn refill_all(&self) {
        let _span = localcore_trace::span_always("music", "refill_all");
        self.refill_browse();
        self.refill_settings();
        self.update_now_playing();
    }

    fn refill_browse(&self) {
        self.refill_songs();
        self.refill_artists();
        self.refill_albums();
        self.refill_playlists();
        self.refill_conflict_banner();
        self.refill_drill();
        self.sync_chrome();
    }

    fn refill_songs(&self) {
        let _span = localcore_trace::span_always("music", "refill_songs");
        clear_list(&self.inner.songs_list);
        let rows = self
            .inner
            .session
            .borrow_mut()
            .library_rows(String::new(), self.inner.sort_option.get());
        match rows {
            Ok(rows) => {
                if rows.state == LibraryContentState::EmptyFolder {
                    append_status(&self.inner.songs_list, "This Folder has no supported audio");
                } else {
                    for section in rows.sections {
                        let heading = text_row(&to_text_data(section.heading));
                        heading.set_activatable(false);
                        heading.add_css_class("heading");
                        self.inner.songs_list.append(&heading);
                        for item in section.items {
                            self.append_track(&self.inner.songs_list, &item);
                        }
                    }
                }
                if let Ok(issues) = self.inner.session.borrow().scan_issue_rows() {
                    for issue in issues {
                        self.inner.songs_list.append(&status_row(&StatusRowData {
                            message: issue.message,
                            severity: to_status_severity(issue.severity),
                        }));
                    }
                }
            }
            Err(error) => self.report(
                "library",
                "Could not project library rows",
                &error.to_string(),
            ),
        }
        self.mark_playing();
    }

    fn refill_artists(&self) {
        clear_list(&self.inner.artists_list);
        match self.inner.session.borrow().artist_rows() {
            Ok(rows) if rows.is_empty() => {
                append_status(&self.inner.artists_list, "No artists in this Folder")
            }
            Ok(rows) => {
                for row in rows {
                    let id = row.id.clone();
                    let texture = self
                        .inner
                        .session
                        .borrow()
                        .artist_art_track(&id)
                        .and_then(|track| self.texture_for(&track));
                    self.append_location(
                        &self.inner.artists_list,
                        row,
                        texture,
                        "system-users-symbolic",
                    );
                }
            }
            Err(error) => self.report("library", "Could not list artists", &error.to_string()),
        }
    }

    fn refill_albums(&self) {
        clear_list(&self.inner.albums_list);
        match self.inner.session.borrow().album_rows() {
            Ok(rows) if rows.is_empty() => {
                append_status(&self.inner.albums_list, "No albums in this Folder")
            }
            Ok(rows) => {
                for row in rows {
                    let id = row.id.clone();
                    let texture = self
                        .inner
                        .session
                        .borrow()
                        .album_art_track(&id)
                        .and_then(|track| self.texture_for(&track));
                    self.append_location(
                        &self.inner.albums_list,
                        row,
                        texture,
                        "media-optical-symbolic",
                    );
                }
            }
            Err(error) => self.report("library", "Could not list albums", &error.to_string()),
        }
    }

    fn append_location(
        &self,
        list: &gtk::ListBox,
        row: music_core::TextRow,
        texture: Option<gdk::Texture>,
        symbol: &str,
    ) {
        let id = row.id.clone();
        let widget = text_row(&TextRowData {
            title: row.title,
            subtitle: row.subtitle,
            trailing: row.trailing,
            leading: Some(if let Some(texture) = texture {
                Leading::Avatar {
                    text: String::new(),
                    texture: Some(texture),
                }
            } else {
                Leading::Symbol(symbol.into())
            }),
        });
        widget.set_activatable(true);
        widget.set_widget_name(&id);
        list.append(&widget);
    }

    fn append_track(&self, list: &gtk::ListBox, item: &music_core::MediaItem) {
        self.append_track_row(list, item, None);
    }

    fn append_track_row(
        &self,
        list: &gtk::ListBox,
        item: &music_core::MediaItem,
        playlist: Option<PlaylistEntryRef>,
    ) {
        let id = item.id.clone();
        let row = media_item(&to_media_data(item.clone(), self.texture_for(&id)));
        row.set_activatable(true);
        row.set_widget_name(&id);
        self.attach_track_context_menu(&row, Some(id), playlist);
        list.append(&row);
    }

    fn texture_for(&self, track_id: &str) -> Option<gdk::Texture> {
        self.inner.artwork.borrow().get(track_id).cloned()
    }

    fn show_tab(&self, id: &str) {
        self.inner.shell.stack.set_visible_child_name(id);
        self.inner.content_pages.set_visible_child_name("tabs");
        self.sync_chrome();
    }

    fn visible_tab(&self) -> String {
        self.inner
            .shell
            .stack
            .visible_child_name()
            .map(|name| name.to_string())
            .unwrap_or_else(|| "songs".into())
    }

    fn visible_nav_pushed(&self) -> bool {
        match self.visible_tab().as_str() {
            "artists" => nav_is_pushed(&self.inner.artists_nav),
            "albums" => nav_is_pushed(&self.inner.albums_nav),
            "playlists" => nav_is_pushed(&self.inner.playlists_nav),
            _ => false,
        }
    }

    fn sync_chrome(&self) {
        let pushed = self.visible_nav_pushed();
        self.inner.shell.header.set_visible(!pushed);
        let tab = self.visible_tab();
        self.inner.sort.set_visible(!pushed && tab == "songs");
        self.inner
            .add_playlist
            .set_visible(!pushed && tab == "playlists");
    }

    fn on_nav_popped(&self) {
        if !self.visible_nav_pushed() {
            self.inner.drill.replace(Drill::None);
            self.inner.drill_list.replace(None);
        }
        self.sync_chrome();
    }

    fn open_artist(&self, id: &str) {
        self.show_tab("artists");
        let items = match self.inner.session.borrow().artist_tracks(id) {
            Ok(items) => items,
            Err(error) => {
                self.report("library", "Could not open this artist", &error.to_string());
                return;
            }
        };
        pop_to_root(&self.inner.artists_nav);
        self.inner.drill.replace(Drill::Artist(id.to_owned()));
        self.push_tracks(
            &self.inner.artists_nav,
            &location_title(id),
            items,
            PlayAll::Artist(id.to_owned()),
            None,
        );
    }

    fn open_album(&self, id: &str) {
        self.show_tab("albums");
        let items = match self.inner.session.borrow().album_tracks(id) {
            Ok(items) => items,
            Err(error) => {
                self.report("library", "Could not open this album", &error.to_string());
                return;
            }
        };
        pop_to_root(&self.inner.albums_nav);
        self.inner.drill.replace(Drill::Album(id.to_owned()));
        self.push_tracks(
            &self.inner.albums_nav,
            &location_title(id),
            items,
            PlayAll::Album(id.to_owned()),
            None,
        );
    }

    fn refill_drill(&self) {
        let drill = self.inner.drill.borrow().clone();
        match drill {
            Drill::None => {}
            Drill::Artist(id) => self.open_artist(&id),
            Drill::Album(id) => self.open_album(&id),
            Drill::Playlist(id) => self.open_playlist_detail(&id),
        }
    }

    fn push_tracks(
        &self,
        nav: &adw::NavigationView,
        title: &str,
        items: Vec<music_core::MediaItem>,
        play_all: PlayAll,
        header_end: Option<gtk::Widget>,
    ) {
        let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
        column.set_margin_top(12);
        column.set_hexpand(true);
        column.set_vexpand(true);
        if !items.is_empty() {
            let play = pill_primary("Play All");
            play.set_halign(gtk::Align::Center);
            let this = self.clone();
            play.connect_clicked(move |_| this.play_all(&play_all));
            column.append(&play);
        }
        let (scroll, list) = flush_media_list();
        list.set_selection_mode(gtk::SelectionMode::Single);
        if items.is_empty() {
            append_status(&list, "No tracks in this location");
        } else {
            for item in &items {
                self.append_track(&list, item);
            }
        }
        let played = self.clone();
        list.connect_row_activated(move |list, row| {
            let id = row.widget_name();
            if !id.is_empty() {
                played.play_from_list(list, &id);
            }
        });
        column.append(&scroll);
        let page = page(
            title,
            &column,
            PageChrome {
                start: None,
                end: header_end,
                root: false,
            },
        );
        self.inner.drill_list.replace(Some(list));
        nav.push(&page);
        self.mark_playing();
        self.sync_chrome();
    }

    fn play_all(&self, target: &PlayAll) {
        let result = {
            let mut session = self.inner.session.borrow_mut();
            match target {
                PlayAll::Artist(id) => session.play_artist(id),
                PlayAll::Album(id) => session.play_album(id),
                PlayAll::Playlist(id) => session.play_playlist(id),
            }
        };
        match result {
            Ok(()) => self.update_now_playing(),
            Err(error) => self.report("playback", "Could not play", &error.to_string()),
        }
    }

    fn refill_search(&self) {
        let _span = localcore_trace::span("music", "refill_search");
        let query = self.inner.query.borrow().clone();
        clear_list(&self.inner.search_results);
        if query.trim().is_empty() {
            self.inner.content_pages.set_visible_child_name("tabs");
            return;
        }
        self.inner.content_pages.set_visible_child_name("search");
        match self.inner.session.borrow().search_hits(&query) {
            Ok(hits) if hits.is_empty() => {
                append_status(&self.inner.search_results, "No Results");
            }
            Ok(hits) => {
                for hit in hits {
                    let value = hit.subtitle.as_deref().unwrap_or(&hit.title);
                    let subtitle = format!(
                        "{}: {}",
                        gtk::glib::markup_escape_text(hit.kind.label()),
                        highlight_markup(value, &query)
                    );
                    let row = search_hit_row(&hit.title, &subtitle, hit.kind.symbol());
                    row.set_widget_name(&hit.id);
                    if matches!(hit.kind, music_core::SearchKind::Track) {
                        self.attach_track_context_menu(&row, Some(hit.id.clone()), None);
                    }
                    self.inner.search_results.append(&row);
                }
            }
            Err(error) => self.report("search", "Could not search", &error.to_string()),
        }
    }

    fn activate_search_hit(&self, id: &str) {
        self.inner.search_entry.set_text("");
        self.inner.content_pages.set_visible_child_name("tabs");
        if id.starts_with("album:") {
            self.open_album(id);
        } else if id.starts_with("artist:") {
            self.open_artist(id);
        } else if self
            .inner
            .session
            .borrow()
            .playlist_rows()
            .ok()
            .is_some_and(|rows| rows.iter().any(|row| row.id == id))
        {
            self.open_playlist_detail(id);
        } else {
            self.play_track(id);
        }
    }

    fn refill_conflict_banner(&self) {
        let result = self.inner.session.borrow().conflict_rows();
        match result {
            Ok(rows) => self.inner.conflict_banner.set_revealed(!rows.is_empty()),
            Err(error) => self.report(
                "conflict",
                "Could not inspect Sync Conflicts",
                &error.to_string(),
            ),
        }
    }

    fn play_track(&self, id: &str) {
        let result = self.inner.session.borrow_mut().play_track(id);
        match result {
            Ok(()) => self.update_now_playing(),
            Err(error) => self.report("playback", "Could not play track", &error.to_string()),
        }
    }

    fn play_from_list(&self, list: &gtk::ListBox, start_id: &str) {
        let ids = track_ids_in(list);
        let result = self
            .inner
            .session
            .borrow_mut()
            .play_ids_from(&ids, start_id);
        match result {
            Ok(()) => self.update_now_playing(),
            Err(error) => self.report("playback", "Could not play track", &error.to_string()),
        }
    }

    fn playback_command(&self, action: impl FnOnce(&mut AppSession) -> Result<(), ShellError>) {
        let result = {
            let mut session = self.inner.session.borrow_mut();
            action(&mut session)
        };
        match result {
            Ok(()) => self.update_now_playing(),
            Err(error) => self.report("playback", "Playback command failed", &error.to_string()),
        }
    }

    fn update_now_playing(&self) {
        let session = self.inner.session.borrow();
        if let Some(item) = session.current_item() {
            let title = item.label.as_deref().unwrap_or("Unknown Track");
            let badge = item.badge.as_deref().unwrap_or_default();
            self.inner.now_title.set_label(title);
            self.inner.now_badge.set_label(badge);
            self.inner.mini_title.set_label(title);
            self.inner.mini_badge.set_label(badge);
            if let Some(texture) = self.inner.artwork.borrow().get(&item.id) {
                self.inner.now_art.set_paintable(Some(texture));
                self.inner.mini_art.set_paintable(Some(texture));
            } else {
                self.inner
                    .now_art
                    .set_icon_name(Some("audio-x-generic-symbolic"));
                self.inner
                    .mini_art
                    .set_icon_name(Some("audio-x-generic-symbolic"));
            }
        } else {
            self.inner.now_title.set_label("Nothing Playing");
            self.inner.now_badge.set_label("");
            self.inner.mini_title.set_label("Nothing Playing");
            self.inner.mini_badge.set_label("");
            self.inner
                .now_art
                .set_icon_name(Some("audio-x-generic-symbolic"));
            self.inner
                .mini_art
                .set_icon_name(Some("audio-x-generic-symbolic"));
        }
        let playing = session.transport_snapshot().status == crate::PlaybackStatus::Playing;
        self.inner
            .play_pause
            .set_label(if playing { "Pause" } else { "Play" });
        self.inner.mini_play.set_icon_name(if playing {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        });
        self.inner
            .mini_play
            .set_tooltip_text(Some(if playing { "Pause" } else { "Play" }));
        drop(session);
        self.mark_playing();
        if let Some(mpris) = self.inner.mpris.borrow().as_ref() {
            mpris.notify_player();
        }
    }

    fn mark_playing(&self) {
        self.mark_playing_in(&self.inner.songs_list);
        if let Some(list) = self.inner.drill_list.borrow().clone() {
            self.mark_playing_in(&list);
        }
    }

    fn mark_playing_in(&self, list: &gtk::ListBox) {
        let playing = self
            .inner
            .session
            .borrow()
            .current_item()
            .map(|item| item.id.clone());
        let mut child = list.first_child();
        while let Some(widget) = child {
            let next = widget.next_sibling();
            if let Ok(row) = widget.downcast::<gtk::ListBoxRow>() {
                let id = row.widget_name();
                let is_playing = playing.as_deref() == Some(id.as_str()) && !id.is_empty();
                if is_playing {
                    row.add_css_class("accent");
                    list.select_row(Some(&row));
                } else {
                    row.remove_css_class("accent");
                }
            }
            child = next;
        }
    }

    fn present_now_playing_sheet(&self) {
        self.inner.now_column.set_visible(true);
    }

    fn refill_playlists(&self) {
        clear_list(&self.inner.playlist_list);
        match self.inner.session.borrow().playlist_rows() {
            Ok(rows) if rows.is_empty() => {
                self.inner.playlist_pages.set_visible_child_name("empty");
            }
            Ok(rows) => {
                self.inner.playlist_pages.set_visible_child_name("list");
                for row in rows {
                    let id = row.id.clone();
                    let texture = self
                        .inner
                        .session
                        .borrow()
                        .playlist_art_track(&id)
                        .and_then(|track| self.texture_for(&track));
                    self.append_location(
                        &self.inner.playlist_list,
                        row,
                        texture,
                        "view-list-symbolic",
                    );
                }
            }
            Err(error) => self.report("playlist", "Could not load playlists", &error.to_string()),
        }
    }

    fn present_create_playlist(&self) {
        let name = adw::EntryRow::builder().title("Name").build();
        let group = adw::PreferencesGroup::new();
        group.add(&name);
        let page = adw::PreferencesPage::new();
        page.add(&group);
        let created = form_sheet(FormSheet {
            title: "Create Playlist".into(),
            cancel: "Cancel".into(),
            confirm: "Save".into(),
            body: page.upcast(),
        });
        let dialog = created.dialog.clone();
        dialog.present(Some(&self.inner.window));
        let this = self.clone();
        created.confirm.connect_clicked(move |_| {
            let result = this
                .inner
                .session
                .borrow_mut()
                .create_playlist(name.text().to_string());
            match result {
                Ok(id) => {
                    dialog.close();
                    this.refill_playlists();
                    this.refill_settings();
                    this.open_playlist_detail(&id);
                }
                Err(error) => {
                    this.report("playlist", "Could not create playlist", &error.to_string())
                }
            }
        });
    }

    fn open_playlist_detail(&self, id: &str) {
        self.show_tab("playlists");
        let result = self.inner.session.borrow().playlist_detail(id);
        let detail = match result {
            Ok(detail) => detail,
            Err(error) => {
                self.report("playlist", "Could not open playlist", &error.to_string());
                return;
            }
        };
        pop_to_root(&self.inner.playlists_nav);
        self.inner.drill.replace(Drill::Playlist(id.to_owned()));

        let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
        column.set_margin_top(12);
        column.set_hexpand(true);
        column.set_vexpand(true);
        column.set_widget_name(route_id(MusicScreen::PlaylistDetail));
        if detail
            .actions
            .iter()
            .any(|action| action.id == "play-all" && action.enabled)
        {
            let play = pill_primary("Play All");
            play.set_halign(gtk::Align::Center);
            let this = self.clone();
            let playlist_id = detail.id.clone();
            play.connect_clicked(move |_| this.play_all(&PlayAll::Playlist(playlist_id.clone())));
            column.append(&play);
        }
        let count = if detail.entries.len() == 1 {
            "1 track".into()
        } else {
            format!("{} tracks", detail.entries.len())
        };
        let caption = gtk::Label::new(Some(&count));
        caption.add_css_class("caption-heading");
        caption.add_css_class("dim-label");
        caption.set_xalign(0.0);
        caption.set_margin_start(12);
        column.append(&caption);
        let (scroll, list) = flush_media_list();
        list.set_selection_mode(gtk::SelectionMode::Single);
        if detail.entries.is_empty() {
            append_status(&list, "This playlist is empty");
        } else {
            for entry in &detail.entries {
                let track_id = self
                    .inner
                    .session
                    .borrow()
                    .playlist_entry_track_id(&detail.id, &entry.id);
                if let Some(track_id) = track_id {
                    if let Some(item) = self.inner.session.borrow().track_item(&track_id) {
                        self.append_track_row(
                            &list,
                            &item,
                            Some(PlaylistEntryRef {
                                playlist_id: detail.id.clone(),
                                entry_id: entry.id.clone(),
                            }),
                        );
                        continue;
                    }
                }
                let row = text_row(&to_text_data(entry.clone()));
                row.set_activatable(false);
                self.attach_track_context_menu(
                    &row,
                    None,
                    Some(PlaylistEntryRef {
                        playlist_id: detail.id.clone(),
                        entry_id: entry.id.clone(),
                    }),
                );
                list.append(&row);
            }
        }
        let played = self.clone();
        list.connect_row_activated(move |list, row| {
            let id = row.widget_name();
            if !id.is_empty() {
                played.play_from_list(list, &id);
            }
        });
        column.append(&scroll);
        let header_end = self.playlist_header_end(&detail);
        let page = page(
            &detail.title,
            &column,
            PageChrome {
                start: None,
                end: Some(header_end.upcast()),
                root: false,
            },
        );
        page.set_widget_name(route_id(MusicScreen::PlaylistDetail));
        self.inner.drill_list.replace(Some(list));
        self.inner.playlists_nav.push(&page);
        self.mark_playing();
        self.sync_chrome();
    }

    fn playlist_header_end(&self, detail: &crate::PlaylistDetailRows) -> gtk::Box {
        let end = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        if detail
            .actions
            .iter()
            .any(|action| action.id == "add-tracks" && action.enabled)
        {
            let add = header_action("list-add-symbolic", "Add Tracks");
            let this = self.clone();
            let detail = detail.clone();
            add.connect_clicked(move |_| this.present_add_tracks(detail.clone()));
            end.append(&add);
        }
        let edit = header_action("document-edit-symbolic", "Edit");
        let this = self.clone();
        let detail_for_edit = detail.clone();
        edit.connect_clicked(move |_| this.present_edit_playlist(detail_for_edit.clone()));
        end.append(&edit);

        let menu = gio::Menu::new();
        menu.append(Some("Delete Playlist…"), Some("playlist.delete"));
        let overflow = overflow(&menu);
        overflow.set_tooltip_text(Some("Playlist Menu"));
        let group = gio::SimpleActionGroup::new();
        let delete = gio::SimpleAction::new("delete", None);
        let this = self.clone();
        let playlist_id = detail.id.clone();
        let token = detail.content_token.clone();
        delete.connect_activate(move |_, _| this.confirm_delete_playlist(&playlist_id, &token));
        group.add_action(&delete);
        overflow.insert_action_group("playlist", Some(&group));
        end.append(&overflow);
        end
    }

    fn present_about(&self) {
        about_dialog(crate::APP_ID, APP_TITLE, env!("CARGO_PKG_VERSION"))
            .present(Some(&self.inner.window));
    }

    fn present_settings(&self) {
        let dialog = preferences_dialog("Settings", settings_screen(self.settings_spec()));
        dialog.set_widget_name(route_id(MusicScreen::Settings));
        self.inner.settings_dialog.replace(Some(dialog.clone()));
        dialog.present(Some(&self.inner.window));
    }

    fn present_add_tracks(&self, detail: crate::PlaylistDetailRows) {
        let _span = localcore_trace::span_always("music", "present_add_tracks");
        let already_in = detail
            .entries
            .iter()
            .filter_map(|entry| {
                self.inner
                    .session
                    .borrow()
                    .playlist_entry_track_id(&detail.id, &entry.id)
            })
            .collect::<BTreeSet<_>>();
        let selected = Rc::new(RefCell::new(BTreeSet::<String>::new()));
        let built = list_screen(&ListScreen {
            search: true,
            filter: None,
            sections: vec![ListSection { heading: None }],
            primary: None,
            selection: None,
            banner: None,
            empty: Some(EmptyState {
                kind: EmptyKind::NoMatches,
                copy: EmptyCopy {
                    title: "No tracks match".into(),
                    description: Some("Try a different search.".into()),
                    action: None,
                },
            }),
        });
        built.root.set_hexpand(true);
        built.root.set_vexpand(true);
        built.root.set_widget_name(route_id(MusicScreen::AddTracks));
        if let Some(search) = &built.search {
            search.set_placeholder_text(Some("Search library"));
        }
        let list = built.lists[0].clone();
        let sheet = form_sheet(FormSheet {
            title: "Add Tracks".into(),
            cancel: "Cancel".into(),
            confirm: "Add".into(),
            body: built.root.clone().upcast(),
        });
        sheet.confirm.set_sensitive(false);
        self.fill_add_track_list(&list, "", &already_in, &selected, &built, &sheet.confirm);
        if let Some(search) = built.search.clone() {
            let this = self.clone();
            let list = list.clone();
            let already_in = already_in.clone();
            let selected = selected.clone();
            let built = built.clone();
            let confirm = sheet.confirm.clone();
            search.connect_search_changed(move |entry| {
                this.fill_add_track_list(
                    &list,
                    &entry.text(),
                    &already_in,
                    &selected,
                    &built,
                    &confirm,
                );
            });
        }
        let picks = selected.clone();
        let confirm = sheet.confirm.clone();
        list.connect_row_activated(move |_, row| {
            if let Some(check) = find_widget::<gtk::CheckButton>(row) {
                check.set_active(!check.is_active());
                sync_add_confirm(&confirm, picks.borrow().len());
            }
        });
        let dialog = sheet.dialog.clone();
        dialog.present(Some(&self.inner.window));
        let this = self.clone();
        sheet.confirm.connect_clicked(move |_| {
            let ids = selected.borrow().iter().cloned().collect::<Vec<_>>();
            if ids.is_empty() {
                this.toast("Select at least one track");
                return;
            }
            let result = this.inner.session.borrow_mut().add_tracks(
                detail.id.clone(),
                detail.content_token.clone(),
                ids,
            );
            match result {
                Ok(_) => {
                    dialog.close();
                    this.refill_playlists();
                    this.refill_settings();
                    this.open_playlist_detail(&detail.id);
                }
                Err(error) => {
                    this.report("playlist", "Could not save playlist", &error.to_string())
                }
            }
        });
    }

    fn fill_add_track_list(
        &self,
        list: &gtk::ListBox,
        query: &str,
        already_in: &BTreeSet<String>,
        selected: &Rc<RefCell<BTreeSet<String>>>,
        built: &ListScreenBuilt,
        confirm: &gtk::Button,
    ) {
        clear_list(list);
        let items = match self.inner.session.borrow().picker_track_items(query) {
            Ok(items) => items
                .into_iter()
                .filter(|item| !already_in.contains(&item.id))
                .collect::<Vec<_>>(),
            Err(error) => {
                self.report("playlist", "Could not load tracks", &error.to_string());
                return;
            }
        };
        if items.is_empty() {
            built.show_empty();
            sync_add_confirm(confirm, selected.borrow().len());
            return;
        }
        built.show_lists();
        for item in items {
            let id = item.id.clone();
            let row = media_item(&to_media_data(item, self.texture_for(&id)));
            let check = gtk::CheckButton::new();
            check.set_active(selected.borrow().contains(&id));
            check.set_can_target(false);
            row.add_prefix(&check);
            row.set_activatable(true);
            row.set_widget_name(&id);
            let picks = selected.clone();
            let confirm = confirm.clone();
            check.connect_toggled(move |check| {
                if check.is_active() {
                    picks.borrow_mut().insert(id.clone());
                } else {
                    picks.borrow_mut().remove(&id);
                }
                sync_add_confirm(&confirm, picks.borrow().len());
            });
            list.append(&row);
        }
        sync_add_confirm(confirm, selected.borrow().len());
    }

    fn attach_track_context_menu(
        &self,
        widget: &impl IsA<gtk::Widget>,
        track_id: Option<String>,
        playlist: Option<PlaylistEntryRef>,
    ) {
        let widget = widget.clone().upcast::<gtk::Widget>();
        let pending = Rc::new(RefCell::new(None::<(f64, f64)>));
        let click = gtk::GestureClick::new();
        click.set_button(0);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let pending_press = pending.clone();
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
            pending_press.replace(Some((x, y)));
        });
        let this = self.clone();
        let host = widget.clone();
        let track_id_click = track_id.clone();
        let playlist_click = playlist.clone();
        click.connect_released(move |gesture, _, _, _| {
            let Some((x, y)) = pending.take() else {
                return;
            };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            this.popup_track_menu(
                &host,
                x,
                y,
                track_id_click.as_deref(),
                playlist_click.as_ref(),
            );
        });
        widget.add_controller(click);

        let long = gtk::GestureLongPress::new();
        long.set_touch_only(true);
        let this = self.clone();
        let host = widget.clone();
        let track_id_long = track_id;
        let playlist_long = playlist;
        long.connect_pressed(move |gesture, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            this.popup_track_menu(
                &host,
                x,
                y,
                track_id_long.as_deref(),
                playlist_long.as_ref(),
            );
        });
        widget.add_controller(long);
    }

    fn popup_track_menu(
        &self,
        parent: &impl IsA<gtk::Widget>,
        x: f64,
        y: f64,
        track_id: Option<&str>,
        playlist: Option<&PlaylistEntryRef>,
    ) {
        let playlists = self
            .inner
            .session
            .borrow()
            .playlist_rows()
            .unwrap_or_default();
        let can_add = track_id.is_some() && !playlists.is_empty();
        let can_remove = playlist.is_some();
        if !can_add && !can_remove {
            return;
        }

        let menu = gio::Menu::new();
        let group = gio::SimpleActionGroup::new();
        if can_add {
            let submenu = gio::Menu::new();
            for row in &playlists {
                if playlist.is_some_and(|entry| entry.playlist_id == row.id) {
                    continue;
                }
                let item = gio::MenuItem::new(Some(&row.title), None);
                item.set_action_and_target_value(Some("track.add"), Some(&row.id.to_variant()));
                submenu.append_item(&item);
            }
            if submenu.n_items() > 0 {
                menu.append_submenu(Some("Add to Playlist"), &submenu);
            }
            let add = gio::SimpleAction::new("add", Some(glib::VariantTy::STRING));
            let this = self.clone();
            let track_id = track_id.expect("can_add requires a track").to_owned();
            add.connect_activate(move |_, param| {
                let Some(playlist_id) = param.and_then(|value| value.str().map(str::to_owned))
                else {
                    return;
                };
                this.add_track_to_playlist(&track_id, &playlist_id);
            });
            group.add_action(&add);
        }
        if let Some(entry) = playlist {
            menu.append(Some("Remove from Playlist"), Some("track.remove"));
            let remove = gio::SimpleAction::new("remove", None);
            let this = self.clone();
            let entry = entry.clone();
            remove.connect_activate(move |_, _| {
                this.remove_playlist_entry(&entry);
            });
            group.add_action(&remove);
        }
        if menu.n_items() == 0 {
            return;
        }

        let parent = parent.as_ref().clone();
        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.insert_action_group("track", Some(&group));
        popover.set_parent(&parent);
        popover.set_halign(gtk::Align::Start);
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
            x.round() as i32,
            y.round() as i32,
            1,
            1,
        )));
        popover.connect_closed(move |popover| {
            popover.unparent();
        });
        popover.popup();
    }

    fn add_track_to_playlist(&self, track_id: &str, playlist_id: &str) {
        let detail = match self.inner.session.borrow().playlist_detail(playlist_id) {
            Ok(detail) => detail,
            Err(error) => {
                self.report("playlist", "Could not open playlist", &error.to_string());
                return;
            }
        };
        let name = detail.title.clone();
        let result = self.inner.session.borrow_mut().add_tracks(
            playlist_id.to_owned(),
            detail.content_token,
            vec![track_id.to_owned()],
        );
        match result {
            Ok(_) => {
                self.toast(&format!("Added to {name}"));
                self.refill_playlists();
                self.refill_settings();
                if matches!(self.inner.drill.borrow().clone(), Drill::Playlist(id) if id == playlist_id)
                {
                    self.open_playlist_detail(playlist_id);
                }
            }
            Err(error) => self.report("playlist", "Could not save playlist", &error.to_string()),
        }
    }

    fn remove_playlist_entry(&self, entry: &PlaylistEntryRef) {
        let detail = match self
            .inner
            .session
            .borrow()
            .playlist_detail(&entry.playlist_id)
        {
            Ok(detail) => detail,
            Err(error) => {
                self.report("playlist", "Could not open playlist", &error.to_string());
                return;
            }
        };
        let result = self.inner.session.borrow_mut().remove_entries(
            entry.playlist_id.clone(),
            detail.content_token,
            vec![entry.entry_id.clone()],
        );
        match result {
            Ok(_) => {
                self.toast("Removed from playlist");
                self.refill_playlists();
                self.refill_settings();
                self.open_playlist_detail(&entry.playlist_id);
            }
            Err(error) => self.report("playlist", "Could not save playlist", &error.to_string()),
        }
    }

    fn present_edit_playlist(&self, detail: crate::PlaylistDetailRows) {
        let selected = Rc::new(RefCell::new(BTreeSet::<String>::new()));
        let dialog_slot = Rc::new(RefCell::new(None::<adw::Dialog>));
        let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
        column.set_margin_top(8);
        column.set_margin_start(8);
        column.set_margin_end(8);
        for (index, entry) in detail.entries.iter().enumerate() {
            let row = text_row(&to_text_data(entry.clone()));
            let check = gtk::CheckButton::new();
            check.set_tooltip_text(Some("Remove on Save"));
            let id = entry.id.clone();
            let picks = selected.clone();
            check.connect_toggled(move |check| {
                if check.is_active() {
                    picks.borrow_mut().insert(id.clone());
                } else {
                    picks.borrow_mut().remove(&id);
                }
            });
            row.add_prefix(&check);
            if index > 0 {
                let up = gtk::Button::from_icon_name("go-up-symbolic");
                up.set_tooltip_text(Some("Move up and save"));
                let this = self.clone();
                let playlist_id = detail.id.clone();
                let token = detail.content_token.clone();
                let entry_id = entry.id.clone();
                let before = detail.entries[index - 1].id.clone();
                let dialog_slot = dialog_slot.clone();
                up.connect_clicked(move |_| {
                    let result = this.inner.session.borrow_mut().move_entry(
                        playlist_id.clone(),
                        token.clone(),
                        entry_id.clone(),
                        Some(before.clone()),
                    );
                    match result {
                        Ok(_) => {
                            if let Some(dialog) = dialog_slot.borrow().as_ref() {
                                dialog.close();
                            }
                            this.open_playlist_detail(&playlist_id);
                        }
                        Err(error) => this.report(
                            "playlist",
                            "Could not move playlist entry",
                            &error.to_string(),
                        ),
                    }
                });
                row.add_suffix(&up);
            }
            if index + 1 < detail.entries.len() {
                let down = gtk::Button::from_icon_name("go-down-symbolic");
                down.set_tooltip_text(Some("Move down and save"));
                let this = self.clone();
                let playlist_id = detail.id.clone();
                let token = detail.content_token.clone();
                let entry_id = entry.id.clone();
                let before = detail.entries.get(index + 2).map(|row| row.id.clone());
                let dialog_slot = dialog_slot.clone();
                down.connect_clicked(move |_| {
                    let result = this.inner.session.borrow_mut().move_entry(
                        playlist_id.clone(),
                        token.clone(),
                        entry_id.clone(),
                        before.clone(),
                    );
                    match result {
                        Ok(_) => {
                            if let Some(dialog) = dialog_slot.borrow().as_ref() {
                                dialog.close();
                            }
                            this.open_playlist_detail(&playlist_id);
                        }
                        Err(error) => this.report(
                            "playlist",
                            "Could not move playlist entry",
                            &error.to_string(),
                        ),
                    }
                });
                row.add_suffix(&down);
            }
            column.append(&row);
        }
        let save = primary_action("Save");
        column.append(&save);
        let dialog = sheet("Edit Playlist", &column, SheetSize::Form);
        dialog_slot.replace(Some(dialog.clone()));
        dialog.present(Some(&self.inner.window));
        let this = self.clone();
        save.connect_clicked(move |_| {
            let ids = selected.borrow().iter().cloned().collect::<Vec<_>>();
            if ids.is_empty() {
                dialog.close();
                return;
            }
            let result = this.inner.session.borrow_mut().remove_entries(
                detail.id.clone(),
                detail.content_token.clone(),
                ids,
            );
            match result {
                Ok(_) => {
                    dialog.close();
                    this.refill_playlists();
                    this.refill_settings();
                    this.open_playlist_detail(&detail.id);
                }
                Err(error) => {
                    this.report("playlist", "Could not save playlist", &error.to_string())
                }
            }
        });
    }

    fn confirm_delete_playlist(&self, playlist_id: &str, content_token: &str) {
        let dialog = confirm_dialog(&ConfirmData {
            question: "Delete this playlist file?".into(),
            destructive_label: "Delete Playlist".into(),
        });
        let this = self.clone();
        let playlist_id = playlist_id.to_owned();
        let content_token = content_token.to_owned();
        dialog.connect_response(None, move |_, response| {
            if response != "confirm" {
                return;
            }
            let result = this
                .inner
                .session
                .borrow_mut()
                .delete_playlist(playlist_id.clone(), content_token.clone());
            match result {
                Ok(()) => {
                    this.inner.drill.replace(Drill::None);
                    this.inner.drill_list.replace(None);
                    pop_to_root(&this.inner.playlists_nav);
                    this.refill_playlists();
                    this.refill_settings();
                    this.sync_chrome();
                }
                Err(error) => {
                    this.report("playlist", "Could not delete playlist", &error.to_string())
                }
            }
        });
        dialog.present(Some(&self.inner.window));
    }

    fn present_conflicts(&self) {
        let result = self.inner.session.borrow().conflict_rows();
        let rows = match result {
            Ok(rows) => rows,
            Err(error) => {
                self.report(
                    "conflict",
                    "Could not load Sync Conflicts",
                    &error.to_string(),
                );
                return;
            }
        };
        let column = gtk::Box::new(gtk::Orientation::Vertical, 10);
        column.set_widget_name(route_id(MusicScreen::SyncConflictGroup));
        column.set_margin_top(12);
        column.set_margin_bottom(12);
        column.set_margin_start(12);
        column.set_margin_end(12);
        if rows.is_empty() {
            column.append(&status_row(&StatusRowData {
                message: "No Sync Conflicts".into(),
                severity: StatusSeverity::Info,
            }));
        }
        for row in rows {
            column.append(&field_row(&FieldRowData {
                label: row.title.clone(),
                value: format!("{} · {}", row.subtitle, row.trailing),
                editable: false,
            }));
            let data = match row.disposition {
                ConflictDisposition::Auto => ("Merge and Resolve", true),
                ConflictDisposition::Choice => ("Choose Order", true),
                ConflictDisposition::DeletedVersusModified => ("Keep Modified Copy", true),
                ConflictDisposition::ManualOnly => ("Resolve in Files", false),
            };
            let button = action_row(&ActionRowData {
                label: data.0.into(),
                role: ActionRole::Normal,
                enabled: data.1,
            });
            let this = self.clone();
            let group_id = row.id;
            if row.disposition == ConflictDisposition::Choice {
                button.connect_clicked(move |_| this.present_conflict_choices(&group_id));
            } else if row.disposition != ConflictDisposition::ManualOnly {
                button.connect_clicked(move |_| {
                    this.confirm_resolve_conflict(group_id.clone(), None)
                });
            }
            column.append(&button);
        }
        let scroll = gtk::ScrolledWindow::builder().child(&column).build();
        let dialog = sheet("Sync Conflicts", &scroll, SheetSize::Picker);
        dialog.present(Some(&self.inner.window));
    }

    fn present_conflict_choices(&self, group_id: &str) {
        let result = self.inner.session.borrow().conflict_choice_rows(group_id);
        let choices = match result {
            Ok(rows) => rows,
            Err(error) => {
                self.report(
                    "conflict",
                    "Could not load conflict choices",
                    &error.to_string(),
                );
                return;
            }
        };
        let selected = Rc::new(RefCell::new(None::<String>));
        let checks = Rc::new(RefCell::new(Vec::<(String, gtk::Image)>::new()));
        let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
        for choice in choices {
            let row = text_row(&to_text_data(choice.clone()));
            row.set_activatable(true);
            let check = gtk::Image::from_icon_name("object-select-symbolic");
            check.set_visible(false);
            row.add_suffix(&check);
            checks.borrow_mut().push((choice.id.clone(), check));
            let selected = selected.clone();
            let checks = checks.clone();
            let id = choice.id;
            row.connect_activated(move |_| {
                selected.replace(Some(id.clone()));
                for (choice_id, indicator) in checks.borrow().iter() {
                    indicator.set_visible(choice_id == &id);
                }
            });
            column.append(&row);
        }
        let resolve = primary_action("Continue");
        column.append(&resolve);
        let dialog = sheet("Choose Playlist Order", &column, SheetSize::Picker);
        dialog.present(Some(&self.inner.window));
        let this = self.clone();
        let group_id = group_id.to_owned();
        resolve.connect_clicked(move |_| {
            let Some(source) = selected.borrow().clone() else {
                this.toast("Choose one playlist order");
                return;
            };
            dialog.close();
            this.confirm_resolve_conflict(group_id.clone(), Some(source));
        });
    }

    fn confirm_resolve_conflict(&self, group_id: String, selected_source: Option<String>) {
        let dialog = confirm_dialog(&ConfirmData {
            question: "Resolve this Sync Conflict and remove losing copies?".into(),
            destructive_label: "Resolve".into(),
        });
        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response != "confirm" {
                return;
            }
            let result = this
                .inner
                .session
                .borrow_mut()
                .resolve_conflict(group_id.clone(), selected_source.clone());
            match result {
                Ok(()) => {
                    this.refill_all();
                    this.toast("Sync Conflict resolved");
                }
                Err(error) => this.report(
                    "conflict",
                    "Could not resolve Sync Conflict",
                    &error.to_string(),
                ),
            }
        });
        dialog.present(Some(&self.inner.window));
    }

    fn refill_settings(&self) {}

    fn settings_spec(&self) -> SettingsScreen {
        let change = nav_row(&NavRowData {
            label: "Change Folder".into(),
            trailing: None,
        });
        let this = self.clone();
        change.connect_activated(move |_| this.pick_folder());
        let reload = action_row(&ActionRowData {
            label: "Reload".into(),
            role: ActionRole::Normal,
            enabled: self.inner.session.borrow().folder().is_some() && !self.inner.work_busy.get(),
        });
        let this = self.clone();
        reload.connect_clicked(move |_| this.reload_folder());
        let scan = self
            .inner
            .work
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|work| work.display().clone()))
            .unwrap_or_else(idle_music_progress);
        let scan_row = progress_row(&ProgressRowData::from(&scan));
        if let Some(cancel) = find_widget::<gtk::Button>(&scan_row) {
            let this = self.clone();
            cancel.connect_clicked(move |_| this.cancel_library_work());
        }
        self.inner.scan_progress_row.replace(Some(scan_row.clone()));
        let logs = nav_row(&NavRowData {
            label: "Logs".into(),
            trailing: Some("Local only".into()),
        });
        let this = self.clone();
        logs.connect_activated(move |_| this.present_logs());

        let mut info_rows = Vec::new();
        if let Ok(rows) = self.inner.session.borrow().settings_info_rows() {
            for row in rows {
                info_rows.push(text_row(&to_text_data(row)).upcast());
            }
        }
        info_rows.push(
            text_row(&TextRowData {
                title: "Version".into(),
                subtitle: None,
                trailing: Some(env!("CARGO_PKG_VERSION").into()),
                leading: None,
            })
            .upcast(),
        );

        SettingsScreen {
            groups: vec![
                SettingsGroup {
                    id: "folder".into(),
                    title: "Folder".into(),
                    rows: vec![
                        text_row(&TextRowData {
                            title: "Folder".into(),
                            subtitle: self.inner.session.borrow().folder().map(str::to_owned),
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
                    title: "Scan".into(),
                    rows: vec![scan_row.upcast()],
                },
                SettingsGroup {
                    id: "playback".into(),
                    title: "Playback".into(),
                    rows: vec![text_row(&TextRowData {
                        title: "Engine".into(),
                        subtitle: Some(self.inner.session.borrow().transport_name().into()),
                        trailing: Some("MPRIS".into()),
                        leading: None,
                    })
                    .upcast()],
                },
                SettingsGroup {
                    id: "diagnostics".into(),
                    title: "Diagnostics".into(),
                    rows: vec![logs.upcast()],
                },
                SettingsGroup {
                    id: "info".into(),
                    title: "Info".into(),
                    rows: info_rows,
                },
            ],
        }
    }

    fn present_logs(&self) {
        self.record(LogLevel::Info, "diagnostics", "Opened local diagnostics");
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
        clear.connect_clicked(move |_| {
            cleared.inner.session.borrow_mut().diagnostics_mut().clear();
            cleared.refill_logs(&cleared_ui, &query.borrow(), selected_level.get());
        });

        if self.inner.settings_dialog.borrow().is_none() {
            self.present_settings();
        }
        if let Some(dialog) = self.inner.settings_dialog.borrow().as_ref() {
            push_settings_subpage(dialog, "Logs", route_id(MusicScreen::Logs), &ui.root);
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
                    title: "No matching local diagnostics".into(),
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

    fn record(&self, level: LogLevel, category: &str, message: impl Into<String>) {
        self.inner
            .session
            .borrow_mut()
            .diagnostics_mut()
            .record(level, category, message);
    }

    fn report(&self, category: &str, diagnostic: &str, user_message: &str) {
        self.record(LogLevel::Error, category, diagnostic);
        self.toast(user_message);
    }

    fn toast(&self, message: &str) {
        self.inner.toast.add_toast(adw::Toast::new(message));
    }
}

#[derive(Clone)]
enum PlayAll {
    Artist(String),
    Album(String),
    Playlist(String),
}

fn nav_is_pushed(nav: &adw::NavigationView) -> bool {
    nav.visible_page()
        .and_then(|page| page.tag())
        .is_none_or(|tag| tag.as_str() != "root")
}

fn pop_to_root(nav: &adw::NavigationView) {
    while nav_is_pushed(nav) && nav.pop() {}
}

fn location_title(id: &str) -> String {
    id.strip_prefix("artist:")
        .or_else(|| id.strip_prefix("album:"))
        .unwrap_or(id)
        .to_owned()
}

fn track_ids_in(list: &gtk::ListBox) -> Vec<String> {
    let mut ids = Vec::new();
    let mut child = list.first_child();
    while let Some(widget) = child {
        let id = widget.widget_name();
        if !id.is_empty() {
            ids.push(id.to_string());
        }
        child = widget.next_sibling();
    }
    ids
}

fn sync_add_confirm(confirm: &gtk::Button, count: usize) {
    if count == 0 {
        confirm.set_label("Add");
        confirm.set_sensitive(false);
    } else {
        confirm.set_label(&format!("Add ({count})"));
        confirm.set_sensitive(true);
    }
}

fn clear_list(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

fn append_status(list: &gtk::ListBox, message: &str) {
    list.append(&status_row(&StatusRowData {
        message: message.into(),
        severity: StatusSeverity::Info,
    }));
}

fn to_text_data(row: music_core::TextRow) -> TextRowData {
    TextRowData {
        title: row.title,
        subtitle: row.subtitle,
        trailing: row.trailing,
        leading: None,
    }
}

fn to_media_data(row: music_core::MediaItem, texture: Option<gdk::Texture>) -> MediaItemData {
    MediaItemData {
        thumbnail_ref: row.thumbnail_ref,
        label: row.label,
        badge: row.badge,
        texture,
        ..MediaItemData::default()
    }
}

fn to_status_severity(severity: CoreStatusSeverity) -> StatusSeverity {
    match severity {
        CoreStatusSeverity::Info => StatusSeverity::Info,
        CoreStatusSeverity::Warning => StatusSeverity::Warning,
        CoreStatusSeverity::Error => StatusSeverity::Error,
    }
}

fn idle_music_progress() -> ProgressDisplay {
    ProgressDisplay {
        label: "Folder walk and metadata".into(),
        detail: None,
        fraction: None,
        cancel: false,
    }
}

fn find_widget<T: IsA<gtk::Widget>>(root: &impl IsA<gtk::Widget>) -> Option<T> {
    let widget = root.as_ref();
    if let Ok(found) = widget.clone().downcast::<T>() {
        return Some(found);
    }
    let mut child = widget.first_child();
    while let Some(node) = child {
        if let Some(found) = find_widget::<T>(&node) {
            return Some(found);
        }
        child = node.next_sibling();
    }
    None
}
