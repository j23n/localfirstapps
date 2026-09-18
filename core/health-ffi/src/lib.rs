//! Display-ready UniFFI surface for [`health_core`].
//!
//! The archive root stays on the host. Records that cross are list/status
//! rows, catalog rows, gap summaries, and command results.

uniffi::setup_scaffolding!("HealthCore");

use std::sync::{Mutex, MutexGuard};

use health_core::display::{
    chart_row, counts_dto, event_rows, export_result, fsck_result, gap_structure, kind_rows,
    rebuild_result, restore_result, ArchiveCommandResult as CoreArchiveCommandResult,
    ArchiveCounts as CoreArchiveCounts, ChartRow as CoreChartRow, EventRow as CoreEventRow,
    GapStructure as CoreGapStructure, KindRow as CoreKindRow,
};
use health_core::{
    episode_gaps, export, fsck, kind_catalog, observation_gaps, open, overall_gaps, query,
    query_observations, rebuild, restore, table_counts, Filter,
};

/// `chart-row` (ADR 0004 R4).
///
/// R6 role: command DTO
#[derive(uniffi::Record, Debug, Clone, PartialEq)]
pub struct ChartRow {
    pub title: String,
    pub subtitle: Option<String>,
    pub unit: Option<String>,
    pub latest: Option<String>,
    pub values: Vec<f64>,
}

impl From<CoreChartRow> for ChartRow {
    fn from(row: CoreChartRow) -> Self {
        Self {
            title: row.title,
            subtitle: row.subtitle,
            unit: row.unit,
            latest: row.latest,
            values: row.values,
        }
    }
}

/// `text-row` for one projected event.
///
/// R6 role: text-row
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct EventRow {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub trailing: Option<String>,
}

impl From<CoreEventRow> for EventRow {
    fn from(row: CoreEventRow) -> Self {
        Self {
            id: row.id,
            title: row.title,
            subtitle: row.subtitle,
            trailing: row.trailing,
        }
    }
}

/// Kind catalog row.
///
/// R6 role: text-row
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct KindRow {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub trailing: Option<String>,
}

impl From<CoreKindRow> for KindRow {
    fn from(row: CoreKindRow) -> Self {
        Self {
            id: row.id,
            title: row.title,
            subtitle: row.subtitle,
            trailing: row.trailing,
        }
    }
}

/// Gap report as status + missing-day ids.
///
/// R6 role: command DTO
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct GapStructure {
    pub title: String,
    pub detail: Option<String>,
    pub present_days: u32,
    pub missing_days: u32,
    pub missing: Vec<String>,
}

impl From<CoreGapStructure> for GapStructure {
    fn from(row: CoreGapStructure) -> Self {
        Self {
            title: row.title,
            detail: row.detail,
            present_days: row.present_days,
            missing_days: row.missing_days,
            missing: row.missing,
        }
    }
}

/// Rebuild / fsck command result.
///
/// R6 role: command DTO
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ArchiveCommandResult {
    pub ok: bool,
    pub title: String,
    pub detail: Option<String>,
}

impl From<CoreArchiveCommandResult> for ArchiveCommandResult {
    fn from(row: CoreArchiveCommandResult) -> Self {
        Self {
            ok: row.ok,
            title: row.title,
            detail: row.detail,
        }
    }
}

/// Table totals after rebuild.
///
/// R6 role: structure DTO
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ArchiveCounts {
    pub events: u32,
    pub blobs: u32,
    pub observations: u32,
    pub episodes: u32,
}

impl From<CoreArchiveCounts> for ArchiveCounts {
    fn from(row: CoreArchiveCounts) -> Self {
        Self {
            events: row.events,
            blobs: row.blobs,
            observations: row.observations,
            episodes: row.episodes,
        }
    }
}

/// Host-owned archive root.
#[derive(uniffi::Object)]
pub struct HealthArchive {
    root: String,
    db: Mutex<Option<rusqlite::Connection>>,
}

#[uniffi::export]
impl HealthArchive {
    #[uniffi::constructor]
    pub fn new(root: String) -> Self {
        Self {
            root,
            db: Mutex::new(None),
        }
    }

    pub fn rebuild(&self) -> Result<ArchiveCommandResult, HealthError> {
        self.clear_db()?;
        let report = rebuild(&self.root)?;
        self.cache_open()?;
        Ok(rebuild_result(&report).into())
    }

    pub fn counts(&self) -> Result<ArchiveCounts, HealthError> {
        self.with_db(|db| Ok(counts_dto(table_counts(db)?).into()))
    }

    pub fn events(&self, current_only: bool) -> Result<Vec<EventRow>, HealthError> {
        self.with_db(|db| {
            let rows = query(
                db,
                &Filter {
                    current: current_only,
                    ..Filter::default()
                },
            )?;
            Ok(event_rows(&rows).into_iter().map(EventRow::from).collect())
        })
    }

    pub fn kind_chart(&self, kind: String) -> Result<ChartRow, HealthError> {
        self.with_db(|db| {
            let rows = query_observations(db, Some(&kind), None)?;
            Ok(chart_row(&kind, &rows).into())
        })
    }

    pub fn catalog(&self) -> Result<Vec<KindRow>, HealthError> {
        self.with_db(|db| {
            Ok(kind_rows(&kind_catalog(db)?)
                .into_iter()
                .map(KindRow::from)
                .collect())
        })
    }

    pub fn observation_gaps(&self, kind: Option<String>) -> Result<GapStructure, HealthError> {
        self.with_db(|db| Ok(gap_structure(observation_gaps(db, kind.as_deref())?).into()))
    }

    pub fn episode_gaps(&self, kind: Option<String>) -> Result<GapStructure, HealthError> {
        self.with_db(|db| Ok(gap_structure(episode_gaps(db, kind.as_deref())?).into()))
    }

    pub fn overall_gaps(&self) -> Result<GapStructure, HealthError> {
        self.with_db(|db| Ok(gap_structure(overall_gaps(db)?).into()))
    }

    pub fn fsck(&self) -> Result<ArchiveCommandResult, HealthError> {
        Ok(fsck_result(&fsck(&self.root)?).into())
    }

    pub fn export_archive(&self, out_dir: String) -> Result<ArchiveCommandResult, HealthError> {
        Ok(export_result(&export(&self.root, &out_dir)?).into())
    }

    pub fn restore_archive(&self, from_dir: String) -> Result<ArchiveCommandResult, HealthError> {
        restore(&from_dir, &self.root)?;
        self.clear_db()?;
        rebuild(&self.root)?;
        self.cache_open()?;
        Ok(restore_result().into())
    }
}

impl HealthArchive {
    fn lock_db(&self) -> Result<MutexGuard<'_, Option<rusqlite::Connection>>, HealthError> {
        self.db.lock().map_err(|_| HealthError::Failed {
            message: "archive connection lock poisoned".into(),
        })
    }

    fn clear_db(&self) -> Result<(), HealthError> {
        *self.lock_db()? = None;
        Ok(())
    }

    fn cache_open(&self) -> Result<(), HealthError> {
        *self.lock_db()? = Some(open(&self.root)?);
        Ok(())
    }

    fn with_db<T>(
        &self,
        f: impl FnOnce(&rusqlite::Connection) -> Result<T, HealthError>,
    ) -> Result<T, HealthError> {
        let mut guard = self.lock_db()?;
        if guard.is_none() {
            *guard = Some(open(&self.root)?);
        }
        let db = guard.as_ref().ok_or_else(|| HealthError::Failed {
            message: "archive connection missing".into(),
        })?;
        f(db)
    }
}

#[derive(uniffi::Error, Debug, Clone, PartialEq, Eq)]
pub enum HealthError {
    Invalid { message: String },
    Io { message: String },
    MissingDatabase { message: String },
    Failed { message: String },
}

impl From<health_core::Error> for HealthError {
    fn from(err: health_core::Error) -> Self {
        match err {
            health_core::Error::Invalid(message) => HealthError::Invalid { message },
            health_core::Error::Io(err) => HealthError::Io {
                message: err.to_string(),
            },
            health_core::Error::MissingDatabase { path } => HealthError::MissingDatabase {
                message: format!("query: database missing at {}; run rebuild", path.display()),
            },
            health_core::Error::Sqlite(err) => HealthError::Failed {
                message: err.to_string(),
            },
            health_core::Error::Log(err) => HealthError::Failed {
                message: err.to_string(),
            },
            health_core::Error::Blob(err) => HealthError::Failed {
                message: err.to_string(),
            },
            health_core::Error::Json(err) => HealthError::Failed {
                message: err.to_string(),
            },
        }
    }
}

impl std::fmt::Display for HealthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HealthError::Invalid { message }
            | HealthError::Io { message }
            | HealthError::MissingDatabase { message }
            | HealthError::Failed { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for HealthError {}
