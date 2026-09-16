//! Local-calendar gap reports. "None seen" is not completeness (ADR 0008 R8).

use std::collections::BTreeSet;

use rusqlite::Connection;
use serde::Serialize;

use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct GapReport {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub table: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub first: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last: String,
    pub present: Vec<String>,
    pub missing: Vec<String>,
}

impl GapReport {
    pub fn present_n(&self) -> usize {
        self.present.len()
    }

    pub fn missing_n(&self) -> usize {
        self.missing.len()
    }

    pub fn span_days(&self) -> i64 {
        match (parse_day(&self.first), parse_day(&self.last)) {
            (Some(a), Some(b)) if b >= a => (b - a) + 1,
            _ => 0,
        }
    }
}

/// Missing local days for one observation kind, or every observation.
pub fn observation_gaps(db: &Connection, kind: Option<&str>) -> Result<GapReport> {
    scan_gaps(db, "observations", kind)
}

/// Missing local days for one episode kind, or every episode.
pub fn episode_gaps(db: &Connection, kind: Option<&str>) -> Result<GapReport> {
    scan_gaps(db, "episodes", kind)
}

/// Days with neither an observation nor an episode.
pub fn overall_gaps(db: &Connection) -> Result<GapReport> {
    let obs = observation_gaps(db, None)?;
    let eps = episode_gaps(db, None)?;
    let mut set = BTreeSet::new();
    set.extend(obs.present);
    set.extend(eps.present);
    if set.is_empty() {
        return Ok(GapReport {
            table: "overall".into(),
            ..GapReport::default()
        });
    }
    let present: Vec<String> = set.iter().cloned().collect();
    let first = present[0].clone();
    let last = present[present.len() - 1].clone();
    let missing = missing_between(&first, &last, &set);
    Ok(GapReport {
        table: "overall".into(),
        first,
        last,
        present,
        missing,
        ..GapReport::default()
    })
}

fn scan_gaps(db: &Connection, table: &str, kind: Option<&str>) -> Result<GapReport> {
    let mut sql = format!("SELECT start_ts FROM {table}");
    if kind.is_some() {
        sql.push_str(" WHERE kind = ?1");
    }
    sql.push_str(" ORDER BY start_ts");
    let mut stmt = db.prepare(&sql)?;
    let mut set = BTreeSet::new();
    if let Some(kind) = kind {
        let rows = stmt.query_map([kind], |row| row.get::<_, String>(0))?;
        for ts in rows {
            set.insert(local_day(&ts?)?);
        }
    } else {
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        for ts in rows {
            set.insert(local_day(&ts?)?);
        }
    }
    let mut g = GapReport {
        kind: kind.unwrap_or("").to_string(),
        table: table.to_string(),
        ..GapReport::default()
    };
    if set.is_empty() {
        return Ok(g);
    }
    g.present = set.iter().cloned().collect();
    g.first = g.present[0].clone();
    g.last = g.present[g.present.len() - 1].clone();
    g.missing = missing_between(&g.first, &g.last, &set);
    Ok(g)
}

fn local_day(ts: &str) -> Result<String> {
    if ts.len() >= 10 {
        Ok(ts[..10].to_string())
    } else {
        Err(Error::Invalid(format!("start_ts {ts:?}")))
    }
}

fn parse_day(day: &str) -> Option<i64> {
    if day.len() != 10 {
        return None;
    }
    let y: i64 = day[0..4].parse().ok()?;
    let m: i64 = day[5..7].parse().ok()?;
    let d: i64 = day[8..10].parse().ok()?;
    Some(y * 400 + m * 32 + d)
}

fn missing_between(first: &str, last: &str, present: &BTreeSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = first.to_string();
    while cur.as_str() <= last {
        if !present.contains(&cur) {
            out.push(cur.clone());
        }
        if cur == last {
            break;
        }
        cur = next_day(&cur);
        if cur.is_empty() {
            break;
        }
    }
    out
}

fn next_day(day: &str) -> String {
    let Ok(y) = day[0..4].parse::<i32>() else {
        return String::new();
    };
    let Ok(m) = day[5..7].parse::<u32>() else {
        return String::new();
    };
    let Ok(d) = day[8..10].parse::<u32>() else {
        return String::new();
    };
    let days = days_in_month(y, m);
    if d < days {
        return format!("{y:04}-{m:02}-{:02}", d + 1);
    }
    if m < 12 {
        return format!("{y:04}-{:02}-01", m + 1);
    }
    format!("{:04}-01-01", y + 1)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 31,
    }
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}
