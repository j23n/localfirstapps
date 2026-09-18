//! Display-ready rows both shells bind (ADR 0003 R5 / R6, ADR 0004 R4).
//!
//! These types are not UniFFI. `health-ffi` copies them onto the wire.

use crate::catalog::{short_kind, KindInfo};
use crate::gaps::GapReport;
use crate::portable::{FsckReport, Manifest};
use crate::projection::{Counts, EventRow as ProjectedEvent, ObservationRow, RebuildReport};

/// `text-row` for one projected event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRow {
    /// Event id.
    pub id: String,
    /// Event type token.
    pub title: String,
    /// `{dev} · {ts}`.
    pub subtitle: Option<String>,
    /// `retracted` or `superseded` when the event is voided.
    pub trailing: Option<String>,
}

/// Kind catalog row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindRow {
    /// Canonical kind identifier.
    pub id: String,
    /// Short display title ([`short_kind`]).
    pub title: String,
    /// Unit, when present.
    pub subtitle: Option<String>,
    /// Observation/episode count.
    pub trailing: Option<String>,
}

/// `chart-row` (ADR 0004 R4).
#[derive(Debug, Clone, PartialEq)]
pub struct ChartRow {
    /// Short kind title.
    pub title: String,
    /// First observation's local day.
    pub subtitle: Option<String>,
    /// Unit from the first observation that has one.
    pub unit: Option<String>,
    /// Latest raw value.
    pub latest: Option<String>,
    /// Parsed numeric values in order.
    pub values: Vec<f64>,
}

/// Gap report as status + missing-day ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapStructure {
    /// Kind short name, or the table name when unscoped.
    pub title: String,
    /// `{first} – {last}` when a span exists.
    pub detail: Option<String>,
    /// Days with at least one row.
    pub present_days: u32,
    /// Days in the span with no row.
    pub missing_days: u32,
    /// Missing local dates.
    pub missing: Vec<String>,
}

/// Rebuild / fsck / export / restore command result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveCommandResult {
    /// Whether the command completed without issues.
    pub ok: bool,
    /// Primary status line.
    pub title: String,
    /// Optional detail (torn tails, issue count, blob count).
    pub detail: Option<String>,
}

/// Table totals after rebuild.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveCounts {
    /// Projected events.
    pub events: u32,
    /// Distinct blobs.
    pub blobs: u32,
    /// Observation rows.
    pub observations: u32,
    /// Episode rows.
    pub episodes: u32,
}

/// Display rows for already-filtered projected events.
///
/// `current_only` is applied by the caller via [`crate::Filter::current`].
#[must_use]
pub fn event_rows(rows: &[ProjectedEvent]) -> Vec<EventRow> {
    rows.iter()
        .map(|row| EventRow {
            id: row.id.clone(),
            title: row.event_type.clone(),
            subtitle: Some(format!("{} · {}", row.dev, row.ts)),
            trailing: if row.retracted {
                Some("retracted".into())
            } else {
                row.superseded_by.as_ref().map(|_| "superseded".into())
            },
        })
        .collect()
}

/// Catalog rows: title is [`short_kind`], trailing is the count.
#[must_use]
pub fn kind_rows(kinds: &[KindInfo]) -> Vec<KindRow> {
    kinds
        .iter()
        .map(|kind| KindRow {
            id: kind.kind.clone(),
            title: short_kind(&kind.kind).to_string(),
            subtitle: (!kind.unit.is_empty()).then(|| kind.unit.clone()),
            trailing: Some(kind.n.to_string()),
        })
        .collect()
}

/// Chart row for one kind's observations.
#[must_use]
pub fn chart_row(kind: &str, observations: &[ObservationRow]) -> ChartRow {
    let values: Vec<f64> = observations
        .iter()
        .filter_map(|row| row.value.as_deref()?.parse().ok())
        .collect();
    let latest = observations.last().and_then(|row| row.value.clone());
    let unit = observations.iter().find_map(|row| row.unit.clone());
    ChartRow {
        title: short_kind(kind).to_string(),
        subtitle: observations
            .first()
            .map(|row| row.start_ts[..10.min(row.start_ts.len())].to_string()),
        unit,
        latest,
        values,
    }
}

/// Gap report as a status structure.
#[must_use]
pub fn gap_structure(report: GapReport) -> GapStructure {
    let present_days = report.present_n() as u32;
    let missing_days = report.missing_n() as u32;
    let title = if report.kind.is_empty() {
        report.table
    } else {
        short_kind(&report.kind).to_string()
    };
    GapStructure {
        title,
        detail: (!report.first.is_empty()).then(|| format!("{} – {}", report.first, report.last)),
        present_days,
        missing_days,
        missing: report.missing,
    }
}

/// Saturating table counts for the FFI / shell DTO.
#[must_use]
pub fn counts_dto(counts: Counts) -> ArchiveCounts {
    ArchiveCounts {
        events: sat(counts.events),
        blobs: sat(counts.blobs),
        observations: sat(counts.observations),
        episodes: sat(counts.episodes),
    }
}

/// Rebuild command copy.
#[must_use]
pub fn rebuild_result(report: &RebuildReport) -> ArchiveCommandResult {
    let torn = report.torn_tails.len();
    ArchiveCommandResult {
        ok: torn == 0,
        title: format!("{} events", report.events.len()),
        detail: (torn > 0).then(|| format!("{torn} torn tail(s)")),
    }
}

/// Fsck command copy.
#[must_use]
pub fn fsck_result(report: &FsckReport) -> ArchiveCommandResult {
    ArchiveCommandResult {
        ok: report.ok(),
        title: format!("{} events, {} blobs", report.events, report.blobs),
        detail: (!report.ok()).then(|| format!("{} issue(s)", report.issues.len())),
    }
}

/// Export command copy.
#[must_use]
pub fn export_result(man: &Manifest) -> ArchiveCommandResult {
    ArchiveCommandResult {
        ok: true,
        title: format!("exported {} events", man.event_count),
        detail: Some(format!("{} blobs", man.blob_count)),
    }
}

/// Restore command copy.
#[must_use]
pub fn restore_result() -> ArchiveCommandResult {
    ArchiveCommandResult {
        ok: true,
        title: "restored".into(),
        detail: None,
    }
}

fn sat(n: i64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}
