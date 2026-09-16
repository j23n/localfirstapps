//! Kind catalog computed from projected tables.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;
use serde::Serialize;

use crate::Result;

const KIND_PREFIXES: &[&str] = &[
    "HKQuantityTypeIdentifier",
    "HKCategoryTypeIdentifier",
    "HKCorrelationTypeIdentifier",
    "HKWorkoutActivityType",
    "HKDataType",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KindInfo {
    pub kind: String,
    pub table: String,
    pub n: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub from: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub to: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub unit: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub preferred: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub overlaps: bool,
}

/// Every projected kind with counts, range, and sources.
pub fn kind_catalog(db: &Connection) -> Result<Vec<KindInfo>> {
    let mut out = kind_rows(db, "observations")?;
    out.extend(kind_rows(db, "episodes")?);
    attach_observation_sources(db, &mut out)?;
    attach_episode_sources(db, &mut out)?;
    attach_overlaps(db, &mut out)?;
    out.sort_by(|a, b| match (a.table.as_str(), b.table.as_str()) {
        ("observations", "episodes") => std::cmp::Ordering::Less,
        ("episodes", "observations") => std::cmp::Ordering::Greater,
        _ => short_kind(&a.kind).cmp(short_kind(&b.kind)),
    });
    Ok(out)
}

fn kind_rows(db: &Connection, table: &str) -> Result<Vec<KindInfo>> {
    let mut stmt = db.prepare(&format!(
        "SELECT kind, COUNT(*), MIN(start_ts), MAX(start_ts) FROM {table} GROUP BY kind"
    ))?;
    let rows = stmt.query_map([], |row| {
        Ok(KindInfo {
            kind: row.get(0)?,
            table: table.to_string(),
            n: row.get(1)?,
            from: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            to: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            unit: String::new(),
            sources: Vec::new(),
            preferred: String::new(),
            overlaps: false,
        })
    })?;
    rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
}

fn attach_observation_sources(db: &Connection, kinds: &mut [KindInfo]) -> Result<()> {
    let mut by_kind: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut pref: BTreeMap<String, String> = BTreeMap::new();
    let mut stmt = db.prepare(
        "SELECT o.kind, o.source, COUNT(*), IFNULL(sp.rank, 100)
         FROM observations o
         LEFT JOIN source_precedence sp ON o.source = sp.source_name
         GROUP BY o.kind, o.source
         ORDER BY o.kind, IFNULL(sp.rank, 100), o.source",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (kind, source) = row?;
        if source.is_empty() {
            continue;
        }
        by_kind
            .entry(kind.clone())
            .or_default()
            .push(source.clone());
        pref.entry(kind).or_insert(source);
    }
    let mut units: BTreeMap<String, String> = BTreeMap::new();
    let mut ustmt = db.prepare(
        "SELECT kind, MIN(unit) FROM observations WHERE unit IS NOT NULL AND unit != '' GROUP BY kind",
    )?;
    let urows = ustmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in urows {
        let (kind, unit) = row?;
        units.insert(kind, unit);
    }
    for kind in kinds.iter_mut().filter(|k| k.table == "observations") {
        kind.sources = by_kind.get(&kind.kind).cloned().unwrap_or_default();
        kind.preferred = pref.get(&kind.kind).cloned().unwrap_or_default();
        kind.unit = units.get(&kind.kind).cloned().unwrap_or_default();
    }
    Ok(())
}

fn attach_episode_sources(db: &Connection, kinds: &mut [KindInfo]) -> Result<()> {
    let mut by_kind: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut stmt = db.prepare(
        "SELECT kind, IFNULL(source,''), COUNT(*) FROM episodes GROUP BY kind, source ORDER BY kind, source",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (kind, source) = row?;
        if source.is_empty() {
            continue;
        }
        by_kind.entry(kind).or_default().push(source);
    }
    for kind in kinds.iter_mut().filter(|k| k.table == "episodes") {
        kind.sources = by_kind.get(&kind.kind).cloned().unwrap_or_default();
        if let Some(first) = kind.sources.first() {
            kind.preferred = first.clone();
        }
    }
    Ok(())
}

fn attach_overlaps(db: &Connection, kinds: &mut [KindInfo]) -> Result<()> {
    let multi: Vec<String> = kinds
        .iter()
        .filter(|k| k.table == "observations" && k.sources.len() > 1)
        .map(|k| k.kind.clone())
        .collect();
    if multi.is_empty() {
        return Ok(());
    }
    let mut overlapping = BTreeSet::new();
    let mut stmt = db.prepare(
        "SELECT kind, source, substr(start_ts, 1, 10) FROM observations WHERE kind = ?1",
    )?;
    for kind in &multi {
        let mut days: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let rows = stmt.query_map([kind], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;
        for row in rows {
            let (source, day) = row?;
            days.entry(day).or_default().insert(source);
        }
        if days.values().any(|sources| sources.len() > 1) {
            overlapping.insert(kind.clone());
        }
    }
    for kind in kinds {
        if overlapping.contains(&kind.kind) {
            kind.overlaps = true;
        }
    }
    Ok(())
}

/// Strip the Apple type-identifier prefix when present.
pub fn short_kind(kind: &str) -> &str {
    for prefix in KIND_PREFIXES {
        if let Some(rest) = kind.strip_prefix(prefix) {
            return rest;
        }
    }
    kind
}

/// Exact match, else a unique suffix match.
pub fn resolve_kind<'a>(names: &'a [String], query: &str) -> Result<Option<&'a str>> {
    if query.is_empty() {
        return Ok(None);
    }
    if let Some(exact) = names.iter().find(|n| *n == query) {
        return Ok(Some(exact.as_str()));
    }
    let suffix: Vec<&str> = names
        .iter()
        .filter(|n| n.ends_with(query))
        .map(String::as_str)
        .collect();
    match suffix.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(*one)),
        many => Err(crate::Error::Invalid(format!(
            "ambiguous kind {query:?}: {}",
            many.join(", ")
        ))),
    }
}
