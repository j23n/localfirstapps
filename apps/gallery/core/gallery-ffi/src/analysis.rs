//! UniFFI `AnalysisSession` wrapping [`gallery_session::run_analysis`].
//!
//! Long work runs on a core-owned thread (start / `start_one` / cancel /
//! progress). Places stays [`gallery_session::run_places`] inside that
//! function — this type does not grow a second Places loop.
//!
//! Bindings live in `GalleryCore.swift` / `GalleryCoreFFI.h` (and the
//! Linux shim copies). Do not add a handwritten `AnalysisSession.swift`.

use std::sync::{Arc, Mutex};

use crate::heic::HeicDecoderAdapter;
use crate::places::{PlacesRecordOutcome, PlacesRunRecord, PlacesRunSummary};
use crate::scanner::{photo_from_record, ScanPhoto};
use crate::support::{lock, FinishGuard, RunLock, StartError};
use crate::HeicDecoder;

/// Why an analysis call failed.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum AnalysisError {
    /// `start` / `start_one` while a run is already in flight.
    AlreadyRunning,
    /// A filesystem operation failed, or the OS refused a worker thread.
    Io {
        /// The path involved, when there is one.
        path: String,
        /// Platform message; for logs only.
        detail: String,
    },
}

impl std::fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnalysisError::AlreadyRunning => write!(f, "an analysis run is already in progress"),
            AnalysisError::Io { path, detail } if path.is_empty() => write!(f, "io: {detail}"),
            AnalysisError::Io { path, detail } => write!(f, "io {path}: {detail}"),
        }
    }
}

impl std::error::Error for AnalysisError {}

/// Which phase the banner is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AnalysisPhase {
    /// MobileCLIP tags.
    Tagging,
    /// Detect / embed / cluster.
    Faces,
    /// Offline gazetteer + Places tags.
    Places,
}

/// Which Scan Photos workers to run.
///
/// R6 role: command DTO.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct AnalysisPhases {
    /// MobileCLIP objects / scenes / landmarks.
    pub tagging: bool,
    /// Detect / embed / cluster people.
    pub faces: bool,
    /// Offline gazetteer + Places tags.
    pub places: bool,
}

/// Live counts for the current phase.
///
/// R6 role: command DTO.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AnalysisProgress {
    /// Tagging / faces / places.
    pub phase: AnalysisPhase,
    /// Items finished in this phase.
    pub done: u32,
    /// Items queued for this phase.
    pub total: u32,
}

/// What a finished (or cancelled) run did.
///
/// R6 role: command DTO.
#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct AnalysisRunSummary {
    /// Eligible stills at the start (library snapshot length).
    pub photos: u32,
    /// Tagging sidecar writes. `None` when the phase did not run.
    pub tagged: Option<u32>,
    /// Face sidecar writes.
    pub faces: Option<u32>,
    /// Places result, when that phase ran.
    pub places: Option<PlacesRunSummary>,
    /// Paths any phase wrote.
    pub written_paths: Vec<String>,
    /// Stopped because the user cancelled.
    pub cancelled: bool,
    /// First error the host should toast.
    pub error: Option<String>,
    /// Why tagging/faces were skipped, if they were.
    pub skipped_ml: Option<String>,
}

/// Callbacks into the host app during a run.
///
/// Implementations are called from the core's worker thread and must not
/// block — on Swift the implementation hands off to the main actor and
/// returns.
#[uniffi::export(with_foreign)]
pub trait AnalysisProgressListener: Send + Sync {
    /// Current phase counts. Fires as `run_analysis` hops progress.
    fn on_progress(&self, phase: AnalysisPhase, done: u32, total: u32);

    /// Exactly once per `start` / `start_one`, after the run lock is released.
    fn on_finished(&self, summary: AnalysisRunSummary);
}

/// Cancellable coarse-grained entry point to [`gallery_session::run_analysis`].
#[derive(uniffi::Object)]
pub struct AnalysisSession {
    run: RunLock,
    progress: Arc<Mutex<Option<AnalysisProgress>>>,
    last_summary: Arc<Mutex<Option<AnalysisRunSummary>>>,
}

#[uniffi::export]
impl AnalysisSession {
    /// Create an idle session.
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            run: RunLock::new(),
            progress: Arc::new(Mutex::new(None)),
            last_summary: Arc::new(Mutex::new(None)),
        })
    }

    /// Start tagging → faces → places on a core-owned thread and return at once.
    ///
    /// `pack_dir` is the host-resolved pack (already verified). `None` is a
    /// Places-only run — a pack is not required. `ml_cache_path` is the
    /// shared `gallery-cache.sqlite`; Places uses it as the queue file when
    /// present.
    pub fn start(
        &self,
        photos: Vec<ScanPhoto>,
        pack_dir: Option<String>,
        ml_cache_path: Option<String>,
        geo_cache_path: String,
        phases: AnalysisPhases,
        force: bool,
        decoder: Option<Arc<dyn HeicDecoder>>,
        progress: Option<Arc<dyn AnalysisProgressListener>>,
    ) -> Result<(), AnalysisError> {
        self.launch(
            photos,
            pack_dir,
            ml_cache_path,
            geo_cache_path,
            phases,
            force,
            false,
            decoder,
            progress,
        )
    }

    /// Force one photo through `phases`. Does not reset library-wide ML queues.
    pub fn start_one(
        &self,
        photo: ScanPhoto,
        pack_dir: Option<String>,
        ml_cache_path: Option<String>,
        geo_cache_path: String,
        phases: AnalysisPhases,
        decoder: Option<Arc<dyn HeicDecoder>>,
        progress: Option<Arc<dyn AnalysisProgressListener>>,
    ) -> Result<(), AnalysisError> {
        self.launch(
            vec![photo],
            pack_dir,
            ml_cache_path,
            geo_cache_path,
            phases,
            true,
            true,
            decoder,
            progress,
        )
    }

    /// Ask the in-flight run to stop. Returns immediately.
    pub fn cancel(&self) {
        self.run.request_cancel();
    }

    /// Whether a run is in flight.
    pub fn is_running(&self) -> bool {
        self.run.is_running()
    }

    /// Latest progress hop, if the worker has published one.
    pub fn progress(&self) -> Option<AnalysisProgress> {
        lock(&self.progress).clone()
    }

    /// Summary of the last finished run, if any.
    pub fn last_summary(&self) -> Option<AnalysisRunSummary> {
        lock(&self.last_summary).clone()
    }
}

impl Drop for AnalysisSession {
    fn drop(&mut self) {
        self.run.shutdown();
    }
}

impl AnalysisSession {
    fn launch(
        &self,
        photos: Vec<ScanPhoto>,
        pack_dir: Option<String>,
        ml_cache_path: Option<String>,
        geo_cache_path: String,
        phases: AnalysisPhases,
        force: bool,
        one: bool,
        decoder: Option<Arc<dyn HeicDecoder>>,
        progress: Option<Arc<dyn AnalysisProgressListener>>,
    ) -> Result<(), AnalysisError> {
        let slot = Arc::clone(&self.progress);
        let last = Arc::clone(&self.last_summary);
        *lock(&slot) = None;
        let spawned = self.run.start("gallery-analysis", move |cancel, running| {
            let reporter = progress.clone();
            let progress_slot = Arc::clone(&slot);
            let last_slot = Arc::clone(&last);
            let mut guard =
                FinishGuard::new(running, move |summary: Option<AnalysisRunSummary>| {
                    let summary = summary.unwrap_or_default();
                    *lock(&progress_slot) = None;
                    *lock(&last_slot) = Some(summary.clone());
                    if let Some(listener) = reporter {
                        listener.on_finished(summary);
                    }
                });
            let on_progress = progress.as_ref().map(|listener| {
                let listener = Arc::clone(listener);
                let slot = Arc::clone(&slot);
                Arc::new(move |hop: gallery_session::AnalysisProgress| {
                    let host = AnalysisProgress {
                        phase: phase_from_session(hop.phase),
                        done: hop.done.min(u32::MAX as usize) as u32,
                        total: hop.total.min(u32::MAX as usize) as u32,
                    };
                    *lock(&slot) = Some(host.clone());
                    listener.on_progress(host.phase, host.done, host.total);
                }) as gallery_session::ProgressFn
            });
            let photos: Vec<_> = photos.into_iter().map(photo_from_record).collect();
            let pack = pack_dir.as_deref().and_then(pack_from_dir);
            let ml_cache = ml_cache_path.as_deref().map(std::path::PathBuf::from);
            let heic = decoder.map(|decoder| {
                Arc::new(HeicDecoderAdapter(decoder)) as Arc<dyn gallery_ml::HostHeicDecoder>
            });
            let mut geo_cache = gallery_session::GeoCache::load(&geo_cache_path);
            let request = gallery_session::AnalysisRequest {
                photos: &photos,
                pack: pack.as_ref(),
                ml_cache: ml_cache.as_deref(),
                geo: &gallery_session::Gazetteer,
                geo_cache: &mut geo_cache,
                phases: phases_from_host(phases),
                force,
                cancel: &cancel,
                on_progress,
                heic_decoder: heic,
            };
            let core = if one {
                gallery_session::analysis::run_analysis_one(request)
            } else {
                gallery_session::run_analysis(request)
            };
            let mut out = summary_from_session(core);
            if let Err(error) = geo_cache.save(&geo_cache_path) {
                if out.error.is_none() {
                    out.error = Some(format!("geocode cache io: {error}"));
                }
            }
            guard.summary = Some(out);
        });
        match spawned {
            Ok(()) => Ok(()),
            Err(StartError::AlreadyRunning) => Err(AnalysisError::AlreadyRunning),
            Err(StartError::Spawn(detail)) => Err(AnalysisError::Io {
                path: String::new(),
                detail: format!("could not spawn analysis thread: {detail}"),
            }),
        }
    }
}

fn phases_from_host(phases: AnalysisPhases) -> gallery_session::AnalysisPhases {
    gallery_session::AnalysisPhases {
        tagging: phases.tagging,
        faces: phases.faces,
        places: phases.places,
    }
}

fn phase_from_session(phase: gallery_session::AnalysisPhase) -> AnalysisPhase {
    match phase {
        gallery_session::AnalysisPhase::Tagging => AnalysisPhase::Tagging,
        gallery_session::AnalysisPhase::Faces => AnalysisPhase::Faces,
        gallery_session::AnalysisPhase::Places => AnalysisPhase::Places,
    }
}

fn pack_from_dir(dir: &str) -> Option<gallery_session::PackStatus> {
    gallery_session::resolve_in(&gallery_session::PackRoots {
        bundled: vec![std::path::PathBuf::from(dir)],
        imported: Vec::new(),
    })
}

fn summary_from_session(summary: gallery_session::AnalysisSummary) -> AnalysisRunSummary {
    AnalysisRunSummary {
        photos: summary.photos.min(u32::MAX as usize) as u32,
        tagged: summary.tagged.map(|n| n.min(u32::MAX as usize) as u32),
        faces: summary.faces.map(|n| n.min(u32::MAX as usize) as u32),
        places: summary.places.map(places_to_record),
        written_paths: summary.written_paths,
        cancelled: summary.cancelled,
        error: summary.error,
        skipped_ml: summary.skipped_ml,
    }
}

fn places_to_record(summary: gallery_session::PlacesSummary) -> PlacesRunSummary {
    PlacesRunSummary {
        processed: summary.processed.min(u32::MAX as usize) as u32,
        written: summary.written.min(u32::MAX as usize) as u32,
        skipped: summary.skipped.min(u32::MAX as usize) as u32,
        failed: summary.failed.min(u32::MAX as usize) as u32,
        written_paths: summary.written_paths,
        records: summary
            .records
            .into_iter()
            .map(|record| PlacesRunRecord {
                image_path: record.image_path,
                place_path: record.place_path,
                outcome: match record.outcome {
                    gallery_session::PlaceOutcome::Written => PlacesRecordOutcome::Written,
                    gallery_session::PlaceOutcome::Skipped => PlacesRecordOutcome::Skipped,
                    gallery_session::PlaceOutcome::Failed(detail) => {
                        PlacesRecordOutcome::Failed { detail }
                    }
                },
            })
            .collect(),
        cancelled: summary.cancelled,
        error: summary.error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::ScanRegion;
    use gallery_model::photo::StableId;

    fn photo(path: &str, gps: bool) -> ScanPhoto {
        ScanPhoto {
            id: StableId::for_photo(path).to_string(),
            path: path.to_string(),
            filename: "a".into(),
            file_size: 1,
            date_taken: None,
            date_from_metadata: false,
            is_video: false,
            live_photo_video_path: None,
            hierarchical_tags: Vec::new(),
            country_code: None,
            enriched_file_date: None,
            file_modification_date: None,
            gps_latitude: gps.then_some(48.8566),
            gps_longitude: gps.then_some(2.3522),
            face_regions: Vec::<ScanRegion>::new(),
        }
    }

    fn wait_idle(session: &AnalysisSession) {
        for _ in 0..1_500 {
            if !session.is_running() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("session stayed running");
    }

    fn wait_summary(session: &AnalysisSession) -> AnalysisRunSummary {
        wait_idle(session);
        session
            .last_summary()
            .expect("worker released the run lock without a summary")
    }

    fn places_only() -> AnalysisPhases {
        AnalysisPhases {
            tagging: false,
            faces: false,
            places: true,
        }
    }

    #[test]
    fn run_analysis_places_on_this_thread() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("a.jpg");
        std::fs::write(&image, b"not-a-jpeg").unwrap();
        let cache = dir.path().join("geo-cache.json");
        let photos = vec![photo_from_record(photo(image.to_str().unwrap(), true))];
        let mut geo_cache = gallery_session::GeoCache::load(&cache);
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let summary = gallery_session::run_analysis(gallery_session::AnalysisRequest {
            photos: &photos,
            pack: None,
            ml_cache: None,
            geo: &gallery_session::Gazetteer,
            geo_cache: &mut geo_cache,
            phases: gallery_session::AnalysisPhases {
                tagging: false,
                faces: false,
                places: true,
            },
            force: false,
            cancel: &cancel,
            on_progress: None,
            heic_decoder: None,
        });
        assert!(summary.places.is_some(), "{summary:?}");
        assert_eq!(summary.places.as_ref().unwrap().written, 1);
    }

    #[test]
    fn places_only_run_does_not_require_a_pack() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("a.jpg");
        std::fs::write(&image, b"not-a-jpeg").unwrap();
        let cache = dir.path().join("geo-cache.json");
        let session = AnalysisSession::new();
        session
            .start(
                vec![photo(image.to_str().unwrap(), true)],
                None,
                None,
                cache.to_str().unwrap().to_string(),
                places_only(),
                false,
                None,
                None,
            )
            .unwrap();
        let summary = wait_summary(&session);
        assert!(!summary.cancelled, "{summary:?}");
        assert!(summary.skipped_ml.is_none(), "{:?}", summary.skipped_ml);
        assert!(summary.tagged.is_none());
        assert!(summary.faces.is_none());
        let places = summary.places.expect("places ran");
        assert_eq!(places.written, 1, "{places:?}");
        assert!(cache.is_file());
        assert!(!session.is_running());
    }

    #[test]
    fn start_one_places_writes_without_a_pack() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("one.jpg");
        std::fs::write(&image, b"not-a-jpeg").unwrap();
        let cache = dir.path().join("geo-cache.json");
        let session = AnalysisSession::new();
        session
            .start_one(
                photo(image.to_str().unwrap(), true),
                None,
                None,
                cache.to_str().unwrap().to_string(),
                places_only(),
                None,
                None,
            )
            .unwrap();
        let summary = wait_summary(&session);
        assert_eq!(summary.places.as_ref().map(|p| p.written), Some(1));
        assert!(summary.skipped_ml.is_none(), "{:?}", summary.skipped_ml);
    }

    #[test]
    fn empty_phases_write_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("geo-cache.json");
        let session = AnalysisSession::new();
        session
            .start(
                vec![],
                None,
                None,
                cache.to_str().unwrap().to_string(),
                AnalysisPhases {
                    tagging: false,
                    faces: false,
                    places: false,
                },
                false,
                None,
                None,
            )
            .unwrap();
        let summary = wait_summary(&session);
        assert!(summary.places.is_none());
        assert!(summary.written_paths.is_empty());
        assert!(!summary.cancelled);
    }

    #[test]
    fn a_second_start_while_running_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("a.jpg");
        std::fs::write(&image, b"not-a-jpeg").unwrap();
        let cache = dir.path().join("geo-cache.json");
        let session = AnalysisSession::new();
        session
            .start(
                vec![photo(image.to_str().unwrap(), true)],
                None,
                None,
                cache.to_str().unwrap().to_string(),
                places_only(),
                false,
                None,
                None,
            )
            .unwrap();
        let err = session
            .start(
                vec![],
                None,
                None,
                cache.to_str().unwrap().to_string(),
                places_only(),
                false,
                None,
                None,
            )
            .unwrap_err();
        assert_eq!(err, AnalysisError::AlreadyRunning);
        wait_idle(&session);
    }
}
