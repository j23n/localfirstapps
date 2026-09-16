//! Display-ready UniFFI surface for [`health_core`].
//!
//! The archive root stays on the host. Records that cross are list/status
//! rows, catalog rows, gap summaries, and command results.

uniffi::setup_scaffolding!("HealthCore");

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

/// Rebuild / fsck command result.
///
/// R6 role: command DTO
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ArchiveCommandResult {
    pub ok: bool,
    pub title: String,
    pub detail: Option<String>,
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

/// Host-owned archive root.
#[derive(uniffi::Object)]
pub struct HealthArchive {
    root: String,
}

#[uniffi::export]
impl HealthArchive {
    #[uniffi::constructor]
    pub fn new(root: String) -> Self {
        Self { root }
    }

    pub fn rebuild(&self) -> Result<ArchiveCommandResult, HealthError> {
        let report = rebuild(&self.root)?;
        let torn = report.torn_tails.len();
        Ok(ArchiveCommandResult {
            ok: torn == 0,
            title: format!("{} events", report.events.len()),
            detail: (torn > 0).then(|| format!("{torn} torn tail(s)")),
        })
    }

    pub fn counts(&self) -> Result<ArchiveCounts, HealthError> {
        let db = open(&self.root)?;
        let c = table_counts(&db)?;
        Ok(ArchiveCounts {
            events: sat(c.events),
            blobs: sat(c.blobs),
            observations: sat(c.observations),
            episodes: sat(c.episodes),
        })
    }

    pub fn events(&self, current_only: bool) -> Result<Vec<EventRow>, HealthError> {
        let db = open(&self.root)?;
        let rows = query(
            &db,
            &Filter {
                current: current_only,
                ..Filter::default()
            },
        )?;
        Ok(rows
            .into_iter()
            .map(|row| EventRow {
                id: row.id.clone(),
                title: row.event_type,
                subtitle: Some(format!("{} · {}", row.dev, row.ts)),
                trailing: if row.retracted {
                    Some("retracted".into())
                } else {
                    row.superseded_by.map(|_| "superseded".into())
                },
            })
            .collect())
    }

    pub fn kind_chart(&self, kind: String) -> Result<ChartRow, HealthError> {
        let db = open(&self.root)?;
        let rows = query_observations(&db, Some(&kind), None)?;
        let values: Vec<f64> = rows
            .iter()
            .filter_map(|row| row.value.as_deref()?.parse().ok())
            .collect();
        let latest = rows.last().and_then(|row| row.value.clone());
        let unit = rows.iter().find_map(|row| row.unit.clone());
        Ok(ChartRow {
            title: health_core::short_kind(&kind).to_string(),
            subtitle: rows
                .first()
                .map(|row| row.start_ts[..10.min(row.start_ts.len())].to_string()),
            unit,
            latest,
            values,
        })
    }

    pub fn catalog(&self) -> Result<Vec<KindRow>, HealthError> {
        let db = open(&self.root)?;
        Ok(kind_catalog(&db)?
            .into_iter()
            .map(|kind| KindRow {
                id: kind.kind.clone(),
                title: health_core::short_kind(&kind.kind).to_string(),
                subtitle: (!kind.unit.is_empty()).then_some(kind.unit),
                trailing: Some(kind.n.to_string()),
            })
            .collect())
    }

    pub fn observation_gaps(&self, kind: Option<String>) -> Result<GapStructure, HealthError> {
        let db = open(&self.root)?;
        let report = observation_gaps(&db, kind.as_deref())?;
        Ok(gap_structure(report))
    }

    pub fn episode_gaps(&self, kind: Option<String>) -> Result<GapStructure, HealthError> {
        let db = open(&self.root)?;
        Ok(gap_structure(episode_gaps(&db, kind.as_deref())?))
    }

    pub fn overall_gaps(&self) -> Result<GapStructure, HealthError> {
        let db = open(&self.root)?;
        Ok(gap_structure(overall_gaps(&db)?))
    }

    pub fn fsck(&self) -> Result<ArchiveCommandResult, HealthError> {
        let report = fsck(&self.root)?;
        Ok(ArchiveCommandResult {
            ok: report.ok(),
            title: format!("{} events, {} blobs", report.events, report.blobs),
            detail: (!report.ok()).then(|| format!("{} issue(s)", report.issues.len())),
        })
    }

    pub fn export_archive(&self, out_dir: String) -> Result<ArchiveCommandResult, HealthError> {
        let man = export(&self.root, &out_dir)?;
        Ok(ArchiveCommandResult {
            ok: true,
            title: format!("exported {} events", man.event_count),
            detail: Some(format!("{} blobs", man.blob_count)),
        })
    }

    pub fn restore_archive(&self, from_dir: String) -> Result<ArchiveCommandResult, HealthError> {
        restore(&from_dir, &self.root)?;
        Ok(ArchiveCommandResult {
            ok: true,
            title: "restored".into(),
            detail: None,
        })
    }
}

fn gap_structure(report: health_core::GapReport) -> GapStructure {
    let present_days = report.present_n() as u32;
    let missing_days = report.missing_n() as u32;
    let title = if report.kind.is_empty() {
        report.table
    } else {
        health_core::short_kind(&report.kind).to_string()
    };
    GapStructure {
        title,
        detail: (!report.first.is_empty()).then(|| format!("{} – {}", report.first, report.last)),
        present_days,
        missing_days,
        missing: report.missing,
    }
}

fn sat(n: i64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[derive(uniffi::Error, Debug, Clone, PartialEq, Eq)]
pub enum HealthError {
    Failed { message: String },
}

impl From<health_core::Error> for HealthError {
    fn from(err: health_core::Error) -> Self {
        HealthError::Failed {
            message: err.to_string(),
        }
    }
}

impl std::fmt::Display for HealthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HealthError::Failed { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for HealthError {}
