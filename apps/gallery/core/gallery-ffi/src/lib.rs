//! The only crate the apps see.
//!
//! UniFFI proc-macro mode (no UDL). The namespace passed to
//! `setup_scaffolding!` names the generated Swift module, so the bindings land
//! as `GalleryCore.swift` / `GalleryCoreFFI.h` — see `scripts/build_core.sh`.
//!
//! FFI rules: coarse-grained calls only, typed error enums, long work
//! on core-owned threads.
//!
//! Surface:
//!
//! * [`core_version`], [`stable_uuid`].
//! * [`tagging`]: [`TaggingSession`] and its progress listener.
//! * [`faces`]: [`FaceSession`], cluster review, name / rename / merge / split.
//! * [`scanner`]: [`ScannerSession`], snapshot IO, metadata reads.
//!   Request/response — see that module for why it is still an object.
//! * [`library`]: [`LibraryIndex`] plus the memory engine as free
//!   functions over an inputs snapshot; [`MemoryGenerator`] holds the
//!   cancel flag.
//! * [`places`]: offline place lookup and Places sidecar writes.
//! * [`conflict`]: [`ConflictSession`] for Syncthing `.xmp` groups.
//! * [`person_log`]: person-state append / read / project / UserDefaults migrate.
//!
//! Tagging and face sessions share one cache file: [`support`] holds
//! the run-thread mechanics both are built on.

uniffi::setup_scaffolding!("GalleryCore");

pub mod conflict;
pub mod faces;
pub mod heic;
pub mod library;
mod locations;
pub mod people;
pub mod person_log;
pub mod places;
pub mod scanner;
mod support;
pub mod tagging;
pub mod view;

pub use conflict::{
    is_conflict_name, ConflictError, ConflictFieldPreview, ConflictPreview, ConflictRow,
    ConflictSession, ConflictSide, GalleryFieldRow, MergeKind,
};
pub use faces::{
    face_merge_direction, ClusterState, FaceAssignKind, FaceAssignmentCommandResult,
    FaceClusterHostRow, FaceCropHostItem, FaceError, FaceFailure, FaceLibraryCommandResult,
    FaceMergeCommand, FaceMergeCommandSide, FaceMergeStructure, FacePhotoCommandResult,
    FaceProgressListener, FaceQueueCommandResult, FaceReclusterCommandResult, FaceRunCommandResult,
    FaceSession, FaceSplitCommandResult, SidecarWriteCommandResult,
};
pub use gallery_index::{SearchHit, SearchKind};
pub use heic::{HeicDecodeError, HeicDecoder, HostDecodedImage};
pub use library::{
    compute_scheduled_memories, generate_memories, memory_cluster_key, memory_country_name,
    scheduled_memory_horizon_days, GenerateMemoriesCommand, LibraryBuildStructure, LibraryIndex,
    MemoryContactCommandItem, MemoryDateCommandItem, MemoryFolderCommandItem, MemoryGenerator,
    MemoryKind, MemoryPersonCommandItem, MemoryStructure, RemovePhotosResult,
    ScheduledMemoryContext, ScheduledMemoryStructure, TagStructureItem, TagStructures,
};
pub use people::{person_link_state, visible_people, PersonLinkKind, PersonLinkResolution};
pub use person_log::{
    person_log_append, person_log_migrate_from_snapshot, person_log_project,
    person_log_project_report, person_log_read, PersonLogError, PersonProjectionRecord,
    PersonStatePair, PersonStateStructure, PersonTornTailRecord, GALLERY_STATE_DIR,
};
pub use places::{
    library_watch_refresh_interval_ms, places_candidate, PlacesProgressListener,
    PlacesRecordOutcome, PlacesRunRecord, PlacesRunSummary, PlacesSession,
};
pub use scanner::{
    load_snapshot, named_people_without_box, parse_xmp_bytes, photo_file_from_scan,
    probe_snapshot_version, read_image_metadata, read_sidecar, read_video_date, save_snapshot,
    snapshot_version, HostContentVersion, HostFaceRegion, HostImageMetadata, HostTagValue,
    HostWallClock, ParsedSidecarHost, ScanCatalogHost, ScanCommand, ScanError, ScanMetrics,
    ScanProgressListener, ScannedFolderHost, ScannedMediaHost, ScannedSidecarHost, ScannerSession,
    SidecarHostView, SnapshotHostDocument,
};
pub use tagging::{
    inspect_model_pack, resolve_model_pack, ModelPackHostInfo, ModelPackHostResolution, PackSource,
    TaggingError, TaggingFailure, TaggingProgressListener, TaggingQueueCommandResult,
    TaggingRunCommandResult, TaggingSession,
};
pub use view::{
    GalleryMediaItem, GalleryTextRow, ViewAction, ViewContentState, ViewError, ViewSection,
    ViewSlotKind, ViewStructure,
};

/// Version of the Rust core, for logging and "is the framework I linked the
/// one I just built?" sanity checks.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Rust-side `StableUUID.derive`, rendered lowercase hyphenated.
///
/// The caller passes an already-standardized path string (Swift:
/// `url.standardized.path`). Unicode NFC is applied inside
/// `localcore_id::derive` (ADR 0002 R4 / M1).
#[uniffi::export]
pub fn stable_uuid(input: String) -> String {
    gallery_model::stable_uuid::derive(&input).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_version_matches_the_crate() {
        assert_eq!(core_version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn stable_uuid_is_lowercase_hyphenated() {
        let rendered = stable_uuid("/library/x.jpg".to_string());
        assert_eq!(rendered.len(), 36);
        assert_eq!(rendered, rendered.to_lowercase());
    }
}
