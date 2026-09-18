//! Headless Gallery session. Leftover `localgallery` is host-only
//! (config, snapshot path, XDG thumbs, folder watch, scan/ops). Photo
//! windows come from `gallery_ffi::LibraryIndex`. Leftover
//! `open_library` builds the core `gallery_index::LibraryIndex`, so the
//! FFI catalog is produced by [`gallery_ffi::ScannerSession`].

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use gallery_ffi::view::MAX_VIEW_WINDOW;
use gallery_ffi::{
    load_snapshot, person_link_state, person_log_append, person_log_project_report,
    photo_file_from_scan, read_image_metadata, read_video_date, save_snapshot, GalleryMediaItem,
    GalleryTextRow, LibraryIndex, MemoryContactCommandItem, MemoryFolderCommandItem,
    MemoryGenerator, MemoryPersonCommandItem, MemoryStructure, PersonLinkKind,
    PersonLinkResolution, PersonStateStructure, RemovePhotosResult, ScanCatalogHost, ScanCommand,
    ScanError, ScanMetrics, ScanProgressListener, ScannedFolderHost, ScannedSidecarHost,
    ScannerSession, ScheduledMemoryContext, SnapshotHostDocument, ViewError, ViewStructure,
};
use localcore_ui::{count_detail, found_detail, ProgressDisplay, WorkProgress};
use localgallery::mutate::MutationTarget;
use localgallery::ops::{OpKind, OpLedger};
use localgallery::{
    library_availability, AnalysisSummary, Config, LibraryAvailability, LibraryState, PhotoFile,
};
use shell_kit_gtk::{LogLevel, LogStore};

use crate::paging::{years_from_structure, ViewList, YearMark};

const PHOTO_PAGE: u64 = MAX_VIEW_WINDOW as u64;
const APPLE_EPOCH_UNIX: f64 = 978_307_200.0;

/// Photos tab vs a drill-in id list. [`LibraryIndex`] has one photo projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhotoIntent {
    Library {
        query: String,
        tags: Vec<String>,
    },
    Ids {
        view_id: String,
        ids: Vec<String>,
        query: String,
        tags: Vec<String>,
    },
}

/// Thumbnail badges and menu copy for one person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonMarks {
    pub featured: bool,
    pub me: bool,
    pub linked: bool,
    pub hidden: bool,
    pub link_label: String,
}

impl Default for PhotoIntent {
    fn default() -> Self {
        Self::Library {
            query: String::new(),
            tags: Vec::new(),
        }
    }
}

fn drop_intent_ids(intent: &mut PhotoIntent, drop: &HashSet<&str>) {
    if let PhotoIntent::Ids { ids, .. } = intent {
        ids.retain(|id| !drop.contains(id.as_str()));
    }
}

#[derive(Debug)]
pub enum ShellError {
    NoFolder,
    InvalidPath,
    Scan(ScanError),
    Host(localgallery::HostError),
    View(ViewError),
}

impl std::fmt::Display for ShellError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoFolder => formatter.write_str("Choose a Folder first"),
            Self::InvalidPath => formatter.write_str("Folder path is not UTF-8"),
            Self::Scan(error) => error.fmt(formatter),
            Self::Host(error) => error.fmt(formatter),
            Self::View(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ShellError {}

impl From<ScanError> for ShellError {
    fn from(error: ScanError) -> Self {
        Self::Scan(error)
    }
}

impl From<localgallery::HostError> for ShellError {
    fn from(error: localgallery::HostError) -> Self {
        Self::Host(error)
    }
}

impl From<ViewError> for ShellError {
    fn from(error: ViewError) -> Self {
        Self::View(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhotoPage {
    pub structure: ViewStructure,
    pub items: Vec<GalleryMediaItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPage {
    pub structure: ViewStructure,
    pub rows: Vec<GalleryTextRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhotoHost {
    pub path: String,
    pub filename: String,
    pub is_video: bool,
    pub live_photo_video_path: Option<String>,
}

/// Leaf folder shown as an Events tile (leftover `event_folders`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventFolder {
    pub id: String,
    pub name: String,
    pub photo_count: u32,
    pub cover_photo_id: Option<String>,
    pub cover_path: Option<String>,
}

/// Hosts + folder rows after `index.build` / `set_folders`. Built on a
/// worker so GTK only installs pointers and refills widgets.
#[derive(Debug, Clone)]
pub struct PreparedCatalog {
    pub hosts: HashMap<String, PhotoHost>,
    pub last_folders: Vec<ScannedFolderHost>,
    pub scan_photo_ids: Vec<String>,
    pub photo_count: usize,
    pub apply_ms: f64,
    /// Sidecar tags were merged before `index.build`.
    pub enriched: bool,
}

/// One collection rail after `index.build`. Built on a worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedCollection {
    pub id: String,
    pub title: String,
    pub rows: Vec<GalleryTextRow>,
}

/// Folder / people / events / tags snapshot. GTK only binds widgets.
#[derive(Debug, Clone)]
pub struct PreparedHub {
    pub folders: TextPage,
    pub people: TextPage,
    pub events: Vec<EventFolder>,
    pub collections: Vec<PreparedCollection>,
    pub tags: TextPage,
}

/// Worker-built catalog plus flattened lists. GTK pointer-swaps and paints.
#[derive(Debug, Clone)]
pub struct PreparedUi {
    pub catalog: PreparedCatalog,
    pub photos: ViewList,
    pub years: Vec<YearMark>,
    pub hub: PreparedHub,
    pub config_error: Option<String>,
}

/// In-memory last `MemoryGenerator` result. Not persisted (5.8-m2).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MemoriesCache {
    pub generation: u64,
    pub seed: String,
    pub items: Vec<MemoryStructure>,
}

/// Settings / chrome copy for the live job (library walk or Scan Photos).
#[derive(Debug, Clone, PartialEq)]
pub struct ScanStatus {
    pub label: String,
    pub detail: Option<String>,
    pub fraction: Option<f64>,
}

struct StatusListener {
    work: Arc<Mutex<Option<WorkProgress>>>,
}

impl ScanProgressListener for StatusListener {
    fn on_progress(&self, discovered: u32) {
        if let Ok(mut work) = self.work.lock() {
            if let Some(progress) = work.as_mut() {
                progress.update("Scanning", Some(found_detail(discovered as u64)), None);
            }
        }
    }
}

pub struct Session {
    config: Config,
    index: Arc<LibraryIndex>,
    scanner: Arc<ScannerSession>,
    memory_gen: Option<Arc<MemoryGenerator>>,
    ops: OpLedger,
    persist_host: bool,
    leftover_loaded: bool,
    folder: Option<PathBuf>,
    query: String,
    required_tags: Vec<String>,
    photos_intent: PhotoIntent,
    current_intent: PhotoIntent,
    photo_count: usize,
    scan_work: Arc<Mutex<Option<WorkProgress>>>,
    diagnostics: LogStore,
    hosts: HashMap<String, PhotoHost>,
    last_folders: Vec<ScannedFolderHost>,
    folder_by_id: HashMap<String, usize>,
    path_to_photo: HashMap<String, String>,
    scan_photo_ids: Vec<String>,
    memories: MemoriesCache,
    memory_seed: String,
    last_apply_ms: f64,
    last_leftover_ms: f64,
    visible_ids: Arc<Vec<String>>,
    person_state: PersonStateStructure,
    contacts: Vec<MemoryContactCommandItem>,
    selecting: bool,
    selected: BTreeSet<String>,
    analysis_cancel: Arc<AtomicBool>,
    last_analysis: Option<AnalysisSummary>,
}

impl Session {
    #[must_use]
    pub fn new(diagnostic_capacity: usize) -> Self {
        Self::with_persist(diagnostic_capacity, false)
    }

    /// Persist leftover `Config` / leftover snapshot (UI path). Tests keep
    /// this off so XDG config and cache stay untouched.
    #[must_use]
    pub fn with_persist(diagnostic_capacity: usize, persist_host: bool) -> Self {
        let mut config = if persist_host {
            Config::load()
        } else {
            Config::default()
        };
        ensure_device_id(&mut config, persist_host);
        let contacts = load_contacts(config.contacts_root.as_deref());
        Self {
            config,
            index: LibraryIndex::new(),
            scanner: ScannerSession::new(),
            memory_gen: None,
            ops: OpLedger::default(),
            persist_host,
            leftover_loaded: false,
            folder: None,
            query: String::new(),
            required_tags: Vec::new(),
            photos_intent: PhotoIntent::default(),
            current_intent: PhotoIntent::default(),
            photo_count: 0,
            scan_work: Arc::new(Mutex::new(None)),
            diagnostics: LogStore::new(diagnostic_capacity),
            hosts: HashMap::new(),
            last_folders: Vec::new(),
            folder_by_id: HashMap::new(),
            path_to_photo: HashMap::new(),
            scan_photo_ids: Vec::new(),
            memories: MemoriesCache::default(),
            memory_seed: civil_day_seed(),
            last_apply_ms: 0.0,
            last_leftover_ms: 0.0,
            visible_ids: Arc::new(Vec::new()),
            person_state: PersonStateStructure::default(),
            contacts,
            selecting: false,
            selected: BTreeSet::new(),
            analysis_cancel: Arc::new(AtomicBool::new(false)),
            last_analysis: None,
        }
    }

    #[must_use]
    pub fn index(&self) -> &LibraryIndex {
        &self.index
    }

    #[must_use]
    pub fn index_arc(&self) -> Arc<LibraryIndex> {
        self.index.clone()
    }

    #[must_use]
    pub fn scanner_arc(&self) -> Arc<ScannerSession> {
        self.scanner.clone()
    }

    #[must_use]
    pub fn scan_work_arc(&self) -> Arc<Mutex<Option<WorkProgress>>> {
        self.scan_work.clone()
    }

    pub fn open_folder(&mut self, folder: &Path) -> Result<(), ShellError> {
        let catalog = self.scan_catalog(folder)?;
        self.apply_catalog(catalog, Some(folder))
    }

    pub fn reload(&mut self) -> Result<(), ShellError> {
        let folder = self.folder.clone().ok_or(ShellError::NoFolder)?;
        self.open_folder(&folder)
    }

    /// I/O walk only. GTK can spawn this; tests stay sync.
    ///
    /// When [`Self::persist_host`] is on, the leftover JSON snapshot is
    /// passed in so unchanged photos keep their sidecar tags (iOS
    /// `loadCache` + cached scan).
    pub fn scan_catalog(&self, folder: &Path) -> Result<ScanCatalogHost, ShellError> {
        let cache = self
            .persist_host
            .then(|| Self::load_scan_cache(folder))
            .flatten();
        Self::scan_with(
            self.scanner.clone(),
            self.scan_work.clone(),
            folder,
            cache.as_ref(),
        )
    }

    pub fn scan_with(
        scanner: Arc<ScannerSession>,
        work: Arc<Mutex<Option<WorkProgress>>>,
        folder: &Path,
        cache: Option<&SnapshotHostDocument>,
    ) -> Result<ScanCatalogHost, ShellError> {
        let root = folder.to_str().ok_or(ShellError::InvalidPath)?.to_string();
        if let Ok(mut guard) = work.lock() {
            *guard = Some(WorkProgress::new("Scanning"));
        }
        let listener = Arc::new(StatusListener { work: work.clone() });
        let cached = cache.map(|snap| snap.all_photos.len()).unwrap_or(0);
        let _span = localcore_trace::span_always("scan", "scanner.scan")
            .extra("root", &root)
            .extra("cached", cached);
        let catalog = scanner.scan(
            root,
            ScanCommand {
                reuse_cached: false,
                cached_photos: cache
                    .map(|snap| snap.all_photos.clone())
                    .unwrap_or_default(),
                cached_sidecar_manifest: cache
                    .and_then(|snap| snap.sidecar_manifest.clone())
                    .unwrap_or_default(),
            },
            Some(listener),
        )?;
        localcore_trace::event(
            "scan",
            format!(
                "catalog photos={} folders={} cache_hits={}",
                catalog.flat_photos.len(),
                catalog.folders.len(),
                catalog.timings.cache_hits
            ),
        );
        Ok(catalog)
    }

    /// Last leftover / iOS `LibrarySnapshot` for `folder`, if the file
    /// matches this root. Corrupt or foreign snapshots are ignored.
    pub fn load_scan_cache(folder: &Path) -> Option<SnapshotHostDocument> {
        Self::load_scan_cache_at(&localgallery::config::snapshot_path(), folder)
    }

    pub fn load_scan_cache_at(path: &Path, folder: &Path) -> Option<SnapshotHostDocument> {
        let path = path.to_str()?;
        let snap = match load_snapshot(path.to_string()) {
            Ok(snap) => snap,
            Err(error) => {
                localcore_trace::event("catalog", format!("snapshot load skipped: {error}"));
                return None;
            }
        };
        let root = snap.folders.first()?.path.as_str();
        if Path::new(root) != folder {
            localcore_trace::event(
                "catalog",
                format!(
                    "snapshot root mismatch have={root} want={}",
                    folder.display()
                ),
            );
            return None;
        }
        localcore_trace::event(
            "catalog",
            format!("snapshot hit photos={}", snap.all_photos.len()),
        );
        Some(snap)
    }

    /// Tagged catalog from a snapshot — first GTK paint before the walk.
    pub fn catalog_from_snapshot(snap: &SnapshotHostDocument) -> ScanCatalogHost {
        ScanCatalogHost {
            flat_photos: snap.all_photos.clone(),
            folders: snap.folders.clone(),
            needs_enrichment: false,
            sidecar_manifest: snap.sidecar_manifest.clone().unwrap_or_default(),
            added_paths: Vec::new(),
            removed_paths: Vec::new(),
            modified_paths: Vec::new(),
            failed_directory_paths: Vec::new(),
            timings: ScanMetrics::default(),
        }
    }

    pub fn snapshot_from_catalog(catalog: &ScanCatalogHost) -> SnapshotHostDocument {
        SnapshotHostDocument {
            all_photos: catalog.flat_photos.clone(),
            folders: catalog.folders.clone(),
            sidecar_manifest: Some(catalog.sidecar_manifest.clone()),
        }
    }

    /// Re-read sidecars whose listing row changed (including deletes).
    pub fn mark_changed_sidecars(
        catalog: &mut ScanCatalogHost,
        cached: Option<&SnapshotHostDocument>,
    ) {
        let Some(cached) = cached.and_then(|snap| snap.sidecar_manifest.as_ref()) else {
            return;
        };
        let old: HashMap<&str, &ScannedSidecarHost> = cached
            .iter()
            .map(|row| (row.photo_id.as_str(), row))
            .collect();
        let mut changed: HashSet<String> = HashSet::new();
        let mut deleted: HashSet<String> = HashSet::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for row in &catalog.sidecar_manifest {
            seen.insert(row.photo_id.as_str());
            match old.get(row.photo_id.as_str()) {
                Some(prev) if sidecar_row_eq(prev, row) => {}
                _ => {
                    changed.insert(row.photo_id.clone());
                }
            }
        }
        for row in cached {
            if !seen.contains(row.photo_id.as_str()) {
                deleted.insert(row.photo_id.clone());
            }
        }
        if changed.is_empty() && deleted.is_empty() {
            return;
        }
        for photo in &mut catalog.flat_photos {
            if deleted.contains(&photo.id) {
                photo.hierarchical_tags.clear();
                photo.country_code = None;
                photo.face_regions.clear();
                photo.enriched_file_date = None;
            } else if changed.contains(&photo.id) {
                photo.enriched_file_date = None;
            }
        }
        catalog.needs_enrichment = true;
        localcore_trace::event(
            "catalog",
            format!(
                "sidecar listing changed n={} deleted={}",
                changed.len(),
                deleted.len()
            ),
        );
    }

    /// Write the iOS / leftover v20 JSON snapshot. Next launch hydrates tags.
    pub fn persist_scan_snapshot(
        folder: &Path,
        catalog: &ScanCatalogHost,
    ) -> Result<(), ShellError> {
        Self::persist_scan_snapshot_to(&localgallery::config::snapshot_path(), catalog).map_err(
            |error| {
                localcore_trace::event(
                    "catalog",
                    format!(
                        "snapshot persist failed folder={} err={error}",
                        folder.display()
                    ),
                );
                error
            },
        )
    }

    pub fn persist_scan_snapshot_to(
        path: &Path,
        catalog: &ScanCatalogHost,
    ) -> Result<(), ShellError> {
        if catalog.folders.is_empty() {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let path = path.to_str().ok_or(ShellError::InvalidPath)?;
        let _span = localcore_trace::span_always("catalog", "persist_snapshot")
            .extra("photos", catalog.flat_photos.len())
            .extra("sidecars", catalog.sidecar_manifest.len());
        save_snapshot(path.to_string(), Self::snapshot_from_catalog(catalog))?;
        localcore_trace::event("catalog", format!("snapshot wrote {path}"));
        Ok(())
    }

    /// Update the shared job without resetting [`WorkProgress::started`].
    pub fn touch_shared_work(
        work: &Mutex<Option<WorkProgress>>,
        label: &str,
        detail: Option<String>,
        fraction: Option<f64>,
    ) {
        if let Ok(mut guard) = work.lock() {
            match guard.as_mut() {
                Some(progress) => progress.update(label, detail, fraction),
                None => {
                    let mut progress = WorkProgress::new(label);
                    if detail.is_some() || fraction.is_some() {
                        progress.update(label, detail, fraction);
                    }
                    *guard = Some(progress);
                }
            }
        }
    }

    pub fn begin_work(&self, label: &str) {
        if let Ok(mut guard) = self.scan_work.lock() {
            *guard = Some(WorkProgress::new(label));
        }
    }

    pub fn touch_work(&self, label: &str, detail: Option<String>, fraction: Option<f64>) {
        Self::touch_shared_work(&self.scan_work, label, detail, fraction);
    }

    pub fn touch_work_counts(&self, label: &str, done: u64, total: u64) {
        if let Ok(mut guard) = self.scan_work.lock() {
            let progress = guard.get_or_insert_with(|| WorkProgress::new(label));
            let started = progress.started();
            let fraction = (total > 0).then(|| done as f64 / total as f64);
            progress.update(
                label,
                Some(count_detail(done, total, started, Instant::now())),
                fraction,
            );
        }
    }

    pub fn clear_work(&self) {
        if let Ok(mut guard) = self.scan_work.lock() {
            *guard = None;
        }
    }

    pub fn persist_library_root(folder: &Path) -> Result<(), std::io::Error> {
        let mut config = Config::load();
        config.library_root = Some(folder.to_path_buf());
        config.save()
    }

    pub fn cancel_scan(&self) {
        self.scanner.cancel();
        if let Ok(mut work) = self.scan_work.lock() {
            *work = None;
        }
    }

    /// Sidecar tags / EXIF onto scanned rows. FFI `scan` is walk-only.
    pub fn enrich_catalog(catalog: &mut ScanCatalogHost) {
        let _span = localcore_trace::span_always("catalog", "enrich")
            .extra("photos", catalog.flat_photos.len());
        let mut tagged = 0u32;
        for photo in &mut catalog.flat_photos {
            if photo.enriched_file_date.is_some() {
                continue;
            }
            if photo.is_video {
                if let Some(unix) = read_video_date(photo.path.clone()) {
                    photo.date_taken = Some(unix as f64 - APPLE_EPOCH_UNIX);
                    photo.date_from_metadata = true;
                }
                photo.enriched_file_date = photo.file_modification_date.or(Some(0.0));
                continue;
            }
            let meta = read_image_metadata(photo.path.clone());
            if !meta.hierarchical_tags.is_empty() {
                tagged += 1;
            }
            // Only replace fields when the sidecar (or EXIF) actually
            // produced them, so in-memory catalogs used by tests keep
            // their tags. Sidecar deletes retract in
            // [`Self::mark_changed_sidecars`].
            if !meta.hierarchical_tags.is_empty() {
                photo.hierarchical_tags = meta.hierarchical_tags;
            }
            if meta.country_code.is_some() {
                photo.country_code = meta.country_code;
            }
            if meta.gps_latitude.is_some() {
                photo.gps_latitude = meta.gps_latitude;
            }
            if meta.gps_longitude.is_some() {
                photo.gps_longitude = meta.gps_longitude;
            }
            if !meta.face_regions.is_empty() {
                photo.face_regions = meta.face_regions;
            }
            photo.enriched_file_date = photo.file_modification_date.or(Some(0.0));
        }
        catalog.needs_enrichment = false;
        localcore_trace::event("catalog", format!("enrich tagged={tagged}"));
    }

    /// Index + host map. Safe on a worker if GTK is not reading windows.
    /// Does not read sidecars — call [`Self::enrich_catalog`] first when tags
    /// must be present (tests, second GTK pass).
    pub fn prepare_catalog(index: &LibraryIndex, catalog: ScanCatalogHost) -> PreparedCatalog {
        let apply_started = Instant::now();
        let _span = localcore_trace::span_always("catalog", "prepare")
            .extra("photos", catalog.flat_photos.len())
            .extra("folders", catalog.folders.len());
        let photo_ids: Vec<String> = catalog
            .flat_photos
            .iter()
            .map(|photo| derived_photo_id(&photo.path))
            .collect();
        let photo_count = photo_ids.len();
        let mut hosts = HashMap::with_capacity(photo_count.saturating_mul(2));
        for photo in &catalog.flat_photos {
            let host = PhotoHost {
                path: photo.path.clone(),
                filename: photo.filename.clone(),
                is_video: photo.is_video,
                live_photo_video_path: photo.live_photo_video_path.clone(),
            };
            hosts.insert(derived_photo_id(&photo.path), host.clone());
            hosts.insert(photo.id.clone(), host);
        }
        let last_folders = catalog.folders.clone();
        {
            let _span = localcore_trace::span("catalog", "index.build")
                .extra("photos", catalog.flat_photos.len());
            index.build(catalog.flat_photos);
        }
        {
            let _span = localcore_trace::span("catalog", "index.set_folders")
                .extra("folders", last_folders.len());
            index.set_folders(last_folders.clone(), photo_ids.clone());
        }
        PreparedCatalog {
            hosts,
            last_folders,
            scan_photo_ids: photo_ids,
            photo_count,
            apply_ms: apply_started.elapsed().as_secs_f64() * 1000.0,
            enriched: false,
        }
    }

    /// Flatten photos + hub after `index.build`. Safe on a worker.
    pub fn project_photos(index: &LibraryIndex, intent: &PhotoIntent) -> (ViewList, Vec<YearMark>) {
        let structure = Self::apply_intent_on_index(index, intent);
        (
            ViewList::photos_flat(&structure),
            years_from_structure(&structure),
        )
    }

    pub fn apply_intent_on_index(index: &LibraryIndex, intent: &PhotoIntent) -> ViewStructure {
        match intent {
            PhotoIntent::Library { query, tags } => {
                index.set_photo_view(query.clone(), tags.clone())
            }
            PhotoIntent::Ids {
                view_id,
                ids,
                query,
                tags,
            } => {
                index.set_photo_ids_view(view_id.clone(), ids.clone(), query.clone(), tags.clone())
            }
        }
    }

    pub fn project_hub(
        index: &LibraryIndex,
        hosts: &HashMap<String, PhotoHost>,
        last_folders: &[ScannedFolderHost],
        scan_photo_ids: &[String],
    ) -> PreparedHub {
        let folders = load_text_page(index.folder_structure(None), |generation| {
            index.folder_window("folders".into(), 0, PHOTO_PAGE, generation)
        });
        let people = load_text_page(index.people_rail_structure(), |generation| {
            index.people_rail_window("people".into(), 0, PHOTO_PAGE, generation)
        });
        let tags = load_text_page(index.tag_structure(), |generation| {
            index.tag_window("tags".into(), 0, PHOTO_PAGE, generation)
        });
        let collection_structure = index.collection_structure();
        let mut collections = Vec::new();
        for section in &collection_structure.sections {
            let page = load_text_page(collection_structure.clone(), |generation| {
                index.collection_window(section.id.clone(), 0, PHOTO_PAGE, generation)
            });
            collections.push(PreparedCollection {
                id: section.id.clone(),
                title: section.title.clone(),
                rows: page.rows,
            });
        }
        PreparedHub {
            folders,
            people,
            events: event_folder_rows(last_folders, scan_photo_ids, hosts),
            collections,
            tags,
        }
    }

    /// Walk-or-enrich + index + flatten. GTK only installs the result.
    pub fn prepare_ui(
        index: &LibraryIndex,
        mut catalog: ScanCatalogHost,
        enrich: bool,
        persist: Option<&Path>,
        work: &Mutex<Option<WorkProgress>>,
    ) -> PreparedUi {
        if enrich {
            Self::touch_shared_work(work, "Reading metadata", None, None);
            Self::enrich_catalog(&mut catalog);
        }
        Self::touch_shared_work(work, "Preparing", None, None);
        let mut prepared = Self::prepare_catalog(index, catalog);
        prepared.enriched = enrich;
        // Rebuild starts with empty person_state. Replay `.gallery/log` before
        // projecting the hub or the rail paints every person and drops stars.
        if let Some(folder) = persist {
            Self::apply_person_log(index, folder);
        }
        let (photos, years) = Self::project_photos(index, &PhotoIntent::default());
        let hub = Self::project_hub(
            index,
            &prepared.hosts,
            &prepared.last_folders,
            &prepared.scan_photo_ids,
        );
        let config_error = persist.and_then(|folder| {
            Self::persist_library_root(folder)
                .err()
                .map(|error| error.to_string())
        });
        PreparedUi {
            catalog: prepared,
            photos,
            years,
            hub,
            config_error,
        }
    }

    /// Pointer swap only. No walk, no `index.build`, no FFI windows.
    pub fn install_prepared(
        &mut self,
        prepared: PreparedCatalog,
        folder: Option<&Path>,
        visible: Option<Arc<Vec<String>>>,
    ) -> Result<(), ShellError> {
        let token = self.ops.begin(OpKind::Scan);
        self.last_leftover_ms = 0.0;
        self.leftover_loaded = false;
        self.invalidate_memories();
        self.photo_count = prepared.photo_count;
        self.hosts = prepared.hosts;
        self.last_folders = prepared.last_folders;
        self.scan_photo_ids = prepared.scan_photo_ids;
        self.rebuild_host_lookups();
        self.last_apply_ms = prepared.apply_ms;
        self.prune_selection();

        if let Some(folder) = folder {
            self.config.library_root = Some(folder.to_path_buf());
            self.folder = Some(folder.to_path_buf());
            if self.persist_host {
                let _ = self.config.save();
            }
        }
        self.refresh_person_state();

        self.query.clear();
        self.required_tags.clear();
        self.photos_intent = PhotoIntent::default();
        self.current_intent = PhotoIntent::default();
        if let Some(ids) = visible {
            self.visible_ids = ids;
        }
        if !self.ops.finish(token) {
            self.diagnostics.record(
                LogLevel::Warning,
                "scan",
                "Scan finished after a newer generation began",
            );
        }
        self.diagnostics.record(
            LogLevel::Info,
            "folder",
            format!("Opened Folder with {} photos", self.photo_count),
        );
        Ok(())
    }

    /// Second pass after sidecar enrich. The FFI index is already rebuilt.
    pub fn install_enrich(&mut self, prepared: PreparedCatalog, visible: Option<Arc<Vec<String>>>) {
        self.photo_count = prepared.photo_count;
        self.hosts = prepared.hosts;
        self.last_folders = prepared.last_folders;
        self.scan_photo_ids = prepared.scan_photo_ids;
        self.rebuild_host_lookups();
        self.last_apply_ms = prepared.apply_ms;
        self.prune_selection();
        if let Some(ids) = visible {
            self.visible_ids = ids;
        }
        if self.folder.is_some() {
            self.refresh_person_state();
        }
        localcore_trace::event(
            "catalog",
            format!("install_enrich photos={}", self.photo_count),
        );
    }

    /// Drop deleted ids from the FFI table and this session's host maps.
    /// Persists a leftover snapshot when host persist is on. Does not scan.
    pub fn apply_photos_removed(&mut self, ids: &[String]) -> RemovePhotosResult {
        let result = self.index.remove_photos(ids.to_vec());
        if result.removed_ids.is_empty() {
            return result;
        }
        let drop: HashSet<&str> = result.removed_ids.iter().map(String::as_str).collect();
        let keep_paths: HashSet<String> = self
            .index
            .export_photos()
            .iter()
            .map(|photo| photo.path.clone())
            .collect();
        self.hosts.retain(|_, host| keep_paths.contains(&host.path));
        self.scan_photo_ids = self.index.scan_order_photo_ids();
        self.last_folders = self.index.export_folders();
        self.rebuild_host_lookups();
        self.photo_count = result.photo_count as usize;
        self.visible_ids = Arc::new(result.visible_photo_ids.clone());
        drop_intent_ids(&mut self.photos_intent, &drop);
        drop_intent_ids(&mut self.current_intent, &drop);
        self.prune_selection();
        self.invalidate_memories();
        if self.persist_host {
            if let Some(folder) = self.folder.clone() {
                let persist = ScanCatalogHost {
                    flat_photos: self.index.export_photos(),
                    folders: self.last_folders.clone(),
                    needs_enrichment: false,
                    sidecar_manifest: Vec::new(),
                    added_paths: Vec::new(),
                    removed_paths: Vec::new(),
                    modified_paths: Vec::new(),
                    failed_directory_paths: Vec::new(),
                    timings: ScanMetrics::default(),
                };
                if let Err(error) = Self::persist_scan_snapshot(&folder, &persist) {
                    localcore_trace::event("catalog", format!("delete snapshot: {error}"));
                }
            }
        }
        localcore_trace::event(
            "catalog",
            format!(
                "apply_photos_removed dropped={} left={} gen={}",
                result.removed_ids.len(),
                result.photo_count,
                result.generation
            ),
        );
        result
    }

    /// Index + folders + path map. Tests call this on the test thread;
    /// GTK prepares on the scan worker and only [`install_prepared`]s here.
    pub fn apply_catalog(
        &mut self,
        catalog: ScanCatalogHost,
        folder: Option<&Path>,
    ) -> Result<(), ShellError> {
        let ui = Self::prepare_ui(&self.index, catalog, true, None, &self.scan_work);
        let visible = Arc::new(
            ui.photos
                .items()
                .iter()
                .map(|item| item.id.clone())
                .collect(),
        );
        self.install_prepared(ui.catalog, folder, Some(visible))?;
        self.clear_work();
        Ok(())
    }

    #[must_use]
    pub fn last_apply_ms(&self) -> f64 {
        self.last_apply_ms
    }

    #[must_use]
    pub fn last_leftover_ms(&self) -> f64 {
        self.last_leftover_ms
    }

    /// Re-walk files. Scan Photos (tagging / faces / places) is a separate
    /// worker started by the window.
    pub fn scan_photos_without_pack(&mut self) -> Result<(), ShellError> {
        self.diagnostics
            .record(LogLevel::Info, "scan", "Reload library — file scan only");
        self.reload()
    }

    /// Photos the analysis worker needs. Cloned off the FFI index.
    #[must_use]
    pub fn analysis_photos(&self) -> Vec<PhotoFile> {
        self.index
            .export_photos()
            .into_iter()
            .map(photo_file_from_scan)
            .collect()
    }

    #[must_use]
    pub fn analysis_cancel_flag(&self) -> Arc<AtomicBool> {
        self.analysis_cancel.clone()
    }

    pub fn prepare_analysis(&self) {
        self.analysis_cancel
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn cancel_analysis(&self) {
        self.analysis_cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn store_analysis_summary(&mut self, summary: AnalysisSummary) {
        self.last_analysis = Some(summary);
    }

    #[must_use]
    pub fn last_analysis(&self) -> Option<&AnalysisSummary> {
        self.last_analysis.as_ref()
    }

    /// Tag buckets in one namespace (`people`, `places`, `objects`, `scenes`).
    #[must_use]
    pub fn tag_namespace_count(&self, namespace: &str) -> usize {
        self.index
            .tag_suggestions()
            .tags
            .iter()
            .filter(|tag| {
                tag.namespace
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(namespace))
            })
            .count()
    }

    /// Re-read sidecars analysis just wrote, then flatten for GTK.
    pub fn overlay_analysis_writes(
        index: &LibraryIndex,
        folders: Vec<ScannedFolderHost>,
        written: &[String],
        persist_folder: Option<&Path>,
        work: &Mutex<Option<WorkProgress>>,
    ) -> PreparedUi {
        let want: HashSet<&str> = written.iter().map(String::as_str).collect();
        let mut photos = index.export_photos();
        for photo in &mut photos {
            if want.contains(photo.path.as_str()) {
                photo.enriched_file_date = None;
            }
        }
        let catalog = ScanCatalogHost {
            flat_photos: photos,
            folders,
            needs_enrichment: true,
            sidecar_manifest: Vec::new(),
            added_paths: Vec::new(),
            removed_paths: Vec::new(),
            modified_paths: Vec::new(),
            failed_directory_paths: Vec::new(),
            timings: ScanMetrics::default(),
        };
        let ui = Self::prepare_ui(index, catalog, true, persist_folder, work);
        if let Some(folder) = persist_folder {
            let persist = ScanCatalogHost {
                flat_photos: index.export_photos(),
                folders: ui.catalog.last_folders.clone(),
                needs_enrichment: false,
                sidecar_manifest: Vec::new(),
                added_paths: Vec::new(),
                removed_paths: Vec::new(),
                modified_paths: Vec::new(),
                failed_directory_paths: Vec::new(),
                timings: ScanMetrics::default(),
            };
            if let Err(error) = Self::persist_scan_snapshot(folder, &persist) {
                localcore_trace::event("catalog", format!("analysis snapshot: {error}"));
            }
        }
        ui
    }

    #[must_use]
    pub fn folder(&self) -> Option<&Path> {
        self.folder.as_deref()
    }

    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    #[must_use]
    pub fn persist_host(&self) -> bool {
        self.persist_host
    }

    #[must_use]
    pub fn leftover_availability(&self) -> LibraryAvailability {
        // Kit UI does not keep leftover `LibraryState` (a second 20k index).
        library_availability(self.folder.as_deref(), None)
    }

    #[must_use]
    pub fn photo_count(&self) -> usize {
        self.photo_count
    }

    #[must_use]
    pub fn scan_status(&self) -> ScanStatus {
        self.settings_progress().into()
    }

    /// Settings row: live job, last Scan Photos line, or idle.
    #[must_use]
    pub fn settings_progress(&self) -> ProgressDisplay {
        if let Ok(guard) = self.scan_work.lock() {
            if let Some(work) = guard.as_ref() {
                return work.display().clone();
            }
        }
        if let Some(summary) = &self.last_analysis {
            return ProgressDisplay {
                label: summary.toast_line(),
                detail: None,
                fraction: None,
                cancel: false,
            };
        }
        idle_scan_status()
    }

    /// Chrome progress after the 500 ms reveal.
    #[must_use]
    pub fn chrome_progress(&self) -> Option<ProgressDisplay> {
        self.scan_work
            .lock()
            .ok()?
            .as_ref()?
            .chrome(std::time::Instant::now())
            .cloned()
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
        if let PhotoIntent::Library { tags, .. } = &self.photos_intent {
            self.photos_intent = PhotoIntent::Library {
                query: self.query.clone(),
                tags: tags.clone(),
            };
        }
    }

    pub fn set_required_tags(&mut self, tags: Vec<String>) {
        self.required_tags = tags;
        if let PhotoIntent::Library { query, .. } = &self.photos_intent {
            self.photos_intent = PhotoIntent::Library {
                query: query.clone(),
                tags: self.required_tags.clone(),
            };
        }
    }

    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Categorized Photos search hits for the live query.
    #[must_use]
    pub fn search_hits(&self, query: &str) -> Vec<gallery_ffi::SearchHit> {
        self.index.search_hits(query)
    }

    #[must_use]
    pub fn required_tags(&self) -> &[String] {
        &self.required_tags
    }

    pub fn diagnostics(&self) -> &LogStore {
        &self.diagnostics
    }

    pub fn diagnostics_mut(&mut self) -> &mut LogStore {
        &mut self.diagnostics
    }

    pub fn record(&mut self, level: LogLevel, category: &str, message: impl Into<String>) {
        self.diagnostics.record(level, category, message);
    }

    #[must_use]
    pub fn leftover_snapshot_loaded(&self) -> bool {
        self.leftover_loaded
    }

    /// Mark leftover snapshot persist finished for `folder` if it is still live.
    pub fn mark_leftover_loaded(&mut self, folder: &Path, worker_ms: f64) {
        if self.folder.as_deref() == Some(folder) {
            self.leftover_loaded = true;
            self.last_leftover_ms = worker_ms;
        }
    }

    /// Walk+enrich+write leftover snapshot. The kit UI path persists the
    /// same file via [`Self::persist_scan_snapshot`] after enrich so this
    /// second walk is not started from GTK.
    pub fn persist_leftover_snapshot(folder: &Path, cancel: &AtomicBool) -> Result<(), ShellError> {
        let _span = localcore_trace::span_always("catalog", "leftover_open")
            .extra("path", folder.display());
        localcore_trace::event(
            "catalog",
            "leftover_open walks+enriches on worker; snapshot only; leftover index dropped",
        );
        leftover_open(folder, cancel)?;
        Ok(())
    }

    pub fn apply_photo_intent(&mut self, intent: PhotoIntent) -> ViewStructure {
        let n = match &intent {
            PhotoIntent::Library { .. } => self.photo_count,
            PhotoIntent::Ids { ids, .. } => ids.len(),
        };
        let _span = localcore_trace::span_always("photos", "apply_photo_intent")
            .extra("n", n)
            .extra(
                "kind",
                match &intent {
                    PhotoIntent::Library { .. } => "library",
                    PhotoIntent::Ids { .. } => "ids",
                },
            );
        self.current_intent = intent.clone();
        let structure = Self::apply_intent_on_index(&self.index, &intent);
        self.remember_visible_ids();
        structure
    }

    /// Record the intent without FFI. Workers call [`Self::apply_intent_on_index`].
    pub fn set_current_intent(&mut self, intent: PhotoIntent) {
        self.current_intent = intent;
    }

    pub fn set_visible_ids(&mut self, ids: Arc<Vec<String>>) {
        self.visible_ids = ids;
    }

    fn remember_visible_ids(&mut self) {
        self.visible_ids = Arc::new(self.index.visible_photo_ids());
    }

    /// Current photo projection. Cached at the last intent apply.
    #[must_use]
    pub fn visible_photo_ids(&self) -> Arc<Vec<String>> {
        self.visible_ids.clone()
    }

    #[must_use]
    pub fn is_selecting(&self) -> bool {
        self.selecting
    }

    pub fn set_selecting(&mut self, on: bool) {
        self.selecting = on;
        if !on {
            self.selected.clear();
        }
    }

    pub fn toggle_selected(&mut self, id: &str) {
        if !self.selected.remove(id) {
            self.selected.insert(id.to_string());
        }
    }

    pub fn select_visible(&mut self) {
        for id in self.visible_ids.iter() {
            self.selected.insert(id.clone());
        }
    }

    pub fn clear_selection(&mut self) {
        self.selected.clear();
    }

    #[must_use]
    pub fn selected_ids(&self) -> &BTreeSet<String> {
        &self.selected
    }

    #[must_use]
    pub fn is_selected(&self, id: &str) -> bool {
        self.selected.contains(id)
    }

    #[must_use]
    pub fn selected_targets(&self) -> Vec<MutationTarget> {
        self.selected
            .iter()
            .filter_map(|id| self.target_for_id(id))
            .collect()
    }

    #[must_use]
    pub fn target_for_id(&self, id: &str) -> Option<MutationTarget> {
        let host = self.hosts.get(id)?;
        Some(MutationTarget {
            id: id.to_string(),
            path: PathBuf::from(&host.path),
            is_video: host.is_video,
            live_photo_video_path: host.live_photo_video_path.as_ref().map(PathBuf::from),
        })
    }

    fn prune_selection(&mut self) {
        self.selected.retain(|id| self.hosts.contains_key(id));
        if !self.selecting {
            self.selected.clear();
        }
    }

    pub fn restore_photos_intent(&mut self) -> ViewStructure {
        self.apply_photo_intent(self.photos_intent.clone())
    }

    #[must_use]
    pub fn current_intent(&self) -> &PhotoIntent {
        &self.current_intent
    }

    #[must_use]
    pub fn photos_intent(&self) -> &PhotoIntent {
        &self.photos_intent
    }

    pub fn apply_photos_tab(&mut self) -> ViewStructure {
        self.photos_intent = PhotoIntent::Library {
            query: self.query.clone(),
            tags: self.required_tags.clone(),
        };
        self.restore_photos_intent()
    }

    /// Thin test helper: current photo intent, first window.
    pub fn photo_page(&self) -> Result<PhotoPage, ShellError> {
        let structure = self.index.photo_structure();
        let items = self.photo_window("photos", 0, 8)?;
        Ok(PhotoPage { structure, items })
    }

    pub fn photo_window(
        &self,
        section: &str,
        offset: u64,
        limit: u64,
    ) -> Result<Vec<GalleryMediaItem>, ShellError> {
        let structure = self.index.photo_structure();
        match self
            .index
            .photo_window(section.to_string(), offset, limit, structure.generation)
        {
            Ok(items) => Ok(items),
            Err(ViewError::StaleGeneration { .. }) => {
                let fresh = self.index.photo_structure();
                Ok(self
                    .index
                    .photo_window(section.to_string(), offset, limit, fresh.generation)?)
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn folder_listing(&self, parent: Option<String>) -> Result<TextPage, ShellError> {
        let _span = localcore_trace::span("folders", "folder_listing")
            .extra("parent", parent.as_deref().unwrap_or("root"));
        let structure = self.index.folder_structure(parent.clone());
        match self
            .index
            .folder_window("folders".into(), 0, PHOTO_PAGE, structure.generation)
        {
            Ok(rows) => Ok(TextPage { structure, rows }),
            Err(ViewError::StaleGeneration { .. }) => {
                let fresh = self.index.folder_structure(parent);
                let rows =
                    self.index
                        .folder_window("folders".into(), 0, PHOTO_PAGE, fresh.generation)?;
                Ok(TextPage {
                    structure: fresh,
                    rows,
                })
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn folder_window(
        &self,
        offset: u64,
        limit: u64,
        generation: u64,
    ) -> Result<Vec<GalleryTextRow>, ShellError> {
        match self
            .index
            .folder_window("folders".into(), offset, limit, generation)
        {
            Ok(rows) => Ok(rows),
            Err(ViewError::StaleGeneration { .. }) => {
                let fresh = self.index.folder_structure(None);
                Ok(self
                    .index
                    .folder_window("folders".into(), offset, limit, fresh.generation)?)
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn folder_photo_ids(&self, folder_id: &str) -> Vec<String> {
        let _span =
            localcore_trace::span_always("folders", "folder_photo_ids").extra("id", folder_id);
        let ids = self.index.folder_photo_ids(folder_id.to_string());
        localcore_trace::event("folders", format!("folder_photo_ids n={}", ids.len()));
        ids
    }

    fn rebuild_host_lookups(&mut self) {
        self.folder_by_id = self
            .last_folders
            .iter()
            .enumerate()
            .map(|(index, folder)| (folder.id.clone(), index))
            .collect();
        self.path_to_photo = self
            .hosts
            .iter()
            .map(|(id, host)| (host.path.clone(), id.clone()))
            .collect();
    }

    /// Child folders for the explorer. Reads the scan table; does not
    /// call [`LibraryIndex::folder_structure`].
    #[must_use]
    pub fn folder_entries(&self, parent: Option<&str>) -> Vec<crate::folders::FolderEntry> {
        crate::folders::listing_entries(&self.last_folders, parent)
    }

    #[must_use]
    pub fn folder_entry(&self, id: &str) -> Option<crate::folders::FolderEntry> {
        let &index = self.folder_by_id.get(id)?;
        Some(crate::folders::entry_at(&self.last_folders, index))
    }

    #[must_use]
    pub fn folder_crumbs(&self, id: Option<&str>) -> Vec<crate::folders::FolderEntry> {
        crate::folders::crumb_trail(&self.last_folders, id)
    }

    #[must_use]
    pub fn folder_path_stack(&self, id: Option<&str>) -> Vec<Option<String>> {
        crate::folders::path_stack(&self.last_folders, id)
    }

    /// Folder whose own photos belong on the explorer at `current`.
    ///
    /// At the tab root that is the hidden scan root (when there is one).
    #[must_use]
    pub fn explorer_photo_folder_id(&self, current: Option<&str>) -> Option<String> {
        if let Some(id) = current {
            return Some(id.to_string());
        }
        crate::folders::skipped_root_id(&self.last_folders).map(str::to_string)
    }

    /// Own photo ids for `folder_id` as a slice of the scan order. No clone.
    #[must_use]
    pub fn folder_own_photo_ids(&self, folder_id: &str) -> &[String] {
        let Some(&index) = self.folder_by_id.get(folder_id) else {
            return &[];
        };
        let folder = &self.last_folders[index];
        let start = folder.photo_start as usize;
        let end = start.saturating_add(folder.photo_count as usize);
        self.scan_photo_ids
            .get(start..end.min(self.scan_photo_ids.len()))
            .unwrap_or(&[])
    }

    pub fn photo_ids_for_tag(&self, tag: &str) -> Vec<String> {
        let _span = localcore_trace::span("photos", "photo_ids_for_tag").extra("tag", tag);
        let ids = self.index.photo_ids_for_tag(tag.to_string());
        localcore_trace::event("photos", format!("photo_ids_for_tag n={}", ids.len()));
        ids
    }

    /// One cover photo for a person tile.
    pub fn person_cover_photo_id(&self, tag: &str) -> Option<String> {
        self.index.featured_photo_id(tag.to_string()).or_else(|| {
            self.index
                .photo_ids_for_tag(tag.to_string())
                .into_iter()
                .next()
        })
    }

    pub fn people_page(&self) -> Result<TextPage, ShellError> {
        let structure = self.index.people_structure();
        let rows =
            match self
                .index
                .people_window("people".into(), 0, PHOTO_PAGE, structure.generation)
            {
                Ok(rows) => rows,
                Err(ViewError::StaleGeneration { .. }) => {
                    let fresh = self.index.people_structure();
                    self.index
                        .people_window("people".into(), 0, PHOTO_PAGE, fresh.generation)?
                }
                Err(error) => return Err(error.into()),
            };
        Ok(TextPage { structure, rows })
    }

    pub fn collection_section(&self, section_id: &str) -> Result<TextPage, ShellError> {
        let structure = self.index.collection_structure();
        let rows = match self.index.collection_window(
            section_id.to_string(),
            0,
            PHOTO_PAGE,
            structure.generation,
        ) {
            Ok(rows) => rows,
            Err(ViewError::StaleGeneration { .. }) => {
                let fresh = self.index.collection_structure();
                self.index.collection_window(
                    section_id.to_string(),
                    0,
                    PHOTO_PAGE,
                    fresh.generation,
                )?
            }
            Err(error) => return Err(error.into()),
        };
        Ok(TextPage { structure, rows })
    }

    pub fn collection_structure(&self) -> ViewStructure {
        self.index.collection_structure()
    }

    pub fn people_structure(&self) -> ViewStructure {
        self.index.people_structure()
    }

    pub fn photo_structure(&self) -> ViewStructure {
        let _span = localcore_trace::span("photos", "photo_structure");
        let structure = self.index.photo_structure();
        if localcore_trace::verbose() {
            let n: usize = structure
                .sections
                .iter()
                .map(|section| section.item_ids.len())
                .sum();
            localcore_trace::detail(
                "photos",
                format!(
                    "photo_structure sections={} ids={n} gen={}",
                    structure.sections.len(),
                    structure.generation
                ),
            );
        }
        structure
    }

    pub fn tag_page(&self) -> Result<TextPage, ShellError> {
        let structure = self.index.tag_structure();
        let rows = match self
            .index
            .tag_window("tags".into(), 0, PHOTO_PAGE, structure.generation)
        {
            Ok(rows) => rows,
            Err(ViewError::StaleGeneration { .. }) => {
                let fresh = self.index.tag_structure();
                self.index
                    .tag_window("tags".into(), 0, PHOTO_PAGE, fresh.generation)?
            }
            Err(error) => return Err(error.into()),
        };
        Ok(TextPage { structure, rows })
    }

    pub fn folder_page(&self) -> Result<TextPage, ShellError> {
        self.folder_listing(None)
    }

    #[must_use]
    pub fn path_for_photo(&self, id: &str) -> Option<&str> {
        self.hosts.get(id).map(|host| host.path.as_str())
    }

    #[must_use]
    pub fn host_for_photo(&self, id: &str) -> Option<&PhotoHost> {
        self.hosts.get(id)
    }

    #[must_use]
    pub fn last_folders(&self) -> &[ScannedFolderHost] {
        &self.last_folders
    }

    /// Leaf folders with photos — leftover `event_folders`, newest first.
    #[must_use]
    pub fn event_folders(&self) -> Vec<EventFolder> {
        event_folder_rows(&self.last_folders, &self.scan_photo_ids, &self.hosts)
    }

    /// Cover photo for a folder id: `last_folders[].cover_photo_path` joined to hosts.
    #[must_use]
    pub fn folder_cover(&self, folder_id: &str) -> Option<(String, PhotoHost)> {
        let &index = self.folder_by_id.get(folder_id)?;
        let path = self.last_folders.get(index)?.cover_photo_path.as_ref()?;
        let id = self.path_to_photo.get(path)?;
        let host = self.hosts.get(id)?.clone();
        Some((id.clone(), host))
    }

    pub fn invalidate_memories(&mut self) {
        if let Some(generator) = self.memory_gen.take() {
            generator.cancel();
        }
        self.memories = MemoriesCache::default();
    }

    #[must_use]
    pub fn memories(&self) -> &[MemoryStructure] {
        &self.memories.items
    }

    #[must_use]
    pub fn memories_cache(&self) -> &MemoriesCache {
        &self.memories
    }

    pub fn store_memories(&mut self, items: Vec<MemoryStructure>) {
        self.memories = MemoriesCache {
            generation: self.index.view_generation(),
            seed: self.memory_seed.clone(),
            items,
        };
    }

    pub fn memory_context(&self) -> ScheduledMemoryContext {
        ScheduledMemoryContext {
            leaf_folders: leaf_memory_folders(&self.last_folders, &self.scan_photo_ids),
            contacts: self.contacts.clone(),
            person_contact_links: person_links(&self.person_state),
            birthdays_enabled: !self.contacts.is_empty(),
            me_person_path: self.person_state.me.clone(),
            hidden_people: self.person_state.hidden.clone(),
            now: apple_now(),
            time_zone_offset_seconds: 0,
            horizon_offset_seconds: Vec::new(),
            seed: self.memory_seed.clone(),
            seen_memory_ids: Vec::new(),
            surfaced_clusters: Vec::new(),
        }
    }

    #[must_use]
    pub fn person_state(&self) -> &PersonStateStructure {
        &self.person_state
    }

    /// Badge and menu state for one `People/…` tile. Paths compare
    /// case-insensitively so a row id and the log spelling stay aligned.
    #[must_use]
    pub fn person_marks(&self, path: &str, title: &str) -> PersonMarks {
        let path = self.resolve_person_path(path);
        let featured = self
            .person_state
            .featured
            .iter()
            .any(|entry| person_path_eq(entry, &path));
        let me = !self.person_state.me.is_empty() && person_path_eq(&self.person_state.me, &path);
        let hidden = self
            .person_state
            .hidden
            .iter()
            .any(|entry| person_path_eq(entry, &path));
        let link = person_link_state(
            path.clone(),
            title.to_string(),
            self.contacts.clone(),
            person_links(&self.person_state),
        );
        let linked = matches!(link.kind, PersonLinkKind::Manual | PersonLinkKind::Auto);
        let marks = PersonMarks {
            featured,
            me,
            linked,
            hidden,
            link_label: link_menu_label(&link),
        };
        localcore_trace::detail(
            "people",
            format!(
                "marks path={path} title={title} featured={featured} me={me} hidden={hidden} link={}",
                marks.link_label
            ),
        );
        marks
    }

    #[must_use]
    pub fn contacts(&self) -> &[MemoryContactCommandItem] {
        &self.contacts
    }

    pub fn set_contacts_folder(&mut self, folder: Option<PathBuf>) -> Result<(), ShellError> {
        localcore_trace::event(
            "people",
            format!(
                "set_contacts_folder {}",
                folder
                    .as_deref()
                    .map(Path::display)
                    .map(|path| path.to_string())
                    .unwrap_or_else(|| "-".into())
            ),
        );
        self.config.contacts_root = folder;
        if self.persist_host {
            self.config
                .save()
                .map_err(|error| ShellError::Host(localgallery::HostError::from(error)))?;
        }
        self.contacts = load_contacts(self.config.contacts_root.as_deref());
        self.invalidate_memories();
        Ok(())
    }

    pub fn refresh_person_state(&mut self) {
        let Some(root) = self.person_log_root() else {
            localcore_trace::event(
                "people",
                format!(
                    "person log project skipped: no UTF-8 library folder ({})",
                    self.folder
                        .as_deref()
                        .or(self.config.library_root.as_deref())
                        .map(Path::display)
                        .map(|path| path.to_string())
                        .unwrap_or_else(|| "-".into())
                ),
            );
            self.person_state = PersonStateStructure::default();
            self.index
                .set_person_state(self.person_state.clone(), apple_now());
            return;
        };
        match person_log_project_report(root.clone()) {
            Ok(report) => {
                self.person_state = report.state;
                localcore_trace::event(
                    "people",
                    format!(
                        "person log project root={root} {}",
                        person_state_summary(&self.person_state)
                    ),
                );
                if localcore_trace::verbose() {
                    localcore_trace::detail(
                        "people",
                        format!(
                            "person log featured={:?} hidden={:?} me={:?} links={:?}",
                            self.person_state.featured,
                            self.person_state.hidden,
                            self.person_state.me,
                            self.person_state.links
                        ),
                    );
                }
                for tail in report.torn_tails {
                    localcore_trace::event(
                        "people",
                        format!(
                            "person log torn tail {} at {}: {}",
                            tail.path, tail.offset, tail.detail
                        ),
                    );
                    self.diagnostics.record(
                        LogLevel::Warning,
                        "people",
                        format!(
                            "Person log torn tail {} at {}: {}",
                            tail.path, tail.offset, tail.detail
                        ),
                    );
                }
            }
            Err(error) => {
                localcore_trace::event("people", format!("person log project failed: {error}"));
                self.diagnostics.record(
                    LogLevel::Warning,
                    "people",
                    format!("Person log project failed: {error}"),
                );
                self.person_state = PersonStateStructure::default();
            }
        }
        self.index
            .set_person_state(self.person_state.clone(), apple_now());
    }

    fn apply_person_log(index: &LibraryIndex, folder: &Path) {
        let Some(root) = folder.to_str() else {
            localcore_trace::event(
                "people",
                format!(
                    "person log attach skipped: folder not UTF-8 ({})",
                    folder.display()
                ),
            );
            return;
        };
        match person_log_project_report(root.to_string()) {
            Ok(report) => {
                localcore_trace::event(
                    "people",
                    format!(
                        "person log attach root={root} {}",
                        person_state_summary(&report.state)
                    ),
                );
                index.set_person_state(report.state, apple_now());
            }
            Err(error) => {
                localcore_trace::event("people", format!("person log attach failed: {error}"));
            }
        }
    }

    pub fn project_current_hub(&self) -> PreparedHub {
        Self::project_hub(
            &self.index,
            &self.hosts,
            &self.last_folders,
            &self.scan_photo_ids,
        )
    }

    pub fn people_rail_page(&self) -> TextPage {
        load_text_page(self.index.people_rail_structure(), |generation| {
            self.index
                .people_rail_window("people".into(), 0, PHOTO_PAGE, generation)
        })
    }

    pub fn resolve_person_path(&self, id_or_path: &str) -> String {
        self.index
            .person_full_path(id_or_path.to_string())
            .unwrap_or_else(|| id_or_path.to_string())
    }

    pub fn toggle_feature_person(&mut self, path: &str) -> bool {
        let path = self.resolve_person_path(path);
        let featured = self
            .person_state
            .featured
            .iter()
            .any(|entry| person_path_eq(entry, &path));
        let ty = if featured {
            "person_unfeatured"
        } else {
            "person_featured"
        };
        localcore_trace::event(
            "people",
            format!("toggle_feature path={path} currently_featured={featured} -> {ty}"),
        );
        self.append_person(ty, &path_body(&path))
    }

    pub fn set_me_person(&mut self, path: Option<&str>) -> bool {
        match path {
            None => {
                localcore_trace::event("people", "set_me clear");
                self.append_person("person_me_clear", "{}")
            }
            Some(path) => {
                let path = self.resolve_person_path(path);
                localcore_trace::event("people", format!("set_me path={path}"));
                self.append_person("person_me_set", &path_body(&path))
            }
        }
    }

    pub fn hide_person(&mut self, path: &str) -> bool {
        let path = self.resolve_person_path(path);
        localcore_trace::event("people", format!("hide path={path}"));
        self.append_person("person_hidden", &path_body(&path))
    }

    pub fn unhide_person(&mut self, path: &str) -> bool {
        let path = self.resolve_person_path(path);
        localcore_trace::event("people", format!("unhide path={path}"));
        self.append_person("person_unhidden", &path_body(&path))
    }

    pub fn set_contact_link(&mut self, path: &str, contact_id: Option<Option<String>>) -> bool {
        let path = self.resolve_person_path(path);
        localcore_trace::event(
            "people",
            format!(
                "set_contact_link path={path} decision={}",
                match &contact_id {
                    None => "reset".into(),
                    Some(None) => "disabled".into(),
                    Some(Some(id)) => format!("manual:{id}"),
                }
            ),
        );
        match contact_id {
            None => self.append_person("person_contact_link_clear", &path_body(&path)),
            Some(None) => self.append_person(
                "person_contact_link_set",
                &format!("{{\"path\":{},\"disabled\":true}}", json_string(&path)),
            ),
            Some(Some(id)) => self.append_person(
                "person_contact_link_set",
                &format!(
                    "{{\"path\":{},\"contact\":{}}}",
                    json_string(&path),
                    json_string(&id)
                ),
            ),
        }
    }

    pub fn set_featured_photo(&mut self, path: &str, photo_id: &str) -> bool {
        let path = self.resolve_person_path(path);
        self.append_person(
            "featured_photo_set",
            &format!(
                "{{\"path\":{},\"photo\":{}}}",
                json_string(&path),
                json_string(photo_id)
            ),
        )
    }

    fn person_log_root(&self) -> Option<String> {
        self.folder
            .as_ref()
            .or(self.config.library_root.as_ref())
            .and_then(|path| path.to_str().map(str::to_string))
    }

    fn append_person(&mut self, event_type: &str, body_json: &str) -> bool {
        let Some(root) = self.person_log_root() else {
            localcore_trace::event(
                "people",
                format!(
                    "append {event_type} skipped: no UTF-8 library folder device={}",
                    self.config.device_id
                ),
            );
            return false;
        };
        localcore_trace::event(
            "people",
            format!(
                "append {event_type} device={} root={root} body={body_json}",
                self.config.device_id
            ),
        );
        match person_log_append(
            root,
            self.config.device_id.clone(),
            event_type.to_string(),
            body_json.to_string(),
        ) {
            Ok(()) => {
                localcore_trace::event("people", format!("append {event_type} ok"));
                self.refresh_person_state();
                if person_event_affects_memories(event_type) {
                    self.invalidate_memories();
                }
                true
            }
            Err(error) => {
                localcore_trace::event("people", format!("append {event_type} failed: {error}"));
                self.diagnostics.record(
                    LogLevel::Error,
                    "people",
                    format!("Person log append failed ({event_type}): {error}"),
                );
                false
            }
        }
    }

    pub fn take_memory_generator(&mut self) -> Arc<MemoryGenerator> {
        if let Some(previous) = self.memory_gen.take() {
            previous.cancel();
        }
        let generator = MemoryGenerator::new();
        self.memory_gen = Some(generator.clone());
        generator
    }

    /// Sync generate for tests. GTK runs the same call off-thread.
    pub fn ensure_memories(&mut self) -> &[MemoryStructure] {
        let generation = self.index.view_generation();
        if self.memories.generation == generation && self.memories.seed == self.memory_seed {
            return &self.memories.items;
        }
        let generator = self.take_memory_generator();
        let items = generator.generate(self.index.clone(), self.memory_context());
        self.store_memories(items);
        &self.memories.items
    }

    pub fn set_memory_seed(&mut self, seed: String) {
        self.memory_seed = seed;
    }
}

fn load_text_page(
    structure: ViewStructure,
    mut load: impl FnMut(u64) -> Result<Vec<GalleryTextRow>, ViewError>,
) -> TextPage {
    let rows = match load(structure.generation) {
        Ok(rows) => rows,
        Err(ViewError::StaleGeneration { current, .. }) => load(current).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    TextPage { structure, rows }
}

fn sidecar_row_eq(left: &ScannedSidecarHost, right: &ScannedSidecarHost) -> bool {
    left.photo_id == right.photo_id
        && left.sidecar_path == right.sidecar_path
        && left.current_version.modification_date == right.current_version.modification_date
        && left.current_version.size == right.current_version.size
}

fn leftover_open(
    folder: &Path,
    cancel: &AtomicBool,
) -> Result<LibraryState, localgallery::HostError> {
    localgallery::open_library(folder, cancel, None)
}

fn idle_scan_status() -> ProgressDisplay {
    ProgressDisplay {
        label: "Not started".into(),
        detail: None,
        fraction: None,
        cancel: false,
    }
}

impl From<ProgressDisplay> for ScanStatus {
    fn from(display: ProgressDisplay) -> Self {
        Self {
            label: display.label,
            detail: display.detail,
            fraction: display.fraction,
        }
    }
}

fn derived_photo_id(path: &str) -> String {
    gallery_ffi::stable_uuid(path.to_string()).to_uppercase()
}

fn apple_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64() - APPLE_EPOCH_UNIX)
        .unwrap_or(0.0)
}

fn ensure_device_id(config: &mut Config, persist: bool) {
    if valid_device(&config.device_id) {
        return;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(1);
    config.device_id = format!("linux-{nanos:016x}");
    if persist {
        let _ = config.save();
    }
}

fn valid_device(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn load_contacts(root: Option<&Path>) -> Vec<MemoryContactCommandItem> {
    let Some(path) = root else {
        localcore_trace::event("people", "contacts load skipped: no folder");
        return Vec::new();
    };
    let Some(root) = path.to_str() else {
        localcore_trace::event(
            "people",
            format!(
                "contacts load skipped: folder not UTF-8 ({})",
                path.display()
            ),
        );
        return Vec::new();
    };
    let vfs = contacts_core::StdVfs::new(contacts_core::TEMP_PREFIX);
    match contacts_core::Store::open(&vfs, root) {
        Ok(store) => {
            let contacts: Vec<MemoryContactCommandItem> =
                store.cards().iter().map(contact_from_card).collect();
            localcore_trace::event(
                "people",
                format!("contacts loaded n={} root={root}", contacts.len()),
            );
            contacts
        }
        Err(error) => {
            localcore_trace::event(
                "people",
                format!("contacts open failed root={root}: {error}"),
            );
            Vec::new()
        }
    }
}

fn person_state_summary(state: &PersonStateStructure) -> String {
    format!(
        "featured={} hidden={} me={} links={}",
        state.featured.len(),
        state.hidden.len(),
        if state.me.is_empty() {
            "-"
        } else {
            state.me.as_str()
        },
        state.links.len()
    )
}

fn person_event_affects_memories(event_type: &str) -> bool {
    matches!(
        event_type,
        "person_hidden"
            | "person_unhidden"
            | "person_me_set"
            | "person_me_clear"
            | "person_contact_link_set"
            | "person_contact_link_clear"
            | "person_renamed"
    )
}

fn contact_from_card(card: &contacts_core::Card) -> MemoryContactCommandItem {
    let id = if card.local_id.is_empty() {
        format!("vcf:{}", card.file_name)
    } else {
        format!("vcf:{}", card.local_id)
    };
    let (given, family) = if card.given_name.is_empty() && card.family_name.is_empty() {
        (card.display_name(), String::new())
    } else {
        (card.given_name.clone(), card.family_name.clone())
    };
    MemoryContactCommandItem {
        id,
        given_name: given,
        family_name: family,
        birthday_month: card.birthday.as_ref().map(|b| u32::from(b.month)),
        birthday_day: card.birthday.as_ref().map(|b| u32::from(b.day)),
    }
}

fn person_path_eq(left: &str, right: &str) -> bool {
    left == right || left.eq_ignore_ascii_case(right)
}

fn link_menu_label(link: &PersonLinkResolution) -> String {
    let name = format!("{} {}", link.given_name, link.family_name);
    let name = name.trim();
    match link.kind {
        PersonLinkKind::Unlinked => "Link to Contact".into(),
        PersonLinkKind::Disabled => "Birthdays disabled".into(),
        PersonLinkKind::Manual if name.is_empty() => "Linked".into(),
        PersonLinkKind::Manual => format!("Linked: {name}"),
        PersonLinkKind::Auto if name.is_empty() => "Auto-matched".into(),
        PersonLinkKind::Auto => format!("Auto: {name}"),
    }
}

fn person_links(state: &PersonStateStructure) -> Vec<MemoryPersonCommandItem> {
    state
        .links
        .iter()
        .map(|pair| MemoryPersonCommandItem {
            person_path: pair.path.clone(),
            contact_id: if pair.value.is_empty() {
                None
            } else {
                Some(pair.value.clone())
            },
        })
        .collect()
}

fn path_body(path: &str) -> String {
    format!("{{\"path\":{}}}", json_string(path))
}

fn json_string(value: &str) -> String {
    let mut out = String::from('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn civil_day_seed() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    unix_ymd(secs)
}

fn unix_ymd(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let y = y + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn event_folder_rows(
    folders: &[ScannedFolderHost],
    photo_ids: &[String],
    hosts: &HashMap<String, PhotoHost>,
) -> Vec<EventFolder> {
    let parents: HashSet<u32> = folders
        .iter()
        .filter_map(|folder| folder.parent_index)
        .collect();
    let path_to_id: HashMap<&str, &str> = hosts
        .iter()
        .map(|(id, host)| (host.path.as_str(), id.as_str()))
        .collect();
    let mut rows: Vec<(Option<f64>, EventFolder)> = folders
        .iter()
        .enumerate()
        .filter_map(|(index, folder)| {
            if parents.contains(&(index as u32)) || folder.photo_count == 0 {
                return None;
            }
            let start = folder.photo_start as usize;
            let end = start.saturating_add(folder.photo_count as usize);
            let slice = photo_ids.get(start..end.min(photo_ids.len()))?;
            if slice.is_empty() {
                return None;
            }
            let cover_path = folder.cover_photo_path.clone().or_else(|| {
                slice
                    .first()
                    .and_then(|id| hosts.get(id).map(|host| host.path.clone()))
            });
            let cover_photo_id = cover_path
                .as_ref()
                .and_then(|path| path_to_id.get(path.as_str()).map(|id| (*id).to_string()))
                .or_else(|| slice.first().cloned());
            Some((
                folder.date_modified,
                EventFolder {
                    id: folder.id.clone(),
                    name: folder.name.clone(),
                    photo_count: folder.photo_count,
                    cover_photo_id,
                    cover_path,
                },
            ))
        })
        .collect();
    rows.sort_by(|left, right| match (left.0, right.0) {
        (Some(a), Some(b)) => b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left.1.name.cmp(&right.1.name),
    });
    rows.into_iter().map(|(_, row)| row).collect()
}

fn leaf_memory_folders(
    folders: &[ScannedFolderHost],
    photo_ids: &[String],
) -> Vec<MemoryFolderCommandItem> {
    folders
        .iter()
        .enumerate()
        .filter_map(|(index, folder)| {
            let has_children = folders
                .iter()
                .any(|child| child.parent_index == Some(index as u32));
            if has_children || folder.photo_count == 0 {
                return None;
            }
            let start = folder.photo_start as usize;
            let end = start.saturating_add(folder.photo_count as usize);
            Some(MemoryFolderCommandItem {
                id: folder.id.clone(),
                name: folder.name.clone(),
                photo_ids: photo_ids.get(start..end.min(photo_ids.len()))?.to_vec(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gallery_ffi::{HostTagValue, ScanMetrics, ScannedMediaHost, ViewContentState, ViewError};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    const JPEG: &[u8] = &[
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00,
        0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06, 0x07, 0x06,
        0x05, 0x08, 0x07, 0x07, 0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D, 0x0C, 0x0B, 0x0B,
        0x0C, 0x19, 0x12, 0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D, 0x1A, 0x1C, 0x1C, 0x20,
        0x24, 0x2E, 0x27, 0x20, 0x22, 0x2C, 0x23, 0x1C, 0x1C, 0x28, 0x37, 0x29, 0x2C, 0x30, 0x31,
        0x34, 0x34, 0x34, 0x1F, 0x27, 0x39, 0x3D, 0x38, 0x32, 0x3C, 0x2E, 0x33, 0x34, 0x32, 0xFF,
        0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00,
        0x14, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xC4, 0x00, 0x14, 0x10, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xDA,
        0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0x7B, 0xFF, 0xD9,
    ];

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gallery-gtk-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_jpeg(dir: &Path, name: &str) {
        std::fs::write(dir.join(name), JPEG).unwrap();
    }

    fn tag(path: &str) -> HostTagValue {
        let display = path.rsplit('/').next().unwrap_or(path).to_string();
        let namespace = path.split_once('/').map(|(head, _)| head.to_string());
        HostTagValue {
            full_path: path.to_string(),
            namespace,
            display_name: display,
        }
    }

    fn media(path: &str, date: Option<f64>, tags: &[&str]) -> ScannedMediaHost {
        let filename = Path::new(path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("photo")
            .to_string();
        ScannedMediaHost {
            id: derived_photo_id(path),
            path: path.to_string(),
            filename,
            file_size: 1,
            date_taken: date,
            date_from_metadata: date.is_some(),
            is_video: false,
            live_photo_video_path: None,
            hierarchical_tags: tags.iter().copied().map(tag).collect(),
            country_code: None,
            enriched_file_date: None,
            file_modification_date: None,
            gps_latitude: None,
            gps_longitude: None,
            face_regions: Vec::new(),
        }
    }

    fn folder(
        id: &str,
        path: &str,
        name: &str,
        parent_index: Option<u32>,
        photo_start: u32,
        photo_count: u32,
        total_photo_count: i64,
    ) -> ScannedFolderHost {
        ScannedFolderHost {
            id: id.to_string(),
            path: path.to_string(),
            name: name.to_string(),
            parent_index,
            photo_start,
            photo_count,
            cover_photo_path: None,
            total_photo_count,
            date_modified: None,
            date_created: None,
        }
    }

    fn catalog(photos: Vec<ScannedMediaHost>, folders: Vec<ScannedFolderHost>) -> ScanCatalogHost {
        ScanCatalogHost {
            flat_photos: photos,
            folders,
            needs_enrichment: false,
            sidecar_manifest: Vec::new(),
            added_paths: Vec::new(),
            removed_paths: Vec::new(),
            modified_paths: Vec::new(),
            failed_directory_paths: Vec::new(),
            timings: ScanMetrics::default(),
        }
    }

    fn nested_catalog(root: &Path) -> ScanCatalogHost {
        let year = root.join("2024");
        let paris = year.join("paris");
        let root_photo = root.join("root.jpg");
        let year_photo = year.join("year.jpg");
        let paris_photo = paris.join("paris.jpg");
        let photos = vec![
            media(root_photo.to_str().unwrap(), Some(100.0), &["Albums/Trip"]),
            media(
                year_photo.to_str().unwrap(),
                Some(200.0),
                &["Events/Picnic"],
            ),
            media(
                paris_photo.to_str().unwrap(),
                Some(300.0),
                &["People/Ada", "Events/Picnic", "Albums/Trip"],
            ),
        ];
        let folders = vec![
            folder("folder-root", root.to_str().unwrap(), "lib", None, 0, 1, 3),
            folder(
                "folder-2024",
                year.to_str().unwrap(),
                "2024",
                Some(0),
                1,
                1,
                2,
            ),
            folder(
                "folder-paris",
                paris.to_str().unwrap(),
                "paris",
                Some(1),
                2,
                1,
                1,
            ),
        ];
        catalog(photos, folders)
    }

    #[test]
    fn apply_photos_removed_keeps_siblings_and_hosts() {
        let dir = temp_dir();
        let mut session = Session::new(32);
        session
            .apply_catalog(nested_catalog(&dir), Some(&dir))
            .unwrap();
        let paris = session.folder_photo_ids("folder-paris");
        assert_eq!(paris.len(), 1);
        let year_own = session.folder_photo_ids("folder-2024");
        let result = session.apply_photos_removed(&paris);
        assert_eq!(result.removed_ids, paris);
        assert_eq!(result.photo_count, 2);
        assert_eq!(session.photo_count(), 2);
        assert!(session.folder_photo_ids("folder-paris").is_empty());
        assert_eq!(session.folder_photo_ids("folder-2024"), year_own);
        assert!(session.host_for_photo(&paris[0]).is_none());
        assert_eq!(session.last_folders()[0].total_photo_count, 2);
        assert_eq!(session.last_folders()[1].total_photo_count, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn leftover_and_ffi_open_a_temp_folder() {
        let dir = temp_dir();
        write_jpeg(&dir, "probe.jpg");
        let cfg = Config {
            library_root: Some(dir.clone()),
            contacts_root: None,
            device_id: String::new(),
        };
        assert_eq!(
            library_availability(cfg.library_root.as_deref(), None),
            LibraryAvailability::Empty
        );
        let _ = localgallery::config::snapshot_path();
        let _ = localgallery::xdg_thumb::lookup(
            &localgallery::xdg_thumb::cache_root(),
            "missing.jpg",
            localgallery::xdg_thumb::ThumbSize::Large,
        );
        let watched = localgallery::watch::watch_dirs(&dir, None);
        assert!(watched.iter().any(|path| path == &dir));
        let ledger = OpLedger::default();
        let token = ledger.begin(OpKind::Scan);
        assert!(ledger.finish(token));

        let mut session = Session::new(32);
        session.open_folder(&dir).unwrap();
        assert_eq!(session.photo_count(), 1);
        assert!(
            session.last_apply_ms() > 0.0,
            "apply_catalog must record wall time"
        );
        assert_eq!(
            session.last_leftover_ms(),
            0.0,
            "apply_catalog must not leftover_open on the caller"
        );
        assert!(
            !session.leftover_snapshot_loaded(),
            "leftover snapshot persist is a worker, not apply_catalog"
        );
        assert!(session.scan_status().label.contains("Not started"));

        let photos = session.photo_page().unwrap();
        assert_eq!(photos.structure.state, ViewContentState::Content);
        assert_eq!(photos.items.len(), 1);
        assert!(photos.items[0].thumbnail_ref.ends_with("probe.jpg"));
        assert!(photos.items.len() <= MAX_VIEW_WINDOW);
        assert!(session.path_for_photo(&photos.items[0].id).is_some());

        let stale = session
            .index()
            .photo_window(
                "photos".into(),
                0,
                8,
                photos.structure.generation.saturating_sub(1),
            )
            .expect_err("older generation is refused");
        assert!(matches!(stale, ViewError::StaleGeneration { .. }));
        let retried = session.photo_window("photos", 0, 8).unwrap();
        assert_eq!(retried.len(), 1);

        let folders = session.folder_page().unwrap();
        assert_eq!(folders.structure.sections[0].id, "folders");

        session.set_query("missing-name".into());
        session.apply_photos_tab();
        let filtered = session.photo_page().unwrap();
        assert!(filtered.items.is_empty() || filtered.structure.generation > 0);

        session.scan_photos_without_pack().unwrap();
        assert!(session.scan_status().label.contains("Not started"));
        assert_eq!(session.analysis_photos().len(), 1);
        assert_eq!(session.tag_namespace_count("people"), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn overlay_analysis_writes_rebuilds_the_index() {
        let dir = temp_dir();
        write_jpeg(&dir, "probe.jpg");
        let mut session = Session::new(32);
        session.open_folder(&dir).unwrap();
        let path = session.analysis_photos()[0].path().to_string();
        let folders = session.last_folders().to_vec();
        let work = session.scan_work_arc();
        let ui = Session::overlay_analysis_writes(session.index(), folders, &[path], None, &work);
        assert_eq!(ui.catalog.photo_count, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nested_tree_folder_person_and_album_drill_in() {
        let dir = temp_dir();
        let mut session = Session::new(32);
        session
            .apply_catalog(nested_catalog(&dir), Some(&dir))
            .unwrap();
        assert_eq!(session.photo_count(), 3);

        let root = session.folder_listing(None).unwrap();
        assert_eq!(root.rows.len(), 1);
        assert_eq!(root.rows[0].title, "2024");
        assert_eq!(root.rows[0].id, "folder-2024");

        let year_own = session.folder_photo_ids("folder-2024");
        assert_eq!(year_own.len(), 1, "own slice, not the recursive total");
        let paris_own = session.folder_photo_ids("folder-paris");
        assert_eq!(paris_own.len(), 1);

        let drilled = session.apply_photo_intent(PhotoIntent::Ids {
            view_id: "folder:folder-paris".into(),
            ids: paris_own.clone(),
            query: String::new(),
            tags: Vec::new(),
        });
        let items = session.photo_window("photos", 0, 8).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, paris_own[0]);
        assert!(drilled.generation > 0);

        session
            .apply_catalog(nested_catalog(&dir), Some(&dir))
            .unwrap();
        let stale = session
            .index()
            .photo_window("photos".into(), 0, 8, drilled.generation);
        assert!(matches!(stale, Err(ViewError::StaleGeneration { .. })));
        session.apply_photos_tab();
        assert_eq!(session.photo_window("photos", 0, 8).unwrap().len(), 3);

        let people = session.people_page().unwrap();
        assert_eq!(people.rows.len(), 1);
        assert_eq!(people.rows[0].title, "Ada");
        let person_ids = session.photo_ids_for_tag(&people.rows[0].id);
        assert_eq!(person_ids, paris_own);

        let events = session.collection_section("events").unwrap();
        assert!(events.rows.iter().any(|row| row.title == "Picnic"));
        let albums = session.collection_section("albums").unwrap();
        assert!(albums.rows.iter().any(|row| row.title == "Trip"));
        let album_ids = session.photo_ids_for_tag(&albums.rows[0].id);
        assert!(!album_ids.is_empty());
        session.apply_photo_intent(PhotoIntent::Ids {
            view_id: format!("album:{}", albums.rows[0].id),
            ids: album_ids.clone(),
            query: String::new(),
            tags: Vec::new(),
        });
        let album_items = session.photo_window("photos", 0, 8).unwrap();
        assert_eq!(
            album_items
                .iter()
                .map(|item| item.id.clone())
                .collect::<Vec<_>>(),
            album_ids
        );

        let people_missing = session.index().collection_window(
            "people".into(),
            0,
            8,
            session.collection_structure().generation,
        );
        assert!(matches!(
            people_missing,
            Err(ViewError::SectionNotFound { .. })
        ));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn folder_cover_joins_last_folders_to_hosts() {
        let dir = temp_dir();
        let mut catalog = nested_catalog(&dir);
        let cover = catalog.flat_photos[2].path.clone();
        catalog.folders[2].cover_photo_path = Some(cover.clone());
        let mut session = Session::new(32);
        session.apply_catalog(catalog, Some(&dir)).unwrap();
        let (id, host) = session.folder_cover("folder-paris").expect("cover");
        assert_eq!(host.path, cover);
        assert_eq!(session.path_for_photo(&id), Some(cover.as_str()));
        assert!(session.folder_cover("folder-2024").is_none());
        let gen = session.photo_structure().generation;
        let children = session.folder_entries(Some("folder-2024"));
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].id, "folder-paris");
        assert_eq!(
            session.folder_own_photo_ids("folder-paris"),
            session.folder_photo_ids("folder-paris")
        );
        assert_eq!(
            session.photo_structure().generation,
            gen,
            "explorer listing must not bump the shared FFI generation"
        );
        let events = session.event_folders();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, "folder-paris");
        assert_eq!(events[0].name, "paris");
        assert_eq!(events[0].photo_count, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_enriches_sidecar_people_and_events() {
        let dir = temp_dir();
        write_jpeg(&dir, "ada.jpg");
        std::fs::write(
            dir.join("ada.jpg.xmp"),
            br#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description xmlns:digiKam="http://www.digikam.org/ns/1.0/">
      <digiKam:TagsList>
        <rdf:Seq>
          <rdf:li>People/Ada</rdf:li>
          <rdf:li>Events/Picnic</rdf:li>
        </rdf:Seq>
      </digiKam:TagsList>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>"#,
        )
        .unwrap();
        let mut session = Session::new(32);
        session.open_folder(&dir).unwrap();
        let people = session.people_page().unwrap();
        assert_eq!(people.rows.len(), 1);
        assert_eq!(people.rows[0].title, "Ada");
        let events = session.collection_section("events").unwrap();
        assert!(events.rows.iter().any(|row| row.title == "Picnic"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn warm_scan_reuses_snapshot_people_without_enrich() {
        let dir = temp_dir();
        write_jpeg(&dir, "ada.jpg");
        write_people_xmp(&dir, "ada.jpg", "Ada");
        let session = Session::new(32);
        let mut catalog =
            Session::scan_with(session.scanner_arc(), session.scan_work_arc(), &dir, None).unwrap();
        assert!(catalog.needs_enrichment);
        Session::enrich_catalog(&mut catalog);
        let snap_path = dir.join("library_snapshot.json");
        Session::persist_scan_snapshot_to(&snap_path, &catalog).unwrap();

        let cache = Session::load_scan_cache_at(&snap_path, &dir).expect("snapshot matches root");
        assert!(
            Session::load_scan_cache_at(&snap_path, Path::new("/other/lib")).is_none(),
            "foreign folder must not reuse this snapshot"
        );

        let warm = Session::scan_with(
            session.scanner_arc(),
            session.scan_work_arc(),
            &dir,
            Some(&cache),
        )
        .unwrap();
        assert!(
            !warm.needs_enrichment,
            "unchanged library must not re-read every sidecar: {:?}",
            warm.timings
        );
        assert!(
            warm.flat_photos.iter().any(|photo| photo
                .hierarchical_tags
                .iter()
                .any(|tag| tag.full_path == "People/Ada")),
            "cached tags must survive the walk"
        );

        let index = LibraryIndex::new();
        let ui = Session::prepare_ui(&index, warm.clone(), false, None, &session.scan_work_arc());
        assert!(!ui.catalog.enriched);
        assert_eq!(ui.hub.people.rows.len(), 1);
        assert_eq!(ui.hub.people.rows[0].title, "Ada");

        std::fs::remove_file(dir.join("ada.jpg.xmp")).unwrap();
        let mut gone = Session::scan_with(
            session.scanner_arc(),
            session.scan_work_arc(),
            &dir,
            Some(&cache),
        )
        .unwrap();
        Session::mark_changed_sidecars(&mut gone, Some(&cache));
        assert!(gone.needs_enrichment);
        Session::enrich_catalog(&mut gone);
        assert!(
            gone.flat_photos
                .iter()
                .all(|photo| photo.hierarchical_tags.is_empty()),
            "deleted sidecar must retract People tags"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn person_log_projects_into_windows_and_memory_context() {
        let dir = temp_dir();
        write_jpeg(&dir, "ada.jpg");
        write_people_xmp(&dir, "ada.jpg", "Ada");
        write_jpeg(&dir, "bob.jpg");
        write_people_xmp(&dir, "bob.jpg", "Bob");
        let mut session = Session::new(32);
        session.open_folder(&dir).unwrap();
        assert!(session.memory_context().hidden_people.is_empty());
        assert!(session.toggle_feature_person("People/Bob"));
        assert!(session.hide_person("People/Ada"));
        assert!(session.set_me_person(Some("People/Bob")));
        let ctx = session.memory_context();
        assert_eq!(ctx.hidden_people, vec!["People/Ada".to_string()]);
        assert_eq!(ctx.me_person_path, "People/Bob");
        let page = session.people_page().unwrap();
        assert_eq!(
            page.rows
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            vec!["People/Bob", "People/Ada"]
        );
        let rail = session.people_rail_page();
        assert_eq!(
            rail.rows
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            vec!["People/Bob"]
        );
        assert_eq!(
            session.person_state().featured,
            vec!["People/Bob".to_string()]
        );
        assert!(session.set_contact_link("People/Bob", Some(None)));
        assert_eq!(
            session.memory_context().person_contact_links[0].contact_id,
            None
        );
        let bob = session.person_marks("People/Bob", "Bob");
        assert!(bob.featured);
        assert!(bob.me);
        assert!(!bob.linked);
        assert!(!bob.hidden);
        let ada = session.person_marks("People/Ada", "Ada");
        assert!(ada.hidden);
        assert!(!ada.featured);
        drop(session);

        let mut again = Session::new(32);
        again.open_folder(&dir).unwrap();
        assert_eq!(again.person_state().hidden, vec!["People/Ada".to_string()]);
        assert_eq!(
            again.person_state().featured,
            vec!["People/Bob".to_string()]
        );
        assert_eq!(again.person_state().me, "People/Bob");
        let bob = again.person_marks("people/bob", "Bob");
        assert!(bob.featured);
        assert!(bob.me);
        assert!(again.person_marks("People/Ada", "Ada").hidden);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prepare_ui_applies_person_log_before_hub() {
        let dir = temp_dir();
        write_jpeg(&dir, "ada.jpg");
        write_people_xmp(&dir, "ada.jpg", "Ada");
        write_jpeg(&dir, "bob.jpg");
        write_people_xmp(&dir, "bob.jpg", "Bob");
        let mut session = Session::new(32);
        session.open_folder(&dir).unwrap();
        assert!(session.toggle_feature_person("People/Bob"));
        assert!(session.hide_person("People/Ada"));
        drop(session);

        let session = Session::new(32);
        let mut catalog =
            Session::scan_with(session.scanner_arc(), session.scan_work_arc(), &dir, None).unwrap();
        Session::enrich_catalog(&mut catalog);

        let stale = LibraryIndex::new();
        let stale_ui = Session::prepare_ui(
            &stale,
            catalog.clone(),
            false,
            None,
            &session.scan_work_arc(),
        );
        let stale_names: Vec<_> = stale_ui
            .hub
            .people
            .rows
            .iter()
            .map(|row| row.title.as_str())
            .collect();
        assert!(
            stale_names.contains(&"Ada") && stale_names.contains(&"Bob"),
            "unfiltered hub should still list every person: {stale_names:?}"
        );

        let live = LibraryIndex::new();
        let live_ui =
            Session::prepare_ui(&live, catalog, false, Some(&dir), &session.scan_work_arc());
        assert_eq!(
            live_ui
                .hub
                .people
                .rows
                .iter()
                .map(|row| row.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Bob"],
            "featured/hidden from .gallery/log must land on the hub before first paint"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn feature_keeps_memories_hide_invalidates() {
        assert!(!person_event_affects_memories("person_featured"));
        assert!(!person_event_affects_memories("person_unfeatured"));
        assert!(!person_event_affects_memories("person_featured_photo"));
        assert!(person_event_affects_memories("person_hidden"));
        assert!(person_event_affects_memories("person_unhidden"));
        assert!(person_event_affects_memories("person_me_set"));
        assert!(person_event_affects_memories("person_contact_link_set"));
    }

    fn write_people_xmp(dir: &std::path::Path, file: &str, name: &str) {
        std::fs::write(
            dir.join(format!("{file}.xmp")),
            format!(
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
  <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
    <rdf:Description xmlns:digiKam="http://www.digikam.org/ns/1.0/">
      <digiKam:TagsList>
        <rdf:Seq>
          <rdf:li>People/{name}</rdf:li>
        </rdf:Seq>
      </digiKam:TagsList>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn memories_cache_hits_same_generation_and_seed() {
        let dir = temp_dir();
        let mut session = Session::new(32);
        session.set_memory_seed("test-day".into());
        session
            .apply_catalog(nested_catalog(&dir), Some(&dir))
            .unwrap();
        let first = session.ensure_memories().to_vec();
        let generation = session.memories_cache().generation;
        let seed = session.memories_cache().seed.clone();
        assert_eq!(seed, "test-day");
        assert_eq!(generation, session.index().view_generation());
        let second = session.ensure_memories().to_vec();
        assert_eq!(first, second);
        assert_eq!(session.memories_cache().generation, generation);

        if let Some(memory) = first.first() {
            let structure = session.apply_photo_intent(PhotoIntent::Ids {
                view_id: memory.id.clone(),
                ids: memory.photo_ids.clone(),
                query: String::new(),
                tags: Vec::new(),
            });
            let items = session.photo_window("photos", 0, PHOTO_PAGE).unwrap();
            let got: Vec<_> = items.into_iter().map(|item| item.id).collect();
            assert_eq!(got, memory.photo_ids);
            assert!(structure.generation > 0);
        } else {
            let ids = session.folder_photo_ids("folder-paris");
            session.apply_photo_intent(PhotoIntent::Ids {
                view_id: "memory-empty".into(),
                ids: ids.clone(),
                query: String::new(),
                tags: Vec::new(),
            });
            let items = session.photo_window("photos", 0, 8).unwrap();
            assert_eq!(
                items.into_iter().map(|item| item.id).collect::<Vec<_>>(),
                ids
            );
        }

        session.invalidate_memories();
        assert!(session.memories().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn paging_three_hundred_photos_is_bounded() {
        let dir = temp_dir();
        let photos: Vec<_> = (0..300)
            .map(|i| {
                media(
                    &format!("{}/p{i:04}.jpg", dir.display()),
                    Some(i as f64),
                    &[],
                )
            })
            .collect();
        let folders = vec![folder(
            "folder-root",
            dir.to_str().unwrap(),
            "lib",
            None,
            0,
            300,
            300,
        )];
        let mut session = Session::new(8);
        session
            .apply_catalog(catalog(photos, folders), Some(&dir))
            .unwrap();
        let structure = session.apply_photos_tab();
        assert!(
            structure
                .sections
                .iter()
                .map(|s| s.item_ids.len())
                .sum::<usize>()
                >= 300
                || session.photo_count() == 300
        );
        let page = session.photo_window("photos", 256, 256).unwrap();
        assert!(page.len() <= MAX_VIEW_WINDOW);
        assert_eq!(page.len(), 44);
        let too_big = session
            .index()
            .photo_window("photos".into(), 0, 257, structure.generation);
        assert!(matches!(too_big, Err(ViewError::WindowTooLarge { .. })));
        let list = crate::paging::ViewList::photos_flat(&session.photo_structure());
        assert_eq!(list.n_items(), 300);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_retry_after_ids_intent_keeps_those_ids() {
        let dir = temp_dir();
        let mut session = Session::new(32);
        session
            .apply_catalog(nested_catalog(&dir), Some(&dir))
            .unwrap();
        let paris_own = session.folder_photo_ids("folder-paris");
        assert_eq!(paris_own.len(), 1);
        session.apply_photo_intent(PhotoIntent::Ids {
            view_id: "folder:folder-paris".into(),
            ids: paris_own.clone(),
            query: String::new(),
            tags: Vec::new(),
        });
        let generation = session.photo_structure().generation;
        // Nested folder_listing writes and bumps the shared FFI generation.
        let _ = session.folder_listing(Some("folder-paris".into())).unwrap();
        assert_ne!(session.photo_structure().generation, generation);
        assert!(matches!(
            session
                .index()
                .photo_window("photos".into(), 0, 8, generation),
            Err(ViewError::StaleGeneration { .. })
        ));
        // Retry re-reads the current structure. apply_photos_tab is what
        // switches back to the library query — stale retry must not do that.
        let retried = session.photo_window("photos", 0, 8).unwrap();
        assert_eq!(
            retried
                .iter()
                .map(|item| item.id.clone())
                .collect::<Vec<_>>(),
            paris_own
        );
        assert!(matches!(
            session.current_intent(),
            PhotoIntent::Ids { view_id, .. } if view_id == "folder:folder-paris"
        ));
        session.apply_photos_tab();
        assert_eq!(session.photo_window("photos", 0, 8).unwrap().len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_after_rebuild_is_refused_then_retry_succeeds() {
        let dir = temp_dir();
        let mut session = Session::new(8);
        session
            .apply_catalog(nested_catalog(&dir), Some(&dir))
            .unwrap();
        let generation = session.photo_structure().generation;
        session
            .apply_catalog(nested_catalog(&dir), Some(&dir))
            .unwrap();
        assert!(matches!(
            session
                .index()
                .photo_window("photos".into(), 0, 8, generation),
            Err(ViewError::StaleGeneration { .. })
        ));
        assert_eq!(session.photo_window("photos", 0, 8).unwrap().len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_listener_reports_a_count_before_apply_catalog() {
        let dir = temp_dir();
        write_jpeg(&dir, "a.jpg");
        write_jpeg(&dir, "b.jpg");
        write_jpeg(&dir, "c.jpg");
        let session = Session::new(8);
        let scanner = session.scanner_arc();
        let status = session.scan_work_arc();
        let (tx, rx) = mpsc::channel();
        let walk = dir.clone();
        thread::spawn(move || {
            let catalog = Session::scan_with(scanner, status, &walk, None);
            let _ = tx.send(catalog);
        });
        let catalog = loop {
            if let Ok(result) = rx.try_recv() {
                break result.unwrap();
            }
            thread::sleep(Duration::from_millis(5));
        };
        let status = session.scan_status();
        let haystack = format!(
            "{} {}",
            status.label,
            status.detail.as_deref().unwrap_or_default()
        );
        assert!(
            haystack.chars().any(|ch| ch.is_ascii_digit()),
            "scan_status should contain a count before apply_catalog: {haystack}"
        );
        let mut session = session;
        session.apply_catalog(catalog, Some(&dir)).unwrap();
        assert_eq!(session.photo_count(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn select_visible_uses_current_ids_and_toggle_is_idempotent() {
        let dir = temp_dir();
        let mut session = Session::new(8);
        session
            .apply_catalog(nested_catalog(&dir), Some(&dir))
            .unwrap();
        let paris = session.folder_photo_ids("folder-paris");
        assert_eq!(paris.len(), 1);
        session.apply_photo_intent(PhotoIntent::Ids {
            view_id: "folder:folder-paris".into(),
            ids: paris.clone(),
            query: String::new(),
            tags: Vec::new(),
        });
        session.set_selecting(true);
        session.select_visible();
        assert_eq!(
            session.selected_ids().iter().cloned().collect::<Vec<_>>(),
            paris
        );
        session.toggle_selected(&paris[0]);
        assert!(session.selected_ids().is_empty());
        session.toggle_selected(&paris[0]);
        session.toggle_selected(&paris[0]);
        assert!(session.selected_ids().is_empty());
        session.set_selecting(false);
        assert!(!session.is_selecting());
        assert!(session.selected_ids().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn host_live_photo_path_round_trips_from_catalog() {
        let path = "/lib/IMG_1.HEIC";
        let live = "/lib/IMG_1.MOV";
        let mut photo = media(path, Some(1.0), &[]);
        photo.live_photo_video_path = Some(live.into());
        let prepared = Session::prepare_catalog(&LibraryIndex::new(), catalog(vec![photo], vec![]));
        let host = prepared
            .hosts
            .get(&derived_photo_id(path))
            .expect("host by derived id");
        assert_eq!(host.live_photo_video_path.as_deref(), Some(live));
        assert!(!host.is_video);
    }

    impl PhotoPage {
        #[allow(dead_code)]
        fn generation_or_empty(&self) -> bool {
            self.items.is_empty() || self.structure.generation > 0
        }
    }
}
