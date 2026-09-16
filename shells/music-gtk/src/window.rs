//! Native GTK4/libadwaita assembly for every generated Music screen.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;
use music_core::{
    ActionRole as CoreActionRole, ConflictDisposition, LibraryContentState, SortOption,
    StatusSeverity as CoreStatusSeverity, StdVfs, TEMP_PREFIX,
};
use shell_kit_gtk::{
    action_row, apply_token_css, banner, choice_dropdown, confirm_dialog, field_row, list_box_page,
    media_item, nav_row, primary_action, push_page, search_entry, settings_page, sheet, status_row,
    text_row, ActionRole, ActionRowData, ChoiceData, ConfirmData, FieldRowData, LogLevel,
    MediaItemData, MusicScreen, NavRowData, StatusRowData, StatusSeverity, TextRowData,
};

use crate::mpris::MprisState;
use crate::routing::route_id;
use crate::{
    hostname, system_transport, MprisHost, Paths, RemoteCommand, Session, ShellError, APP_TITLE,
    COMPACT_WIDTH,
};

const TOKEN_CSS: &str = include_str!("../../../design/tokens/generated/music.css");
const ADW_ACCENT: &str =
    "@define-color accent_bg_color var(--accent);\n@define-color accent_color var(--accent);\n";
const DIAGNOSTIC_CAPACITY: usize = 5_000;

type AppSession = Session<StdVfs, Box<dyn crate::TransportPort>>;

#[derive(Clone)]
pub struct Window {
    inner: Rc<Inner>,
}

struct Inner {
    window: adw::ApplicationWindow,
    header: adw::HeaderBar,
    switcher: adw::ViewSwitcher,
    switcher_bar: adw::ViewSwitcherBar,
    root_stack: gtk::Stack,
    content_stack: adw::ViewStack,
    library_list: gtk::ListBox,
    playlist_list: gtk::ListBox,
    playlist_nav: adw::NavigationView,
    playlist_root: adw::NavigationPage,
    settings_nav: adw::NavigationView,
    settings_box: gtk::Box,
    conflict_banner: adw::Banner,
    toast: adw::ToastOverlay,
    now_title: gtk::Label,
    now_badge: gtk::Label,
    play_pause: gtk::Button,
    query: RefCell<String>,
    sort: Cell<SortOption>,
    session: RefCell<AppSession>,
    paths: Paths,
    mpris: RefCell<Option<MprisHost>>,
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

        let library_col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        library_col.set_widget_name(route_id(MusicScreen::Library));
        let conflict_banner = banner("Sync Conflicts need review");
        conflict_banner.set_revealed(false);
        conflict_banner.set_button_label(Some("Review"));
        library_col.append(&conflict_banner);
        let library_controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        library_controls.set_margin_top(8);
        library_controls.set_margin_bottom(8);
        library_controls.set_margin_start(8);
        library_controls.set_margin_end(8);
        let search = search_entry();
        search.set_placeholder_text(Some("Title, artist, or album"));
        search.set_hexpand(true);
        let sort = choice_dropdown(&ChoiceData {
            labels: vec![
                "Title".into(),
                "Artist".into(),
                "Album".into(),
                "Duration".into(),
            ],
            selected: 0,
        });
        sort.set_tooltip_text(Some("Sort library"));
        library_controls.append(&search);
        library_controls.append(&sort);
        library_col.append(&library_controls);
        let (library_scroll, library_list) = list_box_page();
        library_scroll.set_vexpand(true);
        library_col.append(&library_scroll);
        let library_nav = shell_kit_gtk::navigation_view();
        library_nav.add(&push_page("Library", &library_col));

        let playlist_col = gtk::Box::new(gtk::Orientation::Vertical, 8);
        playlist_col.set_widget_name(route_id(MusicScreen::PlaylistList));
        let create_playlist = primary_action("Create Playlist");
        create_playlist.set_margin_top(8);
        create_playlist.set_margin_start(8);
        create_playlist.set_margin_end(8);
        playlist_col.append(&create_playlist);
        let (playlist_scroll, playlist_list) = list_box_page();
        playlist_scroll.set_vexpand(true);
        playlist_col.append(&playlist_scroll);
        let playlist_nav = shell_kit_gtk::navigation_view();
        let playlist_root = push_page("Playlists", &playlist_col);
        playlist_nav.add(&playlist_root);

        let now_playing = gtk::Box::new(gtk::Orientation::Vertical, 12);
        now_playing.set_widget_name(route_id(MusicScreen::NowPlaying));
        now_playing.set_valign(gtk::Align::Center);
        now_playing.set_margin_top(24);
        now_playing.set_margin_bottom(24);
        now_playing.set_margin_start(24);
        now_playing.set_margin_end(24);
        let artwork = gtk::Image::from_icon_name("audio-x-generic-symbolic");
        artwork.set_pixel_size(160);
        let now_title = gtk::Label::new(Some("Nothing Playing"));
        now_title.add_css_class("title-1");
        now_title.set_wrap(true);
        let now_badge = gtk::Label::new(None);
        now_badge.add_css_class("dim-label");
        now_badge.set_wrap(true);
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

        let settings_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let settings_nav = shell_kit_gtk::navigation_view();
        let settings_root = push_page("Settings", &settings_box);
        settings_root.set_widget_name(route_id(MusicScreen::Settings));
        settings_nav.add(&settings_root);

        let content_stack = adw::ViewStack::new();
        let library_page = content_stack.add_titled(&library_nav, Some("library"), "Library");
        library_page.set_icon_name(Some("folder-music-symbolic"));
        let playing_page =
            content_stack.add_titled(&now_playing, Some("now-playing"), "Now Playing");
        playing_page.set_icon_name(Some("media-playback-start-symbolic"));
        let playlists_page =
            content_stack.add_titled(&playlist_nav, Some("playlists"), "Playlists");
        playlists_page.set_icon_name(Some("view-list-symbolic"));
        let settings_page = content_stack.add_titled(&settings_nav, Some("settings"), "Settings");
        settings_page.set_icon_name(Some("emblem-system-symbolic"));

        let switcher = adw::ViewSwitcher::new();
        switcher.set_stack(Some(&content_stack));
        switcher.set_policy(adw::ViewSwitcherPolicy::Wide);
        let switcher_bar = adw::ViewSwitcherBar::new();
        switcher_bar.set_stack(Some(&content_stack));
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&switcher));

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&content_stack));
        toolbar.add_bottom_bar(&switcher_bar);

        let root_stack = gtk::Stack::new();
        root_stack.add_named(&picker, Some(route_id(MusicScreen::FolderPicker)));
        root_stack.add_named(&toolbar, Some("music"));
        root_stack.set_visible_child_name(route_id(MusicScreen::FolderPicker));
        let toast = adw::ToastOverlay::new();
        toast.set_child(Some(&root_stack));
        window.set_content(Some(&toast));

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
            header: header.clone(),
            switcher: switcher.clone(),
            switcher_bar: switcher_bar.clone(),
            root_stack,
            content_stack,
            library_list: library_list.clone(),
            playlist_list: playlist_list.clone(),
            playlist_nav,
            playlist_root,
            settings_nav,
            settings_box,
            conflict_banner: conflict_banner.clone(),
            toast,
            now_title,
            now_badge,
            play_pause: play_pause.clone(),
            query: RefCell::new(String::new()),
            sort: Cell::new(SortOption::Title),
            session: RefCell::new(session),
            paths,
            mpris: RefCell::new(None),
        });
        let this = Self { inner };

        let picker = this.clone();
        choose.connect_clicked(move |_| picker.pick_folder());
        let searched = this.clone();
        search.connect_search_changed(move |entry| {
            searched.inner.query.replace(entry.text().to_string());
            searched.refill_library();
        });
        let sorted = this.clone();
        sort.connect_selected_notify(move |dropdown| {
            let option = match dropdown.selected() {
                1 => SortOption::Artist,
                2 => SortOption::Album,
                3 => SortOption::Duration,
                _ => SortOption::Title,
            };
            sorted.inner.sort.set(option);
            sorted.refill_library();
        });
        let played = this.clone();
        library_list.connect_row_activated(move |_, row| {
            let id = row.widget_name();
            if !id.is_empty() {
                played.play_track(&id);
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
        create_playlist.connect_clicked(move |_| created.present_create_playlist());
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

        let sized = this.clone();
        window.connect_realize(move |realized| {
            if let Some(surface) = realized.surface() {
                let on_layout = sized.clone();
                surface.connect_layout(move |_, width, _| on_layout.apply_chrome(width));
            }
            sized.apply_chrome(realized.width());
        });
        this.apply_chrome(window.default_width());
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

    fn load_persisted(&self) {
        if let Some(folder) = self.inner.paths.load_folder() {
            if std::path::Path::new(&folder).is_dir() {
                self.open_folder(&folder);
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
                    Some(path) => this.open_folder(&path),
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

    fn open_folder(&self, folder: &str) {
        let result = self.inner.session.borrow_mut().open_folder(folder);
        match result {
            Ok(()) => {
                self.inner.paths.save_folder(folder);
                self.inner.root_stack.set_visible_child_name("music");
                self.refill_all();
            }
            Err(error) => self.report("folder", "Could not open Folder", &error.to_string()),
        }
    }

    fn refill_all(&self) {
        self.refill_library();
        self.refill_playlists();
        self.refill_settings();
        self.update_now_playing();
    }

    fn refill_library(&self) {
        clear_list(&self.inner.library_list);
        let rows = self
            .inner
            .session
            .borrow_mut()
            .library_rows(self.inner.query.borrow().clone(), self.inner.sort.get());
        match rows {
            Ok(rows) => {
                if rows.state == LibraryContentState::EmptyFolder {
                    append_status(
                        &self.inner.library_list,
                        "This Folder has no supported audio",
                    );
                } else if rows.state == LibraryContentState::NoMatches {
                    append_status(&self.inner.library_list, "No tracks match this search");
                } else {
                    for section in rows.sections {
                        let heading = text_row(&to_text_data(section.heading));
                        heading.set_activatable(false);
                        heading.add_css_class("heading");
                        self.inner.library_list.append(&heading);
                        for item in section.items {
                            let id = item.id.clone();
                            let row = media_item(&to_media_data(item));
                            row.set_activatable(true);
                            row.set_widget_name(&id);
                            self.inner.library_list.append(&row);
                        }
                    }
                }
                if let Ok(issues) = self.inner.session.borrow().scan_issue_rows() {
                    for issue in issues {
                        self.inner.library_list.append(&status_row(&StatusRowData {
                            message: issue.message,
                            severity: to_status_severity(issue.severity),
                        }));
                    }
                }
                self.refill_conflict_banner();
            }
            Err(error) => self.report(
                "library",
                "Could not project library rows",
                &error.to_string(),
            ),
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
            Ok(()) => {
                self.inner
                    .content_stack
                    .set_visible_child_name("now-playing");
                self.update_now_playing();
            }
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
            self.inner
                .now_title
                .set_label(item.label.as_deref().unwrap_or("Unknown Track"));
            self.inner
                .now_badge
                .set_label(item.badge.as_deref().unwrap_or_default());
        } else {
            self.inner.now_title.set_label("Nothing Playing");
            self.inner.now_badge.set_label("");
        }
        let playing = session.transport_snapshot().status == crate::PlaybackStatus::Playing;
        self.inner
            .play_pause
            .set_label(if playing { "Pause" } else { "Play" });
        drop(session);
        if let Some(mpris) = self.inner.mpris.borrow().as_ref() {
            mpris.notify_player();
        }
    }

    fn refill_playlists(&self) {
        clear_list(&self.inner.playlist_list);
        let result = self.inner.session.borrow().playlist_rows();
        match result {
            Ok(rows) if rows.is_empty() => append_status(&self.inner.playlist_list, "No playlists"),
            Ok(rows) => {
                for row in rows {
                    let id = row.id.clone();
                    let widget = text_row(&to_text_data(row));
                    widget.set_activatable(true);
                    widget.set_widget_name(&id);
                    self.inner.playlist_list.append(&widget);
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
        let save = primary_action("Save");
        let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
        column.append(&page);
        column.append(&save);
        let dialog = sheet("Create Playlist", &column);
        dialog.present(&self.inner.window);
        let this = self.clone();
        save.connect_clicked(move |_| {
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
        let result = self.inner.session.borrow().playlist_detail(id);
        let detail = match result {
            Ok(detail) => detail,
            Err(error) => {
                self.report("playlist", "Could not open playlist", &error.to_string());
                return;
            }
        };
        self.inner
            .playlist_nav
            .pop_to_page(&self.inner.playlist_root);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
        column.set_margin_top(12);
        column.set_margin_bottom(12);
        column.set_margin_start(12);
        column.set_margin_end(12);
        for action in &detail.actions {
            let button = action_row(&ActionRowData {
                label: action.label.clone(),
                role: match action.role {
                    CoreActionRole::Normal => ActionRole::Normal,
                    CoreActionRole::Destructive => ActionRole::Destructive,
                },
                enabled: action.enabled,
            });
            match action.id.as_str() {
                "add-tracks" => {
                    let this = self.clone();
                    let detail = detail.clone();
                    button.connect_clicked(move |_| this.present_add_tracks(detail.clone()));
                }
                "play-all" => {
                    let this = self.clone();
                    let playlist_id = detail.id.clone();
                    button.connect_clicked(move |_| {
                        let result = this.inner.session.borrow_mut().play_playlist(&playlist_id);
                        match result {
                            Ok(()) => {
                                this.inner
                                    .content_stack
                                    .set_visible_child_name("now-playing");
                                this.update_now_playing();
                            }
                            Err(error) => this.report(
                                "playback",
                                "Could not play playlist",
                                &error.to_string(),
                            ),
                        }
                    });
                }
                "delete-playlist" => {
                    let this = self.clone();
                    let playlist_id = detail.id.clone();
                    let token = detail.content_token.clone();
                    button.connect_clicked(move |_| {
                        this.confirm_delete_playlist(&playlist_id, &token)
                    });
                }
                _ => button.set_sensitive(false),
            }
            column.append(&button);
        }
        let edit = primary_action("Edit Entries");
        let this = self.clone();
        let detail_for_edit = detail.clone();
        edit.connect_clicked(move |_| this.present_edit_playlist(detail_for_edit.clone()));
        column.append(&edit);
        if detail.entries.is_empty() {
            column.append(&status_row(&StatusRowData {
                message: "This playlist is empty".into(),
                severity: StatusSeverity::Info,
            }));
        } else {
            for entry in detail.entries {
                column.append(&text_row(&to_text_data(entry)));
            }
        }
        let scroll = gtk::ScrolledWindow::builder().child(&column).build();
        let page = push_page(&detail.title, &scroll);
        page.set_widget_name(route_id(MusicScreen::PlaylistDetail));
        self.inner.playlist_nav.push(&page);
    }

    fn present_add_tracks(&self, detail: crate::PlaylistDetailRows) {
        let result = self
            .inner
            .session
            .borrow_mut()
            .library_rows(String::new(), SortOption::Title);
        let library = match result {
            Ok(rows) => rows,
            Err(error) => {
                self.report("playlist", "Could not load tracks", &error.to_string());
                return;
            }
        };
        let selected = Rc::new(RefCell::new(BTreeSet::<String>::new()));
        let (list_scroll, list) = list_box_page();
        for item in library
            .sections
            .into_iter()
            .flat_map(|section| section.items)
        {
            let id = item.id.clone();
            let row = media_item(&to_media_data(item));
            let check = gtk::CheckButton::new();
            row.add_suffix(&check);
            let picks = selected.clone();
            check.connect_toggled(move |check| {
                if check.is_active() {
                    picks.borrow_mut().insert(id.clone());
                } else {
                    picks.borrow_mut().remove(&id);
                }
            });
            list.append(&row);
        }
        let save = primary_action("Save");
        let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
        column.set_widget_name(route_id(MusicScreen::AddTracks));
        column.append(&list_scroll);
        column.append(&save);
        let dialog = sheet("Add Tracks", &column);
        dialog.present(&self.inner.window);
        let this = self.clone();
        save.connect_clicked(move |_| {
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
        let dialog = sheet("Edit Playlist", &column);
        dialog_slot.replace(Some(dialog.clone()));
        dialog.present(&self.inner.window);
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
                    this.inner
                        .playlist_nav
                        .pop_to_page(&this.inner.playlist_root);
                    this.refill_playlists();
                    this.refill_settings();
                }
                Err(error) => {
                    this.report("playlist", "Could not delete playlist", &error.to_string())
                }
            }
        });
        dialog.present(&self.inner.window);
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
        let dialog = sheet("Sync Conflicts", &scroll);
        dialog.present(&self.inner.window);
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
        let dialog = sheet("Choose Playlist Order", &column);
        dialog.present(&self.inner.window);
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
        dialog.present(&self.inner.window);
    }

    fn refill_settings(&self) {
        while let Some(child) = self.inner.settings_box.first_child() {
            self.inner.settings_box.remove(&child);
        }
        let page = settings_page();
        page.set_widget_name(route_id(MusicScreen::Settings));
        let folder_group = adw::PreferencesGroup::new();
        folder_group.set_title("Folder");
        folder_group.add(&text_row(&TextRowData {
            title: "Folder".into(),
            subtitle: self.inner.session.borrow().folder().map(str::to_owned),
            trailing: None,
        }));
        let change = nav_row(&NavRowData {
            label: "Change Folder".into(),
            trailing: None,
        });
        let this = self.clone();
        change.connect_activated(move |_| this.pick_folder());
        folder_group.add(&change);
        let reload = action_row(&ActionRowData {
            label: "Reload".into(),
            role: ActionRole::Normal,
            enabled: self.inner.session.borrow().folder().is_some(),
        });
        let this = self.clone();
        reload.connect_clicked(move |_| {
            let result = this.inner.session.borrow_mut().reload();
            match result {
                Ok(()) => this.refill_all(),
                Err(error) => this.report("folder", "Could not Reload Folder", &error.to_string()),
            }
        });
        folder_group.add(&reload);
        page.add(&folder_group);

        let playback = adw::PreferencesGroup::new();
        playback.set_title("Playback");
        playback.add(&text_row(&TextRowData {
            title: "Engine".into(),
            subtitle: Some(self.inner.session.borrow().transport_name().into()),
            trailing: Some("MPRIS".into()),
        }));
        page.add(&playback);

        let diagnostics = adw::PreferencesGroup::new();
        diagnostics.set_title("Diagnostics");
        let logs = nav_row(&NavRowData {
            label: "Logs".into(),
            trailing: Some("Local only".into()),
        });
        let this = self.clone();
        logs.connect_activated(move |_| this.present_logs());
        diagnostics.add(&logs);
        page.add(&diagnostics);

        let info = adw::PreferencesGroup::new();
        info.set_title("Info");
        if let Ok(rows) = self.inner.session.borrow().settings_info_rows() {
            for row in rows {
                info.add(&text_row(&to_text_data(row)));
            }
        }
        info.add(&text_row(&TextRowData {
            title: "Version".into(),
            subtitle: None,
            trailing: Some(env!("CARGO_PKG_VERSION").into()),
        }));
        page.add(&info);
        self.inner.settings_box.append(&page);
    }

    fn present_logs(&self) {
        self.record(LogLevel::Info, "diagnostics", "Opened local diagnostics");
        let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let search = search_entry();
        search.set_placeholder_text(Some("Message or category"));
        search.set_hexpand(true);
        let level = choice_dropdown(&ChoiceData {
            labels: vec![
                "All levels".into(),
                "Info".into(),
                "Warning".into(),
                "Error".into(),
            ],
            selected: 0,
        });
        let clear = action_row(&ActionRowData {
            label: "Clear".into(),
            role: ActionRole::Destructive,
            enabled: true,
        });
        controls.append(&search);
        controls.append(&level);
        controls.append(&clear);
        let (scroll, list) = list_box_page();
        column.append(&controls);
        column.append(&scroll);
        self.refill_logs(&list, "", None);
        let query = Rc::new(RefCell::new(String::new()));
        let selected_level = Rc::new(Cell::new(None::<LogLevel>));
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
            let level = match dropdown.selected() {
                1 => Some(LogLevel::Info),
                2 => Some(LogLevel::Warning),
                3 => Some(LogLevel::Error),
                _ => None,
            };
            filtered_level.set(level);
            filtered.refill_logs(&filtered_list, &filtered_query.borrow(), level);
        });
        let cleared = self.clone();
        let cleared_list = list.clone();
        clear.connect_clicked(move |_| {
            cleared.inner.session.borrow_mut().diagnostics_mut().clear();
            cleared.refill_logs(&cleared_list, &query.borrow(), selected_level.get());
        });
        let page = push_page("Logs", &column);
        page.set_widget_name(route_id(MusicScreen::Logs));
        self.inner.settings_nav.push(&page);
    }

    fn refill_logs(&self, list: &gtk::ListBox, query: &str, level: Option<LogLevel>) {
        clear_list(list);
        let session = self.inner.session.borrow();
        let entries = session.diagnostics().filtered(query, level);
        if entries.is_empty() {
            append_status(list, "No matching local diagnostics");
        } else {
            for entry in entries {
                list.append(&text_row(&TextRowData {
                    title: entry.message.clone(),
                    subtitle: Some(format!("{} · {}", entry.time_label(), entry.category)),
                    trailing: Some(entry.level.label().into()),
                }));
            }
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
    }
}

fn to_media_data(row: music_core::MediaItem) -> MediaItemData {
    MediaItemData {
        thumbnail_ref: row.thumbnail_ref,
        label: row.label,
        badge: row.badge,
    }
}

fn to_status_severity(severity: CoreStatusSeverity) -> StatusSeverity {
    match severity {
        CoreStatusSeverity::Info => StatusSeverity::Info,
        CoreStatusSeverity::Warning => StatusSeverity::Warning,
        CoreStatusSeverity::Error => StatusSeverity::Error,
    }
}
