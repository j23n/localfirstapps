//! Coarse-grained iOS entry point to the shared Places pass.
//!
//! `PlacesSession` owns cancellation only. Eligibility, cache use, lookup, and
//! sidecar policy all remain in `gallery_session::run_places`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::scanner::{photo_from_record, ScanPhoto};
use gallery_vfs::StdVfs;

/// Progress from the core-owned Places loop.
#[uniffi::export(with_foreign)]
pub trait PlacesProgressListener: Send + Sync {
    /// One more queued photo reached a terminal result.
    fn on_progress(&self, done: u32, total: u32);
}

/// Terminal result for one photo.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum PlacesRecordOutcome {
    /// Sidecar bytes changed.
    Written,
    /// No lookup result or the sidecar already had an equal/better path.
    Skipped,
    /// Authority, lookup, or write failed.
    Failed { detail: String },
}

/// One photo processed by a Places run.
///
/// R6 role: command DTO.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PlacesRunRecord {
    /// Image path.
    pub image_path: String,
    /// Resolved path, when lookup succeeded.
    pub place_path: Option<String>,
    /// Terminal result.
    pub outcome: PlacesRecordOutcome,
}

/// Display and refresh data from one Places run.
///
/// R6 role: command DTO.
#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct PlacesRunSummary {
    /// Photos that reached a terminal result.
    pub processed: u32,
    /// Sidecars actually written.
    pub written: u32,
    /// Misses and already-placed photos.
    pub skipped: u32,
    /// Hard failures.
    pub failed: u32,
    /// Paths whose sidecar changed.
    pub written_paths: Vec<String>,
    /// Per-photo results, in queue order.
    pub records: Vec<PlacesRunRecord>,
    /// The run stopped early.
    pub cancelled: bool,
    /// First hard error.
    pub error: Option<String>,
}

/// Cancellable coarse-grained entry point to `gallery_session::run_places`.
#[derive(uniffi::Object)]
pub struct PlacesSession {
    cancel: AtomicBool,
}

#[uniffi::export]
impl PlacesSession {
    /// Create an idle session.
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            cancel: AtomicBool::new(false),
        })
    }

    /// Clear cancellation before the host launches a run.
    ///
    /// This is separate from [`Self::run`] so cancellation that races ahead
    /// of the worker thread cannot be erased at the start of the run.
    pub fn prepare(&self) {
        self.cancel.store(false, Ordering::Release);
    }

    /// Run eligibility, cache lookup, gazetteer lookup, and sidecar writes.
    pub fn run(
        &self,
        photos: Vec<ScanPhoto>,
        cache_path: String,
        force: bool,
        progress: Option<Arc<dyn PlacesProgressListener>>,
    ) -> PlacesRunSummary {
        let photos = photos
            .into_iter()
            .map(photo_from_record)
            .collect::<Vec<_>>();
        let mut cache = gallery_session::GeoCache::load(&cache_path);
        let on_progress = progress.map(|listener| {
            move |done: usize, total: usize| {
                listener.on_progress(
                    done.min(u32::MAX as usize) as u32,
                    total.min(u32::MAX as usize) as u32,
                );
            }
        });
        let summary = gallery_session::run_places(
            &StdVfs::new(),
            &photos,
            &gallery_session::Gazetteer,
            &mut cache,
            force,
            &self.cancel,
            on_progress
                .as_ref()
                .map(|callback| callback as &dyn Fn(usize, usize)),
        );
        let mut out = summary_to_record(summary);
        if let Err(error) = cache.save(&cache_path) {
            if out.error.is_none() {
                out.error = Some(format!("geocode cache io: {error}"));
            }
        }
        out
    }

    /// Ask the current run to stop.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
}

/// Whether one photo belongs in the shared Places queue.
#[uniffi::export]
pub fn places_candidate(photo: ScanPhoto) -> bool {
    gallery_session::is_places_candidate(&photo_from_record(photo))
}

fn summary_to_record(summary: gallery_session::PlacesSummary) -> PlacesRunSummary {
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

/// Watch debounce, milliseconds. Hosts implement the OS watcher.
#[uniffi::export]
pub fn library_watch_refresh_interval_ms() -> u64 {
    gallery_session::REFRESH_INTERVAL.as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::ScanRegion;
    use gallery_model::photo::StableId;

    fn photo(path: &str) -> ScanPhoto {
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
            gps_latitude: Some(48.8566),
            gps_longitude: Some(2.3522),
            face_regions: Vec::<ScanRegion>::new(),
        }
    }

    #[test]
    fn session_runs_the_shared_places_policy() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("a.jpg");
        std::fs::write(&image, b"not-a-jpeg").unwrap();
        let cache = dir.path().join("geo-cache.json");
        let summary = PlacesSession::new().run(
            vec![photo(image.to_str().unwrap())],
            cache.to_str().unwrap().to_string(),
            false,
            None,
        );
        assert_eq!(summary.processed, 1, "{summary:?}");
        assert_eq!(summary.written, 1, "{summary:?}");
        assert_eq!(summary.records[0].outcome, PlacesRecordOutcome::Written);
        assert!(cache.is_file());
    }

    #[test]
    fn cancellation_before_worker_start_is_not_erased() {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("a.jpg");
        std::fs::write(&image, b"not-a-jpeg").unwrap();
        let cache = dir.path().join("geo-cache.json");
        let session = PlacesSession::new();
        session.prepare();
        session.cancel();

        let summary = session.run(
            vec![photo(image.to_str().unwrap())],
            cache.to_str().unwrap().to_string(),
            false,
            None,
        );

        assert!(summary.cancelled, "{summary:?}");
        assert_eq!(summary.processed, 0, "{summary:?}");
    }
}
