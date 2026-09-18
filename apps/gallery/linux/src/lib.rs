//! LocalGallery Linux host: scan, enrich, index. The GTK shell lives in `main`.

pub mod config;
pub mod decode;
pub mod display;
pub mod faces;
pub mod heic;
pub mod host;
pub mod mutate;
pub mod ops;
pub mod persist;
pub mod row;
pub mod thumbs;
pub mod time;
pub mod video;
pub mod watch;
pub mod xdg_thumb;

#[cfg(feature = "ui")]
pub mod ui;

pub use config::{Config, ConfigLoad};
pub use gallery_model::photo::PhotoFile;
pub use gallery_session::{
    checkout_pack_dir, discover_pack, download_pack, installed_pack, ml_enabled, progress_title,
    remove_installed_pack, run_analysis, xdg_pack_present, AnalysisPhase, AnalysisPhases,
    AnalysisProgress, AnalysisRequest, AnalysisSummary, Gazetteer, PackInstallError, PackStatus,
    ProgressFn,
};
pub use heic::linux_heic_decoder;
pub use host::{
    collection_groups, commit_analysis_state, event_folders, find_folder, leaf_tags,
    library_availability, open_library, open_library_with_commit, overlay_sidecars, patch_tree,
    reapply_sidecars, reapply_sidecars_with_commit, scan_input_for_open, CollectionGroup,
    HostError, LibraryAvailability, LibraryState, SnapshotReuse,
};
pub use row::{PhotoRow, UnsupportedPath};
