//! Headless health archive core.
//!
//! Owns the disposable SQLite projection, portable-v1 export/restore/fsck,
//! kind catalog, and gap reports. I/O for the log and blob store goes through
//! [`localcore_log`] and [`localcore_blob`]. Apple `export.xml` is not read
//! (ADR 0008); sample rows come from log events or streamed NDJSON blobs.

#![forbid(unsafe_code)]

pub mod catalog;
pub mod config;
pub mod display;
pub mod error;
pub mod gaps;
pub mod portable;
pub mod projection;
mod samples;

pub use catalog::{kind_catalog, resolve_kind, short_kind, KindInfo};
pub use config::Config;
pub use display::{
    chart_row, counts_dto, event_rows, export_result, fsck_result, gap_structure, kind_rows,
    rebuild_result, restore_result, ArchiveCommandResult, ArchiveCounts, ChartRow, GapStructure,
    KindRow,
};
pub use error::{Error, Result};
pub use gaps::{episode_gaps, observation_gaps, overall_gaps, GapReport};
pub use localcore_log::{
    append, read_all, read_report, Event, TYPE_BLOB_IMPORT, TYPE_EPISODE, TYPE_NOTE,
    TYPE_OBSERVATION, TYPE_RETRACT, TYPE_SUPERSEDE,
};
pub use portable::{
    export, fsck, restore, restore_allowing_voids, restore_with, FsckReport, Manifest,
    RestoreOptions, ARCHIVE_VERSION, TOOL_VERSION,
};
pub use projection::{
    db_path, is_voided, open, query, query_episodes, query_observations, rebuild, semantic_dump,
    table_counts, voided_ids, Counts, EpisodeRow, EventRow, Filter, ObservationRow, RebuildReport,
    APPLICATION_ID,
};
