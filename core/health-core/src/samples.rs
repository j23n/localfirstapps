//! Stream NDJSON sample blobs into the projection.
//!
//! Bulk samples stay in the blob. One `blob_import` names the file. Lines are
//! `{ "class": "observation"|"episode", "item": { ... } }` — the same shape as
//! the retired export golden. XML is never parsed.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use localcore_log::{parse_blob_import, Event, TYPE_BLOB_IMPORT, TYPE_OBSERVATION};
use rusqlite::{params, Transaction};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{Error, Result};

/// Blob kinds that carry canonical sample NDJSON.
pub const SAMPLE_BLOB_KINDS: &[&str] = &["samples", "healthkit", "fit"];

#[derive(Debug, Deserialize)]
struct SampleLine {
    class: String,
    item: Value,
}

pub(crate) fn project_log_events(tx: &Transaction<'_>, events: &[Event]) -> Result<()> {
    for ev in events {
        if is_voided(tx, &ev.id)? {
            continue;
        }
        match ev.event_type.as_str() {
            TYPE_OBSERVATION => insert_observation(tx, &ev.body, None, Some(&ev.id))?,
            "episode" => insert_episode(tx, &ev.body, None, Some(&ev.id))?,
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn project_sample_blobs(
    tx: &Transaction<'_>,
    root: &Path,
    events: &[Event],
) -> Result<()> {
    for ev in events {
        if ev.event_type != TYPE_BLOB_IMPORT || is_voided(tx, &ev.id)? {
            continue;
        }
        let blob = parse_blob_import(&ev.body).map_err(|err| Error::Invalid(err.to_string()))?;
        let path = localcore_blob::path(root, &blob.sha256)?;
        if !path.exists() {
            continue;
        }
        if !is_sample_blob(&blob.kind, &path)? {
            continue;
        }
        stream_sample_file(tx, &path, &blob.sha256, &ev.id)?;
    }
    Ok(())
}

fn is_sample_blob(kind: &str, path: &Path) -> Result<bool> {
    if SAMPLE_BLOB_KINDS.contains(&kind) {
        return Ok(true);
    }
    if kind == "apple" {
        return Ok(false);
    }
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(false);
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        return Ok(serde_json::from_str::<SampleLine>(trimmed).is_ok());
    }
}

fn stream_sample_file(tx: &Transaction<'_>, path: &Path, sha: &str, event_id: &str) -> Result<()> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    for (idx, line) in reader.lines().enumerate() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed: SampleLine = serde_json::from_str(trimmed)
            .map_err(|err| Error::Invalid(format!("{}:{}: {err}", path.display(), idx + 1)))?;
        match parsed.class.as_str() {
            "observation" => insert_observation(tx, &parsed.item, Some(sha), Some(event_id))?,
            "episode" => insert_episode(tx, &parsed.item, Some(sha), Some(event_id))?,
            other => {
                return Err(Error::Invalid(format!(
                    "{}:{}: unknown sample class {other}",
                    path.display(),
                    idx + 1
                )))
            }
        }
    }
    Ok(())
}

fn insert_observation(
    tx: &Transaction<'_>,
    item: &Value,
    blob_sha: Option<&str>,
    event_id: Option<&str>,
) -> Result<()> {
    let dedup = text(item, "dedup_key")
        .ok_or_else(|| Error::Invalid("observation missing dedup_key".into()))?;
    let n = next_n(tx, "observations", &dedup)?;
    let metadata = item.get("metadata").cloned().unwrap_or_else(|| json!({}));
    let metadata = if metadata.is_array() {
        json!({ "entries": metadata })
    } else {
        metadata
    };
    tx.execute(
        "INSERT INTO observations
         (dedup_key, n, kind, source, source_version, device, start_ts, end_ts,
          start_offset, end_offset, unit, value, metadata, blob_sha256, event_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            dedup,
            n,
            text(item, "kind").unwrap_or_default(),
            text(item, "source").unwrap_or_default(),
            text(item, "source_version"),
            text(item, "device"),
            text(item, "start_ts").unwrap_or_default(),
            text(item, "end_ts").unwrap_or_default(),
            text(item, "start_offset"),
            text(item, "end_offset"),
            text(item, "unit"),
            text(item, "value"),
            serde_json::to_string(&metadata)?,
            blob_sha,
            event_id,
        ],
    )?;
    Ok(())
}

fn insert_episode(
    tx: &Transaction<'_>,
    item: &Value,
    blob_sha: Option<&str>,
    event_id: Option<&str>,
) -> Result<()> {
    let dedup = text(item, "dedup_key")
        .ok_or_else(|| Error::Invalid("episode missing dedup_key".into()))?;
    let body = item.get("body").cloned().unwrap_or_else(|| json!({}));
    tx.execute(
        "INSERT OR IGNORE INTO episodes
         (dedup_key, kind, source, start_ts, end_ts, start_offset, end_offset, body, blob_sha256, event_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            dedup,
            text(item, "kind").unwrap_or_default(),
            text(item, "source"),
            text(item, "start_ts").unwrap_or_default(),
            text(item, "end_ts").unwrap_or_default(),
            text(item, "start_offset"),
            text(item, "end_offset"),
            serde_json::to_string(&body)?,
            blob_sha,
            event_id,
        ],
    )?;
    Ok(())
}

fn next_n(tx: &Transaction<'_>, table: &str, dedup: &str) -> Result<i64> {
    let n: i64 = tx.query_row(
        &format!("SELECT COUNT(*) FROM {table} WHERE dedup_key = ?1"),
        [dedup],
        |row| row.get(0),
    )?;
    Ok(n + 1)
}

fn is_voided(tx: &Transaction<'_>, id: &str) -> Result<bool> {
    let retracted: i64 = tx
        .query_row("SELECT retracted FROM events WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .unwrap_or(0);
    let superseded: Option<String> = tx
        .query_row(
            "SELECT superseded_by FROM events WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .ok()
        .flatten();
    Ok(retracted != 0 || superseded.is_some())
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}
