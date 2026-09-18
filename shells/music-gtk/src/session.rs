//! Headless GTK-shell session composed from typed `music-core` commands and
//! display rows. No playlist text is parsed or serialized here.

use music_core::{
    add_tracks_logged, album_art_track_id, album_rows, album_track_items, artist_art_track_id,
    artist_rows, artist_track_items, conflict_choice_rows, conflict_rows, create_playlist_logged,
    delete_playlist_logged, media_item, move_entry_logged, playlist_action_rows,
    playlist_art_track_id, playlist_entry_rows, playlist_rows, remove_entries_logged,
    resolve_conflict_logged, search_hits, set_library_view, settings_info_rows, AddTracksCommand,
    CreatePlaylistCommand, DeletePlaylistCommand, LibraryContentState, LibrarySnapshot, MediaItem,
    MediaSource, MovePlaylistEntryCommand, RemovePlaylistEntriesCommand, ResolveConflictCommand,
    SearchHit, SetLibraryViewCommand, SortOption, StatusRow, StdVfs, Store, StoreError, TextRow,
    Vfs, TEMP_PREFIX,
};
use shell_kit_gtk::{LogLevel, LogStore};

use crate::transport::{PlaybackStatus, TransportError, TransportPort, TransportSnapshot};

#[derive(Debug)]
pub enum ShellError {
    Core(StoreError),
    Transport(TransportError),
    NoFolder,
}

impl std::fmt::Display for ShellError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Core(error) => error.fmt(formatter),
            Self::Transport(error) => error.fmt(formatter),
            Self::NoFolder => formatter.write_str("Choose a Folder first"),
        }
    }
}

impl std::error::Error for ShellError {}

impl From<StoreError> for ShellError {
    fn from(error: StoreError) -> Self {
        Self::Core(error)
    }
}

impl From<TransportError> for ShellError {
    fn from(error: TransportError) -> Self {
        Self::Transport(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibrarySection {
    pub heading: TextRow,
    pub items: Vec<MediaItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryRows {
    pub state: LibraryContentState,
    pub generation: u64,
    pub sections: Vec<LibrarySection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistDetailRows {
    pub id: String,
    pub title: String,
    pub content_token: String,
    pub entries: Vec<TextRow>,
    pub actions: Vec<music_core::ActionRow>,
}

pub struct Session<P: TransportPort> {
    vfs: Box<dyn Vfs>,
    device: String,
    store: Option<Store>,
    folder: Option<String>,
    transport: P,
    queue: Vec<MediaSource>,
    queue_index: Option<usize>,
    current_item: Option<MediaItem>,
    diagnostics: LogStore,
}

impl<P: TransportPort> Session<P> {
    #[must_use]
    pub fn new(
        vfs: impl Vfs + 'static,
        device: String,
        transport: P,
        diagnostic_capacity: usize,
    ) -> Self {
        Self {
            vfs: Box::new(vfs),
            device,
            store: None,
            folder: None,
            transport,
            queue: Vec::new(),
            queue_index: None,
            current_item: None,
            diagnostics: LogStore::new(diagnostic_capacity),
        }
    }

    /// Swap the filesystem used for later playlist and conflict writes.
    pub fn replace_vfs(&mut self, vfs: impl Vfs + 'static) {
        self.vfs = Box::new(vfs);
    }

    pub fn open_folder(&mut self, folder: &str) -> Result<(), ShellError> {
        let _span = localcore_trace::span_always("music", "open_folder");
        let store = Store::open(self.vfs(), folder)?;
        self.install_store(store, folder.to_owned());
        Ok(())
    }

    /// Install a store built off-thread (walk + optional cancel).
    pub fn install_store(&mut self, store: Store, folder: String) {
        let count = store.tracks().len();
        let _ = self.transport.stop();
        self.queue.clear();
        self.queue_index = None;
        self.current_item = None;
        self.store = Some(store);
        self.folder = Some(folder);
        self.diagnostics.record(
            LogLevel::Info,
            "folder",
            format!("Opened Folder with {count} tracks"),
        );
    }

    /// Paint from a disposable snapshot without walking. Returns whether it hit.
    pub fn try_install_cache(&mut self, folder: &str, cache_path: &str) -> bool {
        let host = StdVfs::new(TEMP_PREFIX);
        let Some(store) = Store::load_cache(&host, cache_path, folder) else {
            return false;
        };
        self.install_store(store, folder.to_owned());
        true
    }

    /// Install snapshot tracks so the first paint happens before a walk.
    pub fn install_from_snapshot(
        &mut self,
        folder: &str,
        snapshot: LibrarySnapshot,
    ) -> Result<(), ShellError> {
        self.install_store(Store::from_cache(snapshot)?, folder.to_owned());
        Ok(())
    }

    /// Rewrite the private cache after a walk / host metadata.
    pub fn persist_library_cache(&self, cache_path: &str) -> Result<(), ShellError> {
        let host = StdVfs::new(TEMP_PREFIX);
        self.store()?.save_cache(&host, cache_path)?;
        Ok(())
    }

    pub fn reload(&mut self) -> Result<(), ShellError> {
        let vfs = self.vfs.as_ref();
        self.store
            .as_mut()
            .ok_or(ShellError::NoFolder)?
            .reload(vfs)?;
        self.diagnostics
            .record(LogLevel::Info, "folder", "Reloaded Folder");
        Ok(())
    }

    #[must_use]
    pub fn folder(&self) -> Option<&str> {
        self.folder.as_deref()
    }

    pub fn library_rows(
        &mut self,
        query: String,
        sort: SortOption,
    ) -> Result<LibraryRows, ShellError> {
        let _span = localcore_trace::span_always("music", "library_rows");
        let store = self.store_mut()?;
        let generation = set_library_view(store, SetLibraryViewCommand { query, sort });
        let state = store.library_content_state();
        let headings = store.library_section_rows();
        let mut sections = Vec::with_capacity(headings.len());
        for heading in headings {
            let mut offset = 0;
            let mut items = Vec::new();
            loop {
                let page = store.library_track_rows(&heading.id, offset, 500, generation)?;
                let page_len = page.len();
                items.extend(page);
                if page_len < 500 {
                    break;
                }
                offset += page_len;
            }
            sections.push(LibrarySection { heading, items });
        }
        Ok(LibraryRows {
            state,
            generation,
            sections,
        })
    }

    /// Tracks for the add-tracks picker. Does not change the live library view.
    pub fn picker_track_items(&self, query: &str) -> Result<Vec<MediaItem>, ShellError> {
        let store = self.store()?;
        let needle = query.trim().to_lowercase();
        let mut items = store
            .tracks()
            .iter()
            .filter(|track| {
                needle.is_empty()
                    || track.title.to_lowercase().contains(&needle)
                    || track.artist.to_lowercase().contains(&needle)
                    || track.album.to_lowercase().contains(&needle)
            })
            .map(media_item)
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            left.label
                .cmp(&right.label)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(items)
    }

    pub fn pending_metadata(
        &self,
        limit: usize,
    ) -> Result<Vec<music_core::MetadataRequest>, ShellError> {
        Ok(self.store()?.metadata_requests(0, limit))
    }

    #[must_use]
    pub fn pending_metadata_count(&self) -> usize {
        self.store().map(Store::pending_metadata_count).unwrap_or(0)
    }

    #[must_use]
    pub fn track_count(&self) -> usize {
        self.store().map(|store| store.tracks().len()).unwrap_or(0)
    }

    #[must_use]
    pub fn clone_store(&self) -> Option<Store> {
        self.store.clone()
    }

    pub fn apply_metadata_batch(
        &mut self,
        updates: Vec<music_core::MetadataUpdate>,
    ) -> Result<(), ShellError> {
        let _span =
            localcore_trace::span_always("music", "apply_metadata_batch").extra("n", updates.len());
        self.store_mut()?.apply_metadata_batch(updates)?;
        Ok(())
    }

    pub fn playlist_rows(&self) -> Result<Vec<TextRow>, ShellError> {
        Ok(playlist_rows(self.store()?))
    }

    pub fn album_rows(&self) -> Result<Vec<TextRow>, ShellError> {
        Ok(album_rows(self.store()?))
    }

    pub fn artist_rows(&self) -> Result<Vec<TextRow>, ShellError> {
        Ok(artist_rows(self.store()?))
    }

    pub fn album_tracks(&self, album_id: &str) -> Result<Vec<MediaItem>, ShellError> {
        Ok(album_track_items(self.store()?, album_id))
    }

    pub fn artist_tracks(&self, artist_id: &str) -> Result<Vec<MediaItem>, ShellError> {
        Ok(artist_track_items(self.store()?, artist_id))
    }

    pub fn search_hits(&self, query: &str) -> Result<Vec<SearchHit>, ShellError> {
        Ok(search_hits(self.store()?, query))
    }

    pub fn album_art_track(&self, album_id: &str) -> Option<String> {
        album_art_track_id(self.store().ok()?, album_id)
    }

    pub fn artist_art_track(&self, artist_id: &str) -> Option<String> {
        artist_art_track_id(self.store().ok()?, artist_id)
    }

    pub fn playlist_art_track(&self, playlist_id: &str) -> Option<String> {
        playlist_art_track_id(self.store().ok()?, playlist_id)
    }

    pub fn track_item(&self, id: &str) -> Option<MediaItem> {
        self.store().ok()?.track(id).map(media_item)
    }

    pub fn playlist_entry_track_id(&self, playlist_id: &str, entry_id: &str) -> Option<String> {
        self.store()
            .ok()?
            .playlist(playlist_id)?
            .entries
            .iter()
            .find(|entry| entry.id == entry_id)?
            .track_id
            .clone()
    }

    pub fn playlist_detail(&self, playlist_id: &str) -> Result<PlaylistDetailRows, ShellError> {
        let store = self.store()?;
        let title = playlist_rows(store)
            .into_iter()
            .find(|row| row.id == playlist_id)
            .ok_or(StoreError::NotFound)?
            .title;
        let content_token = store
            .playlist(playlist_id)
            .ok_or(StoreError::NotFound)?
            .content_token
            .clone();
        Ok(PlaylistDetailRows {
            id: playlist_id.to_owned(),
            title,
            content_token,
            entries: playlist_entry_rows(store, playlist_id)?,
            actions: playlist_action_rows(store, playlist_id)?,
        })
    }

    pub fn create_playlist(&mut self, name: String) -> Result<String, ShellError> {
        let device = self.device.clone();
        let vfs = self.vfs.as_ref();
        let store = self.store.as_mut().ok_or(ShellError::NoFolder)?;
        let id = create_playlist_logged(vfs, store, &device, CreatePlaylistCommand { name })?;
        self.diagnostics
            .record(LogLevel::Info, "playlist", "Created playlist");
        Ok(id)
    }

    pub fn add_tracks(
        &mut self,
        playlist_id: String,
        content_token: String,
        track_ids: Vec<String>,
    ) -> Result<String, ShellError> {
        let device = self.device.clone();
        let vfs = self.vfs.as_ref();
        let store = self.store.as_mut().ok_or(ShellError::NoFolder)?;
        let token = add_tracks_logged(
            vfs,
            store,
            &device,
            AddTracksCommand {
                playlist_id,
                content_token,
                track_ids,
            },
        )?;
        self.diagnostics
            .record(LogLevel::Info, "playlist", "Saved added tracks");
        Ok(token)
    }

    pub fn remove_entries(
        &mut self,
        playlist_id: String,
        content_token: String,
        entry_ids: Vec<String>,
    ) -> Result<String, ShellError> {
        let device = self.device.clone();
        let vfs = self.vfs.as_ref();
        let store = self.store.as_mut().ok_or(ShellError::NoFolder)?;
        let token = remove_entries_logged(
            vfs,
            store,
            &device,
            RemovePlaylistEntriesCommand {
                playlist_id,
                content_token,
                entry_ids,
            },
        )?;
        self.diagnostics
            .record(LogLevel::Info, "playlist", "Saved playlist edits");
        Ok(token)
    }

    pub fn move_entry(
        &mut self,
        playlist_id: String,
        content_token: String,
        entry_id: String,
        before_entry_id: Option<String>,
    ) -> Result<String, ShellError> {
        let device = self.device.clone();
        let vfs = self.vfs.as_ref();
        let store = self.store.as_mut().ok_or(ShellError::NoFolder)?;
        let token = move_entry_logged(
            vfs,
            store,
            &device,
            MovePlaylistEntryCommand {
                playlist_id,
                content_token,
                entry_id,
                before_entry_id,
            },
        )?;
        self.diagnostics
            .record(LogLevel::Info, "playlist", "Moved playlist entry");
        Ok(token)
    }

    pub fn delete_playlist(
        &mut self,
        playlist_id: String,
        content_token: String,
    ) -> Result<(), ShellError> {
        let device = self.device.clone();
        let vfs = self.vfs.as_ref();
        let store = self.store.as_mut().ok_or(ShellError::NoFolder)?;
        delete_playlist_logged(
            vfs,
            store,
            &device,
            DeletePlaylistCommand {
                playlist_id,
                content_token,
            },
        )?;
        self.diagnostics
            .record(LogLevel::Info, "playlist", "Deleted playlist");
        Ok(())
    }

    pub fn scan_issue_rows(&self) -> Result<Vec<StatusRow>, ShellError> {
        Ok(self.store()?.scan_issue_rows())
    }

    pub fn settings_info_rows(&self) -> Result<Vec<TextRow>, ShellError> {
        Ok(settings_info_rows(self.store()?))
    }

    pub fn conflict_rows(&self) -> Result<Vec<music_core::ConflictRow>, ShellError> {
        Ok(conflict_rows(self.vfs(), self.store()?)?)
    }

    pub fn conflict_choice_rows(&self, group_id: &str) -> Result<Vec<TextRow>, ShellError> {
        Ok(conflict_choice_rows(self.vfs(), self.store()?, group_id)?)
    }

    pub fn resolve_conflict(
        &mut self,
        group_id: String,
        selected_source: Option<String>,
    ) -> Result<(), ShellError> {
        let device = self.device.clone();
        let vfs = self.vfs.as_ref();
        let store = self.store.as_mut().ok_or(ShellError::NoFolder)?;
        resolve_conflict_logged(
            vfs,
            store,
            &device,
            ResolveConflictCommand {
                group_id,
                selected_source,
            },
        )?;
        self.diagnostics
            .record(LogLevel::Info, "conflict", "Resolved Sync Conflict");
        Ok(())
    }

    pub fn play_track(&mut self, track_id: &str) -> Result<(), ShellError> {
        self.play_ids_from(&[track_id.to_owned()], track_id)
    }

    pub fn play_ids_from(&mut self, ids: &[String], start_id: &str) -> Result<(), ShellError> {
        let mut sources = Vec::new();
        let mut index = 0;
        for id in ids {
            if let Ok(source) = self.store()?.media_source(id) {
                if id == start_id {
                    index = sources.len();
                }
                sources.push(source);
            }
        }
        if sources.is_empty() {
            return Err(StoreError::NotFound.into());
        }
        self.start_queue(sources, index)
    }

    pub fn play_album(&mut self, album_id: &str) -> Result<(), ShellError> {
        let ids: Vec<String> = album_track_items(self.store()?, album_id)
            .into_iter()
            .map(|item| item.id)
            .collect();
        let start = ids.first().cloned().unwrap_or_default();
        self.play_ids_from(&ids, &start)
    }

    pub fn play_artist(&mut self, artist_id: &str) -> Result<(), ShellError> {
        let ids: Vec<String> = artist_track_items(self.store()?, artist_id)
            .into_iter()
            .map(|item| item.id)
            .collect();
        let start = ids.first().cloned().unwrap_or_default();
        self.play_ids_from(&ids, &start)
    }

    pub fn play_playlist(&mut self, playlist_id: &str) -> Result<(), ShellError> {
        let sources = self.store()?.playlist_media_sources(playlist_id)?;
        if sources.is_empty() {
            return Err(StoreError::NotFound.into());
        }
        self.start_queue(sources, 0)
    }

    pub fn play_pause(&mut self) -> Result<(), ShellError> {
        if self.transport.snapshot().status == PlaybackStatus::Playing {
            self.pause()?;
        } else {
            self.play()?;
        }
        Ok(())
    }

    pub fn play(&mut self) -> Result<(), ShellError> {
        self.transport.play()?;
        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), ShellError> {
        self.transport.pause()?;
        Ok(())
    }

    pub fn next_track(&mut self) -> Result<(), ShellError> {
        let Some(index) = self.queue_index else {
            return Ok(());
        };
        if index + 1 < self.queue.len() {
            self.load_queue_index(index + 1)?;
        }
        Ok(())
    }

    pub fn previous(&mut self) -> Result<(), ShellError> {
        let Some(index) = self.queue_index else {
            return Ok(());
        };
        if index > 0 {
            self.load_queue_index(index - 1)?;
        } else {
            self.transport.seek_to(0)?;
        }
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), ShellError> {
        self.transport.stop()?;
        Ok(())
    }

    pub fn seek_relative(&mut self, offset_ms: i64) -> Result<(), ShellError> {
        let current = self.transport.snapshot().position_ms;
        let position = if offset_ms.is_negative() {
            current.saturating_sub(offset_ms.unsigned_abs())
        } else {
            current.saturating_add(offset_ms as u64)
        };
        self.transport.seek_to(position)?;
        Ok(())
    }

    pub fn set_position(&mut self, position_ms: u64) -> Result<(), ShellError> {
        self.transport.seek_to(position_ms)?;
        Ok(())
    }

    pub fn set_volume(&mut self, volume: f64) -> Result<(), ShellError> {
        self.transport.set_volume(volume)?;
        Ok(())
    }

    #[must_use]
    pub fn transport_snapshot(&self) -> TransportSnapshot {
        self.transport.snapshot()
    }

    #[must_use]
    pub fn transport_name(&self) -> &'static str {
        self.transport.engine_name()
    }

    #[must_use]
    pub fn current_item(&self) -> Option<&MediaItem> {
        self.current_item.as_ref()
    }

    #[must_use]
    pub fn diagnostics(&self) -> &LogStore {
        &self.diagnostics
    }

    #[must_use]
    pub fn diagnostics_mut(&mut self) -> &mut LogStore {
        &mut self.diagnostics
    }

    #[must_use]
    pub fn transport(&self) -> &P {
        &self.transport
    }

    fn start_queue(&mut self, queue: Vec<MediaSource>, index: usize) -> Result<(), ShellError> {
        self.queue = queue;
        self.load_queue_index(index)
    }

    fn load_queue_index(&mut self, index: usize) -> Result<(), ShellError> {
        let source = self.queue.get(index).cloned().ok_or(StoreError::NotFound)?;
        let item = self
            .store()?
            .track(&source.id)
            .map(media_item)
            .ok_or(StoreError::NotFound)?;
        self.transport.load(&source)?;
        self.transport.play()?;
        self.queue_index = Some(index);
        self.current_item = Some(item);
        self.diagnostics
            .record(LogLevel::Info, "playback", "Started local playback");
        Ok(())
    }

    fn store(&self) -> Result<&Store, ShellError> {
        self.store.as_ref().ok_or(ShellError::NoFolder)
    }

    fn store_mut(&mut self) -> Result<&mut Store, ShellError> {
        self.store.as_mut().ok_or(ShellError::NoFolder)
    }

    fn vfs(&self) -> &dyn Vfs {
        self.vfs.as_ref()
    }
}
