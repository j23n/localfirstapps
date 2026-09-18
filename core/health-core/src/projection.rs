//! Disposable SQLite projection rebuilt from `log/` + `blobs/`.
//!
//! Apple `export.xml` is not read. Sample rows come from log events or from
//! streamed NDJSON sample blobs (ADR 0008).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use localcore_log::{
    parse_blob_import, Event, ReadReport, TYPE_BLOB_IMPORT, TYPE_RETRACT, TYPE_SUPERSEDE,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::Config;
use crate::samples;
use crate::{Error, Result};

/// SQLite application_id `'ARCH'`.
pub const APPLICATION_ID: i32 = 1_095_913_288;

const PRAGMAS: &[&str] = &[
    "PRAGMA journal_mode = OFF",
    "PRAGMA synchronous = OFF",
    "PRAGMA page_size = 4096",
    "PRAGMA encoding = 'UTF-8'",
    "PRAGMA user_version = 0",
    "PRAGMA application_id = 1095913288",
];

const TABLES: &[&str] = &[
    "CREATE TABLE events (
  id TEXT PRIMARY KEY,
  ts TEXT NOT NULL,
  dev TEXT NOT NULL,
  type TEXT NOT NULL,
  body TEXT NOT NULL,
  superseded_by TEXT,
  retracted INTEGER NOT NULL DEFAULT 0
)",
    "CREATE INDEX events_ts ON events(ts)",
    "CREATE INDEX events_type ON events(type)",
    "CREATE INDEX events_dev ON events(dev)",
    "CREATE TABLE blobs (
  sha256 TEXT PRIMARY KEY,
  size INTEGER NOT NULL,
  name TEXT NOT NULL
)",
    "CREATE TABLE observations (
  dedup_key TEXT NOT NULL,
  n INTEGER NOT NULL,
  kind TEXT NOT NULL,
  source TEXT NOT NULL,
  source_version TEXT,
  device TEXT,
  start_ts TEXT NOT NULL,
  end_ts TEXT NOT NULL,
  start_offset TEXT,
  end_offset TEXT,
  unit TEXT,
  value TEXT,
  metadata TEXT NOT NULL DEFAULT '{}',
  blob_sha256 TEXT,
  event_id TEXT,
  PRIMARY KEY (dedup_key, n)
)",
    "CREATE INDEX observations_kind_start ON observations(kind, start_ts)",
    "CREATE INDEX observations_source ON observations(source)",
    "CREATE TABLE episodes (
  dedup_key TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  source TEXT,
  start_ts TEXT NOT NULL,
  end_ts TEXT NOT NULL,
  start_offset TEXT,
  end_offset TEXT,
  body TEXT NOT NULL,
  blob_sha256 TEXT,
  event_id TEXT,
  route_polyline BLOB,
  route_segments INTEGER,
  route_points INTEGER,
  route_bbox TEXT,
  ascent_m REAL,
  descent_m REAL,
  moving_seconds INTEGER,
  elapsed_seconds INTEGER,
  pause_count INTEGER
)",
    "CREATE INDEX episodes_kind_start ON episodes(kind, start_ts)",
    "CREATE TABLE workout_event (
  episode_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  kind TEXT NOT NULL,
  time_utc TEXT NOT NULL,
  PRIMARY KEY (episode_id, seq)
)",
    "CREATE TABLE source_precedence (
  source_name TEXT PRIMARY KEY,
  rank INTEGER NOT NULL
)",
    "CREATE TABLE attachments (
  blob_sha256 TEXT NOT NULL,
  path TEXT NOT NULL,
  size INTEGER NOT NULL,
  PRIMARY KEY (blob_sha256, path)
)",
    "CREATE VIEW preferred_observations AS
SELECT o.* FROM observations o
JOIN source_precedence sp ON o.source = sp.source_name
WHERE NOT EXISTS (
  SELECT 1 FROM observations o2
  JOIN source_precedence sp2 ON o2.source = sp2.source_name
  WHERE o2.kind = o.kind
    AND o2.start_ts = o.start_ts
    AND o2.end_ts = o.end_ts
    AND (sp2.rank < sp.rank OR (sp2.rank = sp.rank AND o2.source < o.source))
)",
];

/// Derived database location under `root`.
pub fn db_path(root: impl AsRef<Path>) -> PathBuf {
    root.as_ref().join("derived").join("archive.db")
}

/// Rebuild report: recovered events plus ignored torn tails.
#[derive(Debug)]
pub struct RebuildReport {
    pub events: Vec<Event>,
    pub torn_tails: Vec<localcore_log::TornTailDiagnostic>,
}

/// Reconstruct `derived/archive.db` from every complete log line.
pub fn rebuild(root: impl AsRef<Path>) -> Result<RebuildReport> {
    let root = root.as_ref();
    let report = localcore_log::read_report(root)?;
    rebuild_from(root, &report)?;
    Ok(RebuildReport {
        events: report.events,
        torn_tails: report.torn_tails.iter().map(Into::into).collect(),
    })
}

fn rebuild_from(root: &Path, report: &ReadReport) -> Result<()> {
    let path = db_path(root);
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let p = PathBuf::from(format!("{}{suffix}", path.display()));
        if p.exists() {
            fs::remove_file(&p)?;
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }

    let mut db = Connection::open(&path)?;
    apply_pragmas(&db)?;
    for stmt in TABLES {
        db.execute(stmt, [])?;
    }

    let voided = voided_ids(&report.events);
    let mut next_n = HashMap::new();
    let tx = db.transaction()?;
    insert_events(&tx, &report.events)?;
    apply_corrections(&tx, &report.events)?;
    samples::project_log_events(&tx, &report.events, &voided, &mut next_n)?;
    samples::project_sample_blobs(&tx, root, &report.events, &voided, &mut next_n)?;
    write_source_precedence(&tx, root)?;
    tx.commit()?;
    db.execute_batch("VACUUM")?;
    chmod_db(&path);
    Ok(())
}

fn apply_pragmas(db: &Connection) -> Result<()> {
    // [`PRAGMAS`] documents the set; returning PRAGMAs cannot use `execute`.
    let _ = PRAGMAS;
    db.pragma_update(None, "journal_mode", "OFF")?;
    db.pragma_update(None, "synchronous", "OFF")?;
    db.pragma_update(None, "page_size", 4096)?;
    db.pragma_update(None, "user_version", 0)?;
    db.pragma_update(None, "application_id", APPLICATION_ID)?;
    Ok(())
}

fn chmod_db(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for suffix in ["", "-wal", "-shm"] {
            let p = PathBuf::from(format!("{}{suffix}", path.display()));
            let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o600));
        }
    }
}

fn insert_events(tx: &rusqlite::Transaction<'_>, events: &[Event]) -> Result<()> {
    let mut ins_ev =
        tx.prepare("INSERT INTO events (id, ts, dev, type, body) VALUES (?1, ?2, ?3, ?4, ?5)")?;
    let mut ins_blob =
        tx.prepare("INSERT OR IGNORE INTO blobs (sha256, size, name) VALUES (?1, ?2, ?3)")?;
    for ev in events {
        let body = if ev.body.is_null() {
            "{}".to_string()
        } else {
            serde_json::to_string(&ev.body)?
        };
        ins_ev.execute(params![ev.id, ev.ts, ev.dev, ev.event_type, body])?;
        if ev.event_type != TYPE_BLOB_IMPORT {
            continue;
        }
        let blob = parse_blob_import(&ev.body).map_err(|err| Error::Invalid(err.to_string()))?;
        ins_blob.execute(params![blob.sha256, blob.size, blob.name])?;
    }
    Ok(())
}

fn apply_corrections(tx: &rusqlite::Transaction<'_>, events: &[Event]) -> Result<()> {
    for ev in events {
        if ev.event_type != TYPE_SUPERSEDE && ev.event_type != TYPE_RETRACT {
            continue;
        }
        let target = ev.target().ok_or_else(|| {
            Error::Invalid(format!("{} {}: missing target", ev.event_type, ev.id))
        })?;
        match ev.event_type.as_str() {
            TYPE_SUPERSEDE => {
                tx.execute(
                    "UPDATE events SET superseded_by = ?1, retracted = 0 WHERE id = ?2",
                    params![ev.id, target],
                )?;
            }
            TYPE_RETRACT => {
                tx.execute(
                    "UPDATE events SET retracted = 1, superseded_by = NULL WHERE id = ?1",
                    params![target],
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn write_source_precedence(tx: &rusqlite::Transaction<'_>, root: &Path) -> Result<()> {
    let cfg = Config::load(root);
    let mut seen = std::collections::BTreeSet::new();
    for (name, rank) in &cfg.source_ranks {
        tx.execute(
            "INSERT OR REPLACE INTO source_precedence (source_name, rank) VALUES (?1, ?2)",
            params![name, rank],
        )?;
        seen.insert(name.clone());
    }
    let mut extras = Vec::new();
    {
        let mut stmt = tx.prepare("SELECT DISTINCT source FROM observations WHERE source != ''")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        for source in rows {
            let source = source?;
            if !seen.contains(&source) {
                extras.push(source);
            }
        }
    }
    extras.sort();
    for source in extras {
        tx.execute(
            "INSERT OR REPLACE INTO source_precedence (source_name, rank) VALUES (?1, ?2)",
            params![source, cfg.source_rank(&source)],
        )?;
    }
    Ok(())
}

/// Open the derived database.
pub fn open(root: impl AsRef<Path>) -> Result<Connection> {
    let path = db_path(root);
    if !path.exists() {
        return Err(Error::MissingDatabase { path });
    }
    let db = Connection::open(path)?;
    db.pragma_update(None, "foreign_keys", "ON")?;
    Ok(db)
}

/// Event query filter.
#[derive(Debug, Default, Clone)]
pub struct Filter {
    pub event_type: Option<String>,
    pub dev: Option<String>,
    pub id: Option<String>,
    pub current: bool,
}

/// One projected event row, including correction flags.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventRow {
    pub id: String,
    pub ts: String,
    pub dev: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub body: serde_json::Value,
    pub superseded_by: Option<String>,
    pub retracted: bool,
}

/// Events matching `filter`, ordered by `(ts, id)`.
pub fn query(db: &Connection, filter: &Filter) -> Result<Vec<EventRow>> {
    let mut sql = String::from(
        "SELECT id, ts, dev, type, body, superseded_by, retracted FROM events WHERE 1=1",
    );
    let mut args: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(ty) = &filter.event_type {
        sql.push_str(" AND type = ?");
        args.push(ty.clone().into());
    }
    if let Some(dev) = &filter.dev {
        sql.push_str(" AND dev = ?");
        args.push(dev.clone().into());
    }
    if let Some(id) = &filter.id {
        sql.push_str(" AND id = ?");
        args.push(id.clone().into());
    }
    if filter.current {
        sql.push_str(" AND retracted = 0 AND superseded_by IS NULL");
    }
    sql.push_str(" ORDER BY ts, id");
    let mut stmt = db.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args), |row| {
        let body: String = row.get(4)?;
        Ok(EventRow {
            id: row.get(0)?,
            ts: row.get(1)?,
            dev: row.get(2)?,
            event_type: row.get(3)?,
            body: serde_json::from_str(&body).unwrap_or(json!({})),
            superseded_by: row.get(5)?,
            retracted: row.get::<_, i64>(6)? != 0,
        })
    })?;
    rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
}

/// Row totals after a rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Counts {
    pub events: i64,
    pub blobs: i64,
    pub observations: i64,
    pub episodes: i64,
}

/// Count the four primary tables.
pub fn table_counts(db: &Connection) -> Result<Counts> {
    Ok(Counts {
        events: count(db, "events")?,
        blobs: count(db, "blobs")?,
        observations: count(db, "observations")?,
        episodes: count(db, "episodes")?,
    })
}

fn count(db: &Connection, table: &str) -> Result<i64> {
    Ok(
        db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })?,
    )
}

/// One observation row, ordered for semantic dumps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservationRow {
    pub dedup_key: String,
    pub n: i64,
    pub kind: String,
    pub source: String,
    pub source_version: Option<String>,
    pub device: Option<String>,
    pub start_ts: String,
    pub end_ts: String,
    pub start_offset: Option<String>,
    pub end_offset: Option<String>,
    pub unit: Option<String>,
    pub value: Option<String>,
    pub metadata: serde_json::Value,
    pub blob_sha256: Option<String>,
    pub event_id: Option<String>,
}

/// Observations ordered by `(start_ts, dedup_key, n)`.
pub fn query_observations(
    db: &Connection,
    kind: Option<&str>,
    source: Option<&str>,
) -> Result<Vec<ObservationRow>> {
    let mut sql = String::from(
        "SELECT dedup_key, n, kind, source, source_version, device, start_ts, end_ts,
                start_offset, end_offset, unit, value, metadata, blob_sha256, event_id
         FROM observations WHERE 1=1",
    );
    let mut args: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(kind) = kind {
        sql.push_str(" AND kind = ?");
        args.push(kind.to_string().into());
    }
    if let Some(source) = source {
        sql.push_str(" AND source = ?");
        args.push(source.to_string().into());
    }
    sql.push_str(" ORDER BY start_ts, dedup_key, n");
    let mut stmt = db.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args), |row| {
        let metadata: String = row.get(12)?;
        Ok(ObservationRow {
            dedup_key: row.get(0)?,
            n: row.get(1)?,
            kind: row.get(2)?,
            source: row.get(3)?,
            source_version: optional_text(row.get(4)?),
            device: optional_text(row.get(5)?),
            start_ts: row.get(6)?,
            end_ts: row.get(7)?,
            start_offset: optional_text(row.get(8)?),
            end_offset: optional_text(row.get(9)?),
            unit: optional_text(row.get(10)?),
            value: optional_text(row.get(11)?),
            metadata: serde_json::from_str(&metadata).unwrap_or(json!({})),
            blob_sha256: optional_text(row.get(13)?),
            event_id: optional_text(row.get(14)?),
        })
    })?;
    rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
}

/// One episode row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpisodeRow {
    pub dedup_key: String,
    pub kind: String,
    pub source: Option<String>,
    pub start_ts: String,
    pub end_ts: String,
    pub start_offset: Option<String>,
    pub end_offset: Option<String>,
    pub body: serde_json::Value,
    pub blob_sha256: Option<String>,
    pub event_id: Option<String>,
}

/// Episodes ordered by `(start_ts, dedup_key)`.
pub fn query_episodes(db: &Connection, kind: Option<&str>) -> Result<Vec<EpisodeRow>> {
    let mut sql = String::from(
        "SELECT dedup_key, kind, source, start_ts, end_ts, start_offset, end_offset, body, blob_sha256, event_id
         FROM episodes WHERE 1=1",
    );
    let mut args: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(kind) = kind {
        sql.push_str(" AND kind = ?");
        args.push(kind.to_string().into());
    }
    sql.push_str(" ORDER BY start_ts, dedup_key");
    let mut stmt = db.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args), |row| {
        let body: String = row.get(7)?;
        Ok(EpisodeRow {
            dedup_key: row.get(0)?,
            kind: row.get(1)?,
            source: optional_text(row.get(2)?),
            start_ts: row.get(3)?,
            end_ts: row.get(4)?,
            start_offset: optional_text(row.get(5)?),
            end_offset: optional_text(row.get(6)?),
            body: serde_json::from_str(&body).unwrap_or(json!({})),
            blob_sha256: optional_text(row.get(8)?),
            event_id: optional_text(row.get(9)?),
        })
    })?;
    rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
}

fn optional_text(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.is_empty())
}

/// Ordered semantic dump of the four primary tables. Not SQLite bytes.
pub fn semantic_dump(db: &Connection) -> Result<serde_json::Value> {
    let events = query(db, &Filter::default())?;
    let mut blob_stmt = db.prepare("SELECT sha256, size, name FROM blobs ORDER BY sha256")?;
    let blobs = blob_stmt
        .query_map([], |row| {
            Ok(json!({
                "sha256": row.get::<_, String>(0)?,
                "size": row.get::<_, i64>(1)?,
                "name": row.get::<_, String>(2)?,
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!({
        "events": events,
        "blobs": blobs,
        "observations": query_observations(db, None, None)?,
        "episodes": query_episodes(db, None)?,
    }))
}

/// Event ids retracted or superseded by a correction in `events`.
///
/// Same definition [`is_voided`] reads back from the projection after
/// [`apply_corrections`]: any `retract`/`supersede` target is voided.
pub fn voided_ids(events: &[Event]) -> HashSet<String> {
    let mut out = HashSet::new();
    for ev in events {
        if ev.event_type != TYPE_RETRACT && ev.event_type != TYPE_SUPERSEDE {
            continue;
        }
        if let Some(target) = ev.target() {
            out.insert(target);
        }
    }
    out
}

/// Whether an event id is currently retracted or superseded.
pub fn is_voided(db: &Connection, id: &str) -> Result<bool> {
    let flag: Option<(Option<String>, i64)> = db
        .query_row(
            "SELECT superseded_by, retracted FROM events WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(match flag {
        Some((superseded, retracted)) => superseded.is_some() || retracted != 0,
        None => false,
    })
}
