//! One Scan Photos run: tagging, then faces, then places.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::geo::{GeoCache, ReverseGeocoder};
use gallery_model::photo::PhotoFile;
use gallery_vfs::StdVfs;

use crate::eligibility::is_ml_eligible;
use crate::pack::{self, PackStatus};
use crate::places::{self, PlacesSummary};
use crate::refresh::{refresh_plan, SidecarRefreshPlan};

/// Which phase the banner is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisPhase {
    /// MobileCLIP tags.
    Tagging,
    /// Detect / embed / cluster.
    Faces,
    /// Offline gazetteer + `Places/*`.
    Places,
}

impl AnalysisPhase {
    /// Banner verb.
    pub fn label(self) -> &'static str {
        match self {
            AnalysisPhase::Tagging => "Tagging…",
            AnalysisPhase::Faces => "Finding faces…",
            AnalysisPhase::Places => "Looking up places…",
        }
    }

    /// One word for the shared progress chip.
    pub fn short_label(self) -> &'static str {
        match self {
            AnalysisPhase::Tagging => "Tagging",
            AnalysisPhase::Faces => "Faces",
            AnalysisPhase::Places => "Places",
        }
    }
}

/// Which Scan Photos workers to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisPhases {
    /// MobileCLIP objects / scenes / landmarks.
    pub tagging: bool,
    /// Detect / embed / cluster people.
    pub faces: bool,
    /// Offline gazetteer + `Places/*`.
    pub places: bool,
}

impl AnalysisPhases {
    /// Tagging, faces, and places.
    pub fn all() -> Self {
        Self {
            tagging: true,
            faces: true,
            places: true,
        }
    }

    /// No worker selected.
    pub fn none() -> Self {
        Self {
            tagging: false,
            faces: false,
            places: false,
        }
    }

    /// True when every worker is off.
    pub fn is_empty(self) -> bool {
        !self.tagging && !self.faces && !self.places
    }

    /// True when tagging or faces was asked for.
    pub fn wants_ml(self) -> bool {
        self.tagging || self.faces
    }
}

impl Default for AnalysisPhases {
    fn default() -> Self {
        Self::all()
    }
}

/// Live counts for the current phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisProgress {
    /// Tagging / faces / places.
    pub phase: AnalysisPhase,
    /// Items finished in this phase.
    pub done: usize,
    /// Items queued for this phase.
    pub total: usize,
}

/// What a finished (or cancelled) run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnalysisSummary {
    /// Eligible stills at the start.
    pub photos: usize,
    /// Tagging sidecar writes. `None` when the phase did not run.
    pub tagged: Option<usize>,
    /// Face sidecar writes.
    pub faces: Option<usize>,
    /// Places result, when that phase ran.
    pub places: Option<PlacesSummary>,
    /// Paths any phase wrote.
    pub written_paths: Vec<String>,
    /// How the host should pick those writes up.
    pub refresh: SidecarRefreshPlan,
    /// Stopped because the user cancelled.
    pub cancelled: bool,
    /// First error the host should toast.
    pub error: Option<String>,
    /// Why tagging/faces were skipped, if they were.
    pub skipped_ml: Option<String>,
}

impl AnalysisSummary {
    /// One line for Preferences / a toast.
    pub fn toast_line(&self) -> String {
        if self.cancelled {
            return "Scan cancelled".into();
        }
        if let Some(err) = &self.error {
            return err.clone();
        }
        let places_n = self.places.as_ref().map(|p| p.written).unwrap_or(0);
        let tagged = self.tagged.unwrap_or(0);
        let faces = self.faces.unwrap_or(0);
        if tagged + faces + places_n == 0 {
            if let Some(why) = &self.skipped_ml {
                return why.clone();
            }
            return "Nothing new to write".into();
        }
        format!("Wrote {tagged} tags, {faces} faces, {places_n} places")
    }
}

/// Progress hop used by worker threads.
pub type ProgressFn = Arc<dyn Fn(AnalysisProgress) + Send + Sync>;

/// Inputs for one Scan Photos run.
pub struct AnalysisRequest<'a> {
    /// Library entries at the start of the run.
    pub photos: &'a [PhotoFile],
    /// Discovered pack, if any. Ignored when `ml` is off.
    pub pack: Option<&'a PackStatus>,
    /// Face/tag cache DB path. Ignored when `ml` is off.
    pub ml_cache: Option<&'a std::path::Path>,
    /// Reverse geocoder used for the Places phase.
    pub geo: &'a dyn ReverseGeocoder,
    /// On-disk place-lookup cache.
    pub geo_cache: &'a mut GeoCache,
    /// Which workers to run. Scan Photos uses [`AnalysisPhases::all`].
    pub phases: AnalysisPhases,
    /// Re-write selected phases even when skip keys are current.
    pub force: bool,
    /// Host-owned cancel flag.
    pub cancel: &'a AtomicBool,
    /// Optional progress hop for the banner.
    pub on_progress: Option<ProgressFn>,
    /// Host HEIC door. Ignored when `ml` is off.
    pub heic_decoder: Option<Arc<dyn gallery_ml::HostHeicDecoder>>,
}

/// Run the three phases. `pack` / `ml_cache` are ignored when `ml` is off.
pub fn run_analysis(request: AnalysisRequest<'_>) -> AnalysisSummary {
    run_analysis_inner(request, false)
}

/// Force the selected phases on `request.photos` only.
///
/// Does not reset library-wide tagging/face queues. Places still honors
/// `request.force` (Scan Photos one-photo uses `true`).
pub fn run_analysis_one(request: AnalysisRequest<'_>) -> AnalysisSummary {
    run_analysis_inner(request, true)
}

fn run_analysis_inner(request: AnalysisRequest<'_>, one: bool) -> AnalysisSummary {
    let AnalysisRequest {
        photos,
        pack,
        ml_cache,
        geo,
        geo_cache,
        phases,
        force,
        cancel,
        on_progress,
        heic_decoder,
    } = request;
    #[cfg(not(feature = "ml"))]
    let _ = one;
    let _span =
        localcore_trace::span_always("catalog", "run_analysis").extra("photos", photos.len());
    let mut summary = AnalysisSummary {
        photos: photos.len(),
        ..AnalysisSummary::default()
    };
    if phases.is_empty() {
        let _ = force;
        summary.refresh = refresh_plan(std::iter::empty::<String>());
        return summary;
    }
    let stills: Vec<&PhotoFile> = photos.iter().filter(|p| is_ml_eligible(p)).collect();

    if phases.wants_ml() {
        #[cfg(feature = "ml")]
        {
            match (pack, ml_cache, crate::pack::ml_enabled()) {
                (Some(pack), Some(cache), true) => {
                    if let Err(e) = run_ml(
                        &stills,
                        pack,
                        cache,
                        cancel,
                        on_progress.clone(),
                        heic_decoder,
                        phases,
                        force,
                        one,
                        &mut summary,
                    ) {
                        if summary.error.is_none() {
                            summary.error = Some(e);
                        }
                    }
                }
                (None, _, true) => {
                    summary.skipped_ml = Some(
                        "No model pack. Put one in ~/.local/share/localgallery/pack or set LOCALGALLERY_PACK."
                            .into(),
                    );
                }
                _ => {
                    summary.skipped_ml = Some(
                        "This build has no ONNX. Rebuild with --features ml to tag and find faces."
                            .into(),
                    );
                }
            }
        }
        #[cfg(not(feature = "ml"))]
        {
            let _ = pack;
            let _ = &stills;
            let _ = heic_decoder;
            let _ = force;
            summary.skipped_ml = Some(
                "This build has no ONNX. Rebuild with --features ml to tag and find faces.".into(),
            );
        }
    } else {
        let _ = pack;
        let _ = &stills;
        let _ = heic_decoder;
    }

    if cancel.load(Ordering::Relaxed) {
        summary.cancelled = true;
        summary.refresh = refresh_plan(summary.written_paths.iter().cloned());
        return summary;
    }

    if !phases.places {
        summary.refresh = refresh_plan(summary.written_paths.iter().cloned());
        return summary;
    }

    let vfs = StdVfs::new();
    let places = places::run_places(
        &vfs,
        photos,
        geo,
        geo_cache,
        ml_cache,
        force,
        cancel,
        Some(&|done, total| {
            if let Some(cb) = on_progress.as_ref() {
                cb(AnalysisProgress {
                    phase: AnalysisPhase::Places,
                    done,
                    total,
                });
            }
        }),
    );
    summary
        .written_paths
        .extend(places.written_paths.iter().cloned());
    if places.cancelled {
        summary.cancelled = true;
    }
    if summary.error.is_none() {
        summary.error = places.error.clone();
    }
    summary.places = Some(places);
    summary.refresh = refresh_plan(summary.written_paths.iter().cloned());
    summary
}

#[cfg(feature = "ml")]
fn run_ml(
    stills: &[&PhotoFile],
    pack: &PackStatus,
    cache_db: &std::path::Path,
    cancel: &AtomicBool,
    on_progress: Option<ProgressFn>,
    heic: Option<Arc<dyn gallery_ml::HostHeicDecoder>>,
    phases: AnalysisPhases,
    force: bool,
    one: bool,
    summary: &mut AnalysisSummary,
) -> Result<(), String> {
    use gallery_ml::{FaceEngine, FaceRunOptions, RunOptions, TaggingEngine};
    use std::sync::Arc;

    let vfs = Arc::new(StdVfs::new());
    if let Some(dir) = cache_db.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let paths: Vec<String> = stills.iter().map(|p| p.path().to_string()).collect();

    if phases.tagging {
        let mut tagging = TaggingEngine::open(cache_db, &pack.directory, vfs.clone())
            .map_err(|e| e.to_string())?;
        if let Some(dec) = heic.clone() {
            tagging = tagging.with_heic_decoder(dec);
        }
        if one {
            for path in &paths {
                tagging.reopen(path).map_err(|e| e.to_string())?;
            }
        } else {
            if force {
                tagging.reset_queue().map_err(|e| e.to_string())?;
            }
            tagging.enqueue(&paths).map_err(|e| e.to_string())?;
        }
        let progress = PhaseProgress::new(AnalysisPhase::Tagging, on_progress.clone());
        let tag_summary = tagging
            .run_with_options(
                &progress,
                cancel,
                &RunOptions {
                    workers: Some(1),
                    force: force || one,
                    only_paths: one.then(|| paths.clone()),
                    ..RunOptions::default()
                },
            )
            .map_err(|e| e.to_string())?;
        summary.tagged = Some(tag_summary.sidecars_written);
        summary.written_paths.extend(progress.take_written());
        if tag_summary.cancelled {
            summary.cancelled = true;
            return Ok(());
        }
    }

    if !phases.faces {
        return Ok(());
    }
    if !pack.has_faces {
        return Ok(());
    }
    if cancel.load(Ordering::Relaxed) {
        summary.cancelled = true;
        return Ok(());
    }

    let mut faces = match FaceEngine::open(cache_db, &pack.directory, vfs) {
        Ok(engine) => engine,
        Err(gallery_ml::MlError::FaceModelsUnavailable) => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    if let Some(dec) = heic.clone() {
        faces = faces.with_heic_decoder(dec);
    }
    if one {
        for path in &paths {
            faces.reopen(path).map_err(|e| e.to_string())?;
        }
    } else {
        if force {
            faces.reset_queue().map_err(|e| e.to_string())?;
        }
        faces.enqueue(&paths).map_err(|e| e.to_string())?;
    }
    let progress = PhaseProgress::new(AnalysisPhase::Faces, on_progress);
    let face_summary = faces
        .run_with_options(
            &progress,
            cancel,
            &FaceRunOptions {
                workers: Some(1),
                force: force || one,
                only_paths: one.then(|| paths.clone()),
                ..FaceRunOptions::default()
            },
        )
        .map_err(|e| e.to_string())?;
    summary.faces = Some(face_summary.sidecars_written);
    summary.written_paths.extend(progress.take_written());
    if face_summary.cancelled {
        summary.cancelled = true;
    }
    Ok(())
}

#[cfg(feature = "ml")]
struct PhaseProgress {
    phase: AnalysisPhase,
    on_progress: Option<ProgressFn>,
    written: std::sync::Mutex<Vec<String>>,
}

#[cfg(feature = "ml")]
impl PhaseProgress {
    fn new(phase: AnalysisPhase, on_progress: Option<ProgressFn>) -> Self {
        PhaseProgress {
            phase,
            on_progress,
            written: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn take_written(&self) -> Vec<String> {
        self.written
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    }
}

#[cfg(feature = "ml")]
impl gallery_ml::TaggingProgress for PhaseProgress {
    fn on_progress(&self, done: usize, total: usize) {
        if let Some(cb) = &self.on_progress {
            cb(AnalysisProgress {
                phase: self.phase,
                done,
                total,
            });
        }
    }
    fn on_photos_tagged(&self, paths: &[String]) {
        if let Ok(mut w) = self.written.lock() {
            w.extend(paths.iter().cloned());
        }
    }
    fn on_finished(&self, _summary: &gallery_ml::RunSummary) {}
}

#[cfg(feature = "ml")]
impl gallery_ml::FaceProgress for PhaseProgress {
    fn on_progress(&self, done: usize, total: usize) {
        if let Some(cb) = &self.on_progress {
            cb(AnalysisProgress {
                phase: self.phase,
                done,
                total,
            });
        }
    }
    fn on_photos_with_faces(&self, _paths: &[String]) {}
    fn on_sidecars_written(&self, paths: &[String]) {
        if let Ok(mut w) = self.written.lock() {
            w.extend(paths.iter().cloned());
        }
    }
    fn on_finished(&self, _summary: &gallery_ml::FaceRunSummary) {}
}

/// Banner line.
pub fn progress_title(p: &AnalysisProgress) -> String {
    if p.total > 0 {
        format!("{} {}/{}", p.phase.label(), p.done, p.total)
    } else {
        p.phase.label().to_string()
    }
}

/// Why Scan Photos is limited, for the prefs description.
pub fn readiness_blurb(pack: Option<&PackStatus>, photo_count: usize) -> String {
    if photo_count == 0 {
        return "Open a library folder first.".into();
    }
    let mut parts = Vec::new();
    if pack::ml_enabled() {
        match pack {
            Some(p) => {
                let faces = if p.has_faces {
                    "tagging and faces"
                } else {
                    "tagging (this pack has no face models)"
                };
                parts.push(format!(
                    "Pack {} ({}) — {faces}.",
                    if p.version.is_empty() {
                        &p.name
                    } else {
                        &p.version
                    },
                    p.source_label()
                ));
            }
            None => parts.push(
                "No model pack — tagging and faces stay off. Places still runs for GPS photos."
                    .into(),
            ),
        }
    } else {
        parts.push(
            "This binary has no ONNX. Places still runs. Rebuild with --features ml to tag.".into(),
        );
        if let Some(p) = pack {
            parts.push(format!(
                "Found pack {} ready for that build.",
                if p.version.is_empty() {
                    &p.name
                } else {
                    &p.version
                }
            ));
        }
    }
    parts.push("Places uses a bundled gazetteer (GPS stays on the device).".into());
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::{Gazetteer, GeoCache};

    struct UnusedHeic;

    impl gallery_ml::HostHeicDecoder for UnusedHeic {
        fn decode(&self, _path: &str) -> gallery_ml::MlResult<gallery_ml::RgbImage> {
            panic!("ml is off; host decoder must not be asked")
        }
    }

    #[test]
    fn places_only_run_accepts_a_heic_decoder_when_ml_is_off() {
        let cancel = AtomicBool::new(false);
        let mut geo_cache = GeoCache::new();
        let summary = run_analysis(AnalysisRequest {
            photos: &[],
            pack: None,
            ml_cache: None,
            geo: &Gazetteer,
            geo_cache: &mut geo_cache,
            phases: AnalysisPhases::all(),
            force: false,
            cancel: &cancel,
            on_progress: None,
            heic_decoder: Some(Arc::new(UnusedHeic)),
        });
        assert!(!summary.cancelled);
        assert!(summary.places.is_some());
        assert!(summary.error.is_none());
        assert!(summary.skipped_ml.is_some());
    }

    #[test]
    fn places_only_skips_the_onnx_note() {
        let cancel = AtomicBool::new(false);
        let mut geo_cache = GeoCache::new();
        let summary = run_analysis(AnalysisRequest {
            photos: &[],
            pack: None,
            ml_cache: None,
            geo: &Gazetteer,
            geo_cache: &mut geo_cache,
            phases: AnalysisPhases {
                tagging: false,
                faces: false,
                places: true,
            },
            force: false,
            cancel: &cancel,
            on_progress: None,
            heic_decoder: Some(Arc::new(UnusedHeic)),
        });
        assert!(summary.places.is_some());
        assert!(summary.skipped_ml.is_none(), "{:?}", summary.skipped_ml);
        assert!(summary.tagged.is_none());
        assert!(summary.faces.is_none());
    }

    #[test]
    fn start_one_places_does_not_require_a_pack() {
        let cancel = AtomicBool::new(false);
        let mut geo_cache = GeoCache::new();
        let summary = run_analysis_one(AnalysisRequest {
            photos: &[],
            pack: None,
            ml_cache: None,
            geo: &Gazetteer,
            geo_cache: &mut geo_cache,
            phases: AnalysisPhases {
                tagging: false,
                faces: false,
                places: true,
            },
            force: true,
            cancel: &cancel,
            on_progress: None,
            heic_decoder: None,
        });
        assert!(summary.places.is_some());
        assert!(summary.skipped_ml.is_none(), "{:?}", summary.skipped_ml);
        assert!(summary.tagged.is_none());
        assert!(summary.faces.is_none());
    }

    #[test]
    fn empty_phases_write_nothing() {
        let cancel = AtomicBool::new(false);
        let mut geo_cache = GeoCache::new();
        let summary = run_analysis(AnalysisRequest {
            photos: &[],
            pack: None,
            ml_cache: None,
            geo: &Gazetteer,
            geo_cache: &mut geo_cache,
            phases: AnalysisPhases::none(),
            force: false,
            cancel: &cancel,
            on_progress: None,
            heic_decoder: None,
        });
        assert!(summary.places.is_none());
        assert!(summary.skipped_ml.is_none());
        assert_eq!(summary.toast_line(), "Nothing new to write");
    }

    #[test]
    fn readiness_mentions_gazetteer() {
        let blurb = readiness_blurb(None, 3);
        assert!(blurb.contains("gazetteer"), "{blurb}");
        assert!(!blurb.contains("Nominatim"), "{blurb}");
    }

    #[test]
    fn progress_title_includes_counts() {
        let title = progress_title(&AnalysisProgress {
            phase: AnalysisPhase::Places,
            done: 2,
            total: 9,
        });
        assert_eq!(title, "Looking up places… 2/9");
    }
}
