//! Places pass: eligibility, cache, offline gazetteer, sidecar write.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::geo::{resolve, wait_until_allowed, GeoCache, GeoError, ReverseGeocoder};
use gallery_meta::places::write_places;
use gallery_meta::read_view;
use gallery_meta::sidecar::{alt_sidecar_path, sidecar_path};
use gallery_model::photo::PhotoFile;
use gallery_vfs::{Vfs, VfsResult};

use crate::eligibility::{is_places_candidate, places_needed};

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
/// Queue uses in-memory tags. The write skip re-reads the sidecar so a
/// finished city on disk is not overwritten when the row is stale.
pub fn run_places(
    vfs: &dyn Vfs,
    photos: &[PhotoFile],
    geo: &dyn ReverseGeocoder,
    cache: &mut GeoCache,
    force: bool,
    cancel: &AtomicBool,
    on_progress: Option<&dyn Fn(usize, usize)>,
) -> PlacesSummary {
    let queue: Vec<&PhotoFile> = photos
        .iter()
        .filter(|p| is_places_candidate(p) && (force || places_needed(row_tags(p), false)))
        .collect();
    let total = queue.len();
    let mut summary = PlacesSummary {
        considered: total,
        ..PlacesSummary::default()
    };
    if let Some(cb) = on_progress {
        cb(0, total);
    }
    let mut last_lookup = None;
    for (i, photo) in queue.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            summary.cancelled = true;
            break;
        }
        if !force {
            match sidecar_places_needed(vfs, photo.path()) {
                Ok(true) => {}
                Ok(false) => {
                    finish_photo(
                        &mut summary,
                        photo.path(),
                        None,
                        PlaceOutcome::Skipped,
                        i,
                        total,
                        on_progress,
                    );
                    continue;
                }
                Err(error) => {
                    let detail = error.to_string();
                    if summary.error.is_none() {
                        summary.error = Some(detail.clone());
                    }
                    finish_photo(
                        &mut summary,
                        photo.path(),
                        None,
                        PlaceOutcome::Failed(detail),
                        i,
                        total,
                        on_progress,
                    );
                    continue;
                }
            }
        }
        let (Some(lat), Some(lon)) = (photo.gps_latitude, photo.gps_longitude) else {
            finish_photo(
                &mut summary,
                photo.path(),
                None,
                PlaceOutcome::Skipped,
                i,
                total,
                on_progress,
            );
            continue;
        };
        if cache.nearest(lat, lon).is_none()
            && !wait_until_allowed(&mut last_lookup, geo.min_interval(), cancel)
        {
            summary.cancelled = true;
            break;
        }
        let request = match resolve(cache, geo, lat, lon) {
            Ok(Some(req)) => req,
            Ok(None) => {
                finish_photo(
                    &mut summary,
                    photo.path(),
                    None,
                    PlaceOutcome::Skipped,
                    i,
                    total,
                    on_progress,
                );
                continue;
            }
            Err(GeoError::Retryable(e)) | Err(GeoError::Fatal(e)) => {
                if summary.error.is_none() {
                    summary.error = Some(e.clone());
                }
                finish_photo(
                    &mut summary,
                    photo.path(),
                    None,
                    PlaceOutcome::Failed(e),
                    i,
                    total,
                    on_progress,
                );
                continue;
            }
        };
        let place_path = Some(request.path.clone());
        match write_places(vfs, photo.path(), &request) {
            Ok(outcome) if outcome.written => finish_photo(
                &mut summary,
                photo.path(),
                place_path,
                PlaceOutcome::Written,
                i,
                total,
                on_progress,
            ),
            Ok(_) => finish_photo(
                &mut summary,
                photo.path(),
                place_path,
                PlaceOutcome::Skipped,
                i,
                total,
                on_progress,
            ),
            Err(e) => {
                let detail = e.to_string();
                if summary.error.is_none() {
                    summary.error = Some(detail.clone());
                }
                finish_photo(
                    &mut summary,
                    photo.path(),
                    place_path,
                    PlaceOutcome::Failed(detail),
                    i,
                    total,
                    on_progress,
                );
            }
        }
    }
    summary
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
        let mut p = PhotoFile::new("/lib/a.jpg", "a", 12);
        p.gps_latitude = Some(48.8566);
        p.gps_longitude = Some(2.3522);
        p
    }

    #[test]
    fn run_writes_sidecar_through_vfs() {
        let vfs = MemVfs::new();
        vfs.write_atomic("/lib/a.jpg", b"not-a-jpeg").unwrap();
        let mut cache = GeoCache::new();
        let summary = run_places(
            &vfs,
            &[still_with_gps()],
            &FakeGeo,
            &mut cache,
            false,
            &AtomicBool::new(false),
            None,
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
        let summary = run_places(
            &vfs,
            &[still_with_gps()],
            &FakeGeo,
            &mut cache,
            false,
            &AtomicBool::new(false),
            None,
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
        let summary = run_places(
            &vfs,
            &[p],
            &FakeGeo,
            &mut cache,
            false,
            &AtomicBool::new(false),
            None,
        );
        assert_eq!(summary.written, 1);
    }

    #[test]
    fn bundled_gazetteer_writes_a_sidecar() {
        let vfs = MemVfs::new();
        vfs.write_atomic("/lib/a.jpg", b"not-a-jpeg").unwrap();
        let mut cache = GeoCache::new();
        let summary = run_places(
            &vfs,
            &[still_with_gps()],
            &Gazetteer,
            &mut cache,
            false,
            &AtomicBool::new(false),
            None,
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
        let summary = run_places(
            &DeniedVfs(inner),
            &[still_with_gps()],
            &MustNotLookup,
            &mut cache,
            false,
            &AtomicBool::new(false),
            None,
        );

        assert_eq!(summary.failed, 1, "{summary:?}");
        assert_eq!(summary.written, 0);
        assert_eq!(summary.skipped, 0);
        assert!(summary
            .error
            .as_deref()
            .is_some_and(|error| error.contains("permission denied")));
    }
}
