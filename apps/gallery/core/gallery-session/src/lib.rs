//! Product policy both apps share: Scan Photos order, pack roots, Places
//! eligibility, watch mute, sidecar refresh.
//!
//! Engines and XMP stay in the crates below this one. The GTK / SwiftUI
//! shells hop progress and draw chrome.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod analysis;
pub mod eligibility;
pub mod geo;
pub mod pack;
pub mod people;
pub mod places;
pub mod refresh;
pub mod watch;

pub use analysis::{
    progress_title, readiness_blurb, run_analysis, AnalysisPhase, AnalysisPhases, AnalysisProgress,
    AnalysisRequest, AnalysisSummary, ProgressFn,
};
pub use eligibility::{is_ml_eligible, is_places_candidate, places_needed};
pub use geo::{
    haversine_km, resolve, wait_until_allowed, Gazetteer, GeoCache, GeoCacheEntry, GeoCacheError,
    GeoError, ReverseGeocoder, CACHE_RADIUS_KM, DISK_CACHE_VERSION,
};
pub use pack::{
    checkout_pack_dir, data_pack_root, default_roots, discover_pack, download_pack, fetch_pack,
    install_pack_from, installed_pack, ml_enabled, remove_installed_pack, remove_pack_at,
    resolve_in, xdg_pack_present, PackFetch, PackInstallError, PackRoots, PackStatus,
};
pub use places::{places_queue_db_path, run_places, PlaceOutcome, PlaceRecord, PlacesSummary};
pub use refresh::{refresh_plan, SidecarRefreshPlan};
pub use watch::{should_note, REFRESH_INTERVAL};
