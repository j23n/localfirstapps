//! Places pass: eligibility, cache, offline gazetteer, sidecar write.
//!
//! Work rows live in `places_work` via [`localcore_queue`]. Gazetteer,
//! sidecar skip, and eligibility stay here — the queue only remembers
//! which paths are pending, in flight, or settled.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use localcore_queue::{
    begin, claimable, configure_connection, enqueue, ensure_table, finish_done, finish_failed,
    mark_stale, nfc_path, reclaim_abandoned, release, Queue,
};
use rusqlite::Connection;

use crate::geo::{resolve, wait_until_allowed, GeoCache, GeoError, ReverseGeocoder};
use gallery_meta::places::write_places;
use gallery_meta::read_view;
use gallery_meta::sidecar::{alt_sidecar_path, sidecar_path};
use gallery_model::photo::PhotoFile;
use gallery_vfs::{Vfs, VfsResult};

use crate::eligibility::{is_places_candidate, places_needed};

/// `places_work` in `gallery-cache.sqlite` (or a session-owned file).
const PLACES: Queue = Queue::new("places_work", "place_count");

/// Stamped on `done` rows. Not a model pack — Places is the gazetteer.
const PLACES_PACK: &str = "gazetteer";

/// Capability-local failure code stored on the row; the queue does not
/// interpret it.
const PLACES_FAILED: i64 = 1;

/// SQLite file that holds `places_work`.
///
/// Sibling of the geo-cache JSON (`gallery-cache.sqlite`), the same file
/// tagging and faces use when they share the directory. `PlacesSession`
/// keeps its FFI signature and derives this from `cache_path`.
pub fn places_queue_db_path(geo_cache_path: impl AsRef<Path>) -> PathBuf {
    match geo_cache_path.as_ref().parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join("gallery-cache.sqlite"),
        _ => PathBuf::from("gallery-cache.sqlite"),
    }
}

/// What one Places pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlacesSummary {
    /// Photos considered (eligible at the start of the pass).
    pub considered: usize,
    /// Photos that reached a terminal result before cancellation.
    pub processed: usize,
    /// Sidecars actually written.
    pub written: usize,
    /// Lookups that returned nothing or already-placed writes.
    pub skipped: usize,
    /// Hard failures (after retries).
    pub failed: usize,
    /// Paths whose sidecar changed.
    pub written_paths: Vec<String>,
    /// Per-photo results, in queue order.
    pub records: Vec<PlaceRecord>,
    /// The run stopped early.
    pub cancelled: bool,
    /// First hard error, if any.
    pub error: Option<String>,
}

/// One photo's terminal Places result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceRecord {
    /// Image path.
    pub image_path: String,
    /// Resolved path, when lookup succeeded.
    pub place_path: Option<String>,
    /// What happened.
    pub outcome: PlaceOutcome,
}

/// Terminal result for one queued photo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceOutcome {
    /// Sidecar bytes changed.
    Written,
    /// No lookup result or the sidecar already had an equal/better path.
    Skipped,
    /// Authority, lookup, or write failed.
    Failed(String),
}

/// Walk `photos`, look up each eligible still, write sidecars.
///
/// `queue_db` is `gallery-cache.sqlite` (or another session-owned file).
/// `None` uses an in-memory table (tests). Enqueue is idempotent; a `done`
/// row is not claimed again unless `force` marks it stale. The write skip
/// still re-reads the sidecar so a finished city on disk is not overwritten
/// when the row is stale.
pub fn run_places(
    vfs: &dyn Vfs,
    photos: &[PhotoFile],
    geo: &dyn ReverseGeocoder,
    cache: &mut GeoCache,
    queue_db: Option<&Path>,
    force: bool,
    cancel: &AtomicBool,
    on_progress: Option<&dyn Fn(usize, usize)>,
) -> PlacesSummary {
    let _span = localcore_trace::span_always("queue", "run_places").extra("photos", photos.len());
    let mut by_key: HashMap<String, &PhotoFile> = HashMap::new();
    for photo in photos {
        if is_places_candidate(photo) && (force || places_needed(row_tags(photo), false)) {
            by_key.entry(nfc_path(photo.path())).or_insert(photo);
        }
    }
    let mut summary = PlacesSummary {
        considered: by_key.len(),
        ..PlacesSummary::default()
    };

    let mut conn = match open_places_queue(queue_db) {
        Ok(conn) => conn,
        Err(error) => {
            summary.error = Some(error);
            return summary;
        }
    };

    let paths: Vec<String> = by_key.keys().cloned().collect();
    if persist_step(&mut summary, || enqueue(&mut conn, PLACES, &paths)).is_err() {
        return summary;
    }
    if persist_step(&mut summary, || reclaim_abandoned(&conn, PLACES)).is_err() {
        return summary;
    }
    if force {
        for path in by_key.keys() {
            if persist_step(&mut summary, || mark_stale(&conn, PLACES, path)).is_err() {
                return summary;
            }
        }
    }

    let items = match claimable(&conn, PLACES, 0, None) {
        Ok(items) => items
            .into_iter()
            .filter(|item| by_key.contains_key(&item.path))
            .collect::<Vec<_>>(),
        Err(error) => {
            summary.error = Some(error.to_string());
            return summary;
        }
    };
    let total = items.len();
    if let Some(cb) = on_progress {
        cb(0, total);
    }

    let mut last_lookup = None;
    for (i, item) in items.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            summary.cancelled = true;
            break;
        }
        let Some(photo) = by_key.get(&item.path).copied() else {
            continue;
        };
        match begin(&conn, PLACES, &item.path) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(error) => {
                if summary.error.is_none() {
                    summary.error = Some(error.to_string());
                }
                finish_photo(
                    &mut summary,
                    photo.path(),
                    None,
                    PlaceOutcome::Failed(error.to_string()),
                    i,
                    total,
                    on_progress,
                );
                continue;
            }
        }

        let result = process_photo(vfs, photo, geo, cache, force, cancel, &mut last_lookup);
        match result {
            ProcessResult::Cancelled => {
                if let Err(error) = release(&conn, PLACES, &item.path) {
                    if summary.error.is_none() {
                        summary.error = Some(error.to_string());
                    }
                }
                summary.cancelled = true;
                break;
            }
            ProcessResult::Finished {
                place_path,
                outcome,
            } => {
                if let Err(error) = finish_queue(&conn, &item.path, &outcome) {
                    if summary.error.is_none() {
                        summary.error = Some(error);
                    }
                }
                finish_photo(
                    &mut summary,
                    photo.path(),
                    place_path,
                    outcome,
                    i,
                    total,
                    on_progress,
                );
            }
        }
    }
    summary
}

enum ProcessResult {
    Cancelled,
    Finished {
        place_path: Option<String>,
        outcome: PlaceOutcome,
    },
}

fn process_photo(
    vfs: &dyn Vfs,
    photo: &PhotoFile,
    geo: &dyn ReverseGeocoder,
    cache: &mut GeoCache,
    force: bool,
    cancel: &AtomicBool,
    last_lookup: &mut Option<std::time::Instant>,
) -> ProcessResult {
    if !force {
        match sidecar_places_needed(vfs, photo.path()) {
            Ok(true) => {}
            Ok(false) => {
                return ProcessResult::Finished {
                    place_path: None,
                    outcome: PlaceOutcome::Skipped,
                };
            }
            Err(error) => {
                return ProcessResult::Finished {
                    place_path: None,
                    outcome: PlaceOutcome::Failed(error.to_string()),
                };
            }
        }
    }
    let (Some(lat), Some(lon)) = (photo.gps_latitude, photo.gps_longitude) else {
        return ProcessResult::Finished {
            place_path: None,
            outcome: PlaceOutcome::Skipped,
        };
    };
    if cache.nearest(lat, lon).is_none()
        && !wait_until_allowed(last_lookup, geo.min_interval(), cancel)
    {
        return ProcessResult::Cancelled;
    }
    let request = match resolve(cache, geo, lat, lon) {
        Ok(Some(req)) => req,
        Ok(None) => {
            return ProcessResult::Finished {
                place_path: None,
                outcome: PlaceOutcome::Skipped,
            };
        }
        Err(GeoError::Retryable(e)) | Err(GeoError::Fatal(e)) => {
            return ProcessResult::Finished {
                place_path: None,
                outcome: PlaceOutcome::Failed(e),
            };
        }
    };
    let place_path = Some(request.path.clone());
    match write_places(vfs, photo.path(), &request) {
        Ok(outcome) if outcome.written => ProcessResult::Finished {
            place_path,
            outcome: PlaceOutcome::Written,
        },
        Ok(_) => ProcessResult::Finished {
            place_path,
            outcome: PlaceOutcome::Skipped,
        },
        Err(e) => ProcessResult::Finished {
            place_path,
            outcome: PlaceOutcome::Failed(e.to_string()),
        },
    }
}

fn finish_queue(conn: &Connection, path: &str, outcome: &PlaceOutcome) -> Result<(), String> {
    let ok = match outcome {
        PlaceOutcome::Written => finish_done(conn, PLACES, path, PLACES_PACK, 1, None),
        PlaceOutcome::Skipped => finish_done(conn, PLACES, path, PLACES_PACK, 0, None),
        PlaceOutcome::Failed(_) => finish_failed(conn, PLACES, path, PLACES_FAILED),
    }
    .map_err(|e| e.to_string())?;
    if !ok {
        return Err(format!("places queue lost claim for {path}"));
    }
    Ok(())
}

fn persist_step<T>(
    summary: &mut PlacesSummary,
    op: impl FnOnce() -> rusqlite::Result<T>,
) -> Result<T, String> {
    match op() {
        Ok(value) => Ok(value),
        Err(error) => {
            let detail = error.to_string();
            if summary.error.is_none() {
                summary.error = Some(detail.clone());
            }
            Err(detail)
        }
    }
}

fn open_places_queue(path: Option<&Path>) -> Result<Connection, String> {
    let conn = match path {
        Some(path) => {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(parent);
                }
            }
            Connection::open(path).map_err(|e| e.to_string())?
        }
        None => Connection::open_in_memory().map_err(|e| e.to_string())?,
    };
    configure_connection(&conn).map_err(|e| e.to_string())?;
    ensure_table(&conn, PLACES).map_err(|e| e.to_string())?;
    Ok(conn)
}

fn finish_photo(
    summary: &mut PlacesSummary,
    image_path: &str,
    place_path: Option<String>,
    outcome: PlaceOutcome,
    index: usize,
    total: usize,
    on_progress: Option<&dyn Fn(usize, usize)>,
) {
    summary.processed += 1;
    match &outcome {
        PlaceOutcome::Written => {
            summary.written += 1;
            summary.written_paths.push(image_path.to_string());
        }
        PlaceOutcome::Skipped => summary.skipped += 1,
        PlaceOutcome::Failed(_) => summary.failed += 1,
    }
    if let PlaceOutcome::Failed(detail) = &outcome {
        if summary.error.is_none() {
            summary.error = Some(detail.clone());
        }
    }
    summary.records.push(PlaceRecord {
        image_path: image_path.to_string(),
        place_path,
        outcome,
    });
    if let Some(cb) = on_progress {
        cb(index + 1, total);
    }
}

fn row_tags(photo: &PhotoFile) -> impl Iterator<Item = &str> {
    photo.hierarchical_tags.iter().map(|t| t.full_path.as_str())
}

fn sidecar_places_needed(vfs: &dyn Vfs, image_path: &str) -> VfsResult<bool> {
    let tags = sidecar_tag_paths(vfs, image_path)?;
    Ok(places_needed(tags, false))
}

fn sidecar_tag_paths(vfs: &dyn Vfs, image_path: &str) -> VfsResult<Vec<String>> {
    let target = sidecar_path(image_path);
    let bytes = if vfs.try_exists(&target)? {
        Some(vfs.read(&target)?)
    } else {
        match alt_sidecar_path(image_path) {
            Some(alt) if vfs.try_exists(&alt)? => Some(vfs.read(&alt)?),
            _ => None,
        }
    };
    let Some(bytes) = bytes else {
        return Ok(Vec::new());
    };
    let Ok(view) = read_view(&bytes) else {
        return Ok(Vec::new());
    };
    Ok(view.tags_list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::{Gazetteer, ReverseGeocoder};
    use gallery_meta::place_from_parts;
    use gallery_model::photo::HierarchicalTag;
    use gallery_vfs::{MemVfs, VfsError};
    use localcore_queue::{item, WorkState};
    use std::time::Duration;

    struct FakeGeo;

    impl ReverseGeocoder for FakeGeo {
        fn lookup(
            &self,
            _lat: f64,
            _lon: f64,
        ) -> Result<Option<gallery_meta::PlaceWriteRequest>, GeoError> {
            Ok(place_from_parts(
                Some("France"),
                Some("Île-de-France"),
                Some("Paris"),
                None,
                Some("fr"),
            ))
        }
        fn min_interval(&self) -> Duration {
            Duration::ZERO
        }
    }

    fn still_with_gps() -> PhotoFile {
        still_with_gps_at("/lib/a.jpg")
    }

    fn still_with_gps_at(path: &str) -> PhotoFile {
        let mut p = PhotoFile::new(path, "a", 12);
        p.gps_latitude = Some(48.8566);
        p.gps_longitude = Some(2.3522);
        p
    }

    fn run(
        vfs: &dyn Vfs,
        photos: &[PhotoFile],
        geo: &dyn ReverseGeocoder,
        cache: &mut GeoCache,
        queue_db: Option<&Path>,
        force: bool,
        cancel: bool,
    ) -> PlacesSummary {
        run_places(
            vfs,
            photos,
            geo,
            cache,
            queue_db,
            force,
            &AtomicBool::new(cancel),
            None,
        )
    }

    #[test]
    fn run_writes_sidecar_through_vfs() {
        let vfs = MemVfs::new();
        vfs.write_atomic("/lib/a.jpg", b"not-a-jpeg").unwrap();
        let mut cache = GeoCache::new();
        let summary = run(
            &vfs,
            &[still_with_gps()],
            &FakeGeo,
            &mut cache,
            None,
            false,
            false,
        );
        assert_eq!(summary.written, 1);
        assert!(vfs.exists("/lib/a.jpg.xmp"));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn finished_sidecar_is_skipped_even_if_row_is_empty() {
        let vfs = MemVfs::new();
        vfs.write_atomic("/lib/a.jpg", b"x").unwrap();
        let req = place_from_parts(
            Some("France"),
            Some("Île-de-France"),
            Some("Paris"),
            None,
            Some("fr"),
        )
        .unwrap();
        write_places(&vfs, "/lib/a.jpg", &req).unwrap();
        let mut cache = GeoCache::new();
        let summary = run(
            &vfs,
            &[still_with_gps()],
            &FakeGeo,
            &mut cache,
            None,
            false,
            false,
        );
        assert_eq!(summary.written, 0);
        assert!(summary.skipped >= 1);
    }

    #[test]
    fn shallow_row_tag_stays_eligible() {
        let mut p = still_with_gps();
        p.hierarchical_tags = vec![HierarchicalTag::new("Places/France")];
        let vfs = MemVfs::new();
        vfs.write_atomic("/lib/a.jpg", b"x").unwrap();
        let mut cache = GeoCache::new();
        let summary = run(&vfs, &[p], &FakeGeo, &mut cache, None, false, false);
        assert_eq!(summary.written, 1);
    }

    #[test]
    fn bundled_gazetteer_writes_a_sidecar() {
        let vfs = MemVfs::new();
        vfs.write_atomic("/lib/a.jpg", b"not-a-jpeg").unwrap();
        let mut cache = GeoCache::new();
        let summary = run(
            &vfs,
            &[still_with_gps()],
            &Gazetteer,
            &mut cache,
            None,
            false,
            false,
        );
        assert_eq!(summary.written, 1, "{summary:?}");
        let view = read_view(&vfs.read("/lib/a.jpg.xmp").unwrap()).unwrap();
        assert!(
            view.tags_list
                .iter()
                .any(|t| t.starts_with("Places/France") && t.contains("Paris")),
            "{:?}",
            view.tags_list
        );
        assert_eq!(view.photo_tools.country_code.as_deref(), Some("FR"));
    }

    #[test]
    fn permission_denied_sidecar_is_a_failure_not_absence() {
        struct DeniedVfs(MemVfs);

        impl Vfs for DeniedVfs {
            fn open(
                &self,
                path: &str,
            ) -> gallery_vfs::VfsResult<Box<dyn gallery_vfs::ReadSeek + Send>> {
                self.0.open(path)
            }

            fn stat(&self, path: &str) -> gallery_vfs::VfsResult<gallery_vfs::Stat> {
                if path == "/lib/a.jpg.xmp" {
                    return Err(VfsError::PermissionDenied {
                        path: path.to_string(),
                    });
                }
                self.0.stat(path)
            }

            fn list(&self, dir: &str) -> gallery_vfs::VfsResult<Vec<gallery_vfs::Entry>> {
                self.0.list(dir)
            }

            fn stat_entry(&self, path: &str) -> gallery_vfs::VfsResult<gallery_vfs::Entry> {
                self.0.stat_entry(path)
            }

            fn write_atomic(&self, path: &str, bytes: &[u8]) -> gallery_vfs::VfsResult<()> {
                self.0.write_atomic(path, bytes)
            }

            fn exists(&self, path: &str) -> bool {
                self.0.exists(path)
            }
        }

        struct MustNotLookup;

        impl ReverseGeocoder for MustNotLookup {
            fn lookup(
                &self,
                _lat: f64,
                _lon: f64,
            ) -> Result<Option<gallery_meta::PlaceWriteRequest>, GeoError> {
                panic!("permission denial was treated as an absent sidecar")
            }

            fn min_interval(&self) -> Duration {
                Duration::ZERO
            }
        }

        let inner = MemVfs::new();
        inner.write_atomic("/lib/a.jpg", b"x").unwrap();
        let mut cache = GeoCache::new();
        let summary = run(
            &DeniedVfs(inner),
            &[still_with_gps()],
            &MustNotLookup,
            &mut cache,
            None,
            false,
            false,
        );

        assert_eq!(summary.failed, 1, "{summary:?}");
        assert_eq!(summary.written, 0);
        assert_eq!(summary.skipped, 0);
        assert!(summary
            .error
            .as_deref()
            .is_some_and(|error| error.contains("permission denied")));
    }

    #[test]
    fn nfc_and_nfd_paths_share_one_queue_row() {
        let nfc = "/lib/caf\u{00E9}.jpg";
        let nfd = "/lib/cafe\u{0301}.jpg";
        let vfs = MemVfs::new();
        vfs.write_atomic(nfc, b"x").unwrap();
        vfs.write_atomic(nfd, b"x").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("gallery-cache.sqlite");
        let mut cache = GeoCache::new();
        let summary = run(
            &vfs,
            &[still_with_gps_at(nfd), still_with_gps_at(nfc)],
            &FakeGeo,
            &mut cache,
            Some(&db),
            false,
            false,
        );
        assert_eq!(summary.considered, 1, "{summary:?}");
        assert_eq!(summary.processed, 1, "{summary:?}");
        assert_eq!(summary.written, 1, "{summary:?}");

        let conn = Connection::open(&db).unwrap();
        let row = item(&conn, PLACES, nfd).unwrap().unwrap();
        assert_eq!(row.path, nfc);
        assert_eq!(row.state, WorkState::Done);
        assert!(item(&conn, PLACES, nfc).unwrap().is_some());
    }

    #[test]
    fn done_row_is_not_reprocessed_on_the_next_run() {
        struct MustNotLookup;

        impl ReverseGeocoder for MustNotLookup {
            fn lookup(
                &self,
                _lat: f64,
                _lon: f64,
            ) -> Result<Option<gallery_meta::PlaceWriteRequest>, GeoError> {
                panic!("a done places_work row was claimed again")
            }

            fn min_interval(&self) -> Duration {
                Duration::ZERO
            }
        }

        let vfs = MemVfs::new();
        vfs.write_atomic("/lib/a.jpg", b"x").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("gallery-cache.sqlite");
        let mut cache = GeoCache::new();
        let first = run(
            &vfs,
            &[still_with_gps()],
            &FakeGeo,
            &mut cache,
            Some(&db),
            false,
            false,
        );
        assert_eq!(first.written, 1, "{first:?}");

        let second = run(
            &vfs,
            &[still_with_gps()],
            &MustNotLookup,
            &mut cache,
            Some(&db),
            false,
            false,
        );
        assert_eq!(second.processed, 0, "{second:?}");
        assert_eq!(second.written, 0, "{second:?}");
        assert!(!second.cancelled);
    }

    #[test]
    fn abandoned_hashing_row_is_reclaimed_on_the_next_run() {
        let vfs = MemVfs::new();
        vfs.write_atomic("/lib/a.jpg", b"x").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("gallery-cache.sqlite");
        {
            let mut conn = Connection::open(&db).unwrap();
            configure_connection(&conn).unwrap();
            ensure_table(&conn, PLACES).unwrap();
            enqueue(&mut conn, PLACES, &["/lib/a.jpg".into()]).unwrap();
            assert!(begin(&conn, PLACES, "/lib/a.jpg").unwrap());
        }

        let mut cache = GeoCache::new();
        let summary = run(
            &vfs,
            &[still_with_gps()],
            &FakeGeo,
            &mut cache,
            Some(&db),
            false,
            false,
        );
        assert_eq!(summary.written, 1, "{summary:?}");
        let conn = Connection::open(&db).unwrap();
        assert_eq!(
            item(&conn, PLACES, "/lib/a.jpg").unwrap().unwrap().state,
            WorkState::Done
        );
    }

    #[test]
    fn queue_path_is_gallery_cache_beside_the_geo_json() {
        assert_eq!(
            places_queue_db_path("/var/cache/geo-cache.json"),
            PathBuf::from("/var/cache/gallery-cache.sqlite")
        );
    }
}
