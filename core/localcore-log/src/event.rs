//! Append-only log record.
//!
//! Ported from health `internal/event`, with gallery person-state types added
//! so one schema serves both consumers (ADR 0005 R13/R14).

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::Error;

/// Canonical UTC timestamp stored in the log (Go `TSFormat`).
pub const TS_FORMAT: &str = "2006-01-02T15:04:05.000000000Z";

pub const TYPE_BLOB_IMPORT: &str = "blob_import";
pub const TYPE_OBSERVATION: &str = "observation";
pub const TYPE_MED_START: &str = "med_start";
pub const TYPE_MED_STOP: &str = "med_stop";
pub const TYPE_MED_EVENT: &str = "med_event";
pub const TYPE_MEDITATION: &str = "meditation";
pub const TYPE_NOTE: &str = "note";
pub const TYPE_EXTRACTION: &str = "extraction";
pub const TYPE_SUPERSEDE: &str = "supersede";
pub const TYPE_RETRACT: &str = "retract";

pub const TYPE_PERSON_HIDDEN: &str = "person_hidden";
pub const TYPE_PERSON_UNHIDDEN: &str = "person_unhidden";
pub const TYPE_PERSON_FEATURED: &str = "person_featured";
pub const TYPE_PERSON_UNFEATURED: &str = "person_unfeatured";
pub const TYPE_FEATURED_PHOTO_SET: &str = "featured_photo_set";
pub const TYPE_FEATURED_PHOTO_CLEAR: &str = "featured_photo_clear";
pub const TYPE_PERSON_ME_SET: &str = "person_me_set";
pub const TYPE_PERSON_ME_CLEAR: &str = "person_me_clear";
pub const TYPE_PERSON_RENAMED: &str = "person_renamed";
pub const TYPE_PERSON_CONTACT_LINK_SET: &str = "person_contact_link_set";
pub const TYPE_PERSON_CONTACT_LINK_CLEAR: &str = "person_contact_link_clear";

/// One NDJSON line. Field order matches the on-disk format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub ts: String,
    pub dev: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub body: serde_json::Value,
}

/// Decoded body of a `blob_import` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobImport {
    pub sha256: String,
    pub size: i64,
    pub name: String,
    pub kind: String,
}

/// Whether `t` is an allowed event type.
pub fn known_type(t: &str) -> bool {
    matches!(
        t,
        TYPE_BLOB_IMPORT
            | TYPE_OBSERVATION
            | TYPE_MED_START
            | TYPE_MED_STOP
            | TYPE_MED_EVENT
            | TYPE_MEDITATION
            | TYPE_NOTE
            | TYPE_EXTRACTION
            | TYPE_SUPERSEDE
            | TYPE_RETRACT
            | TYPE_PERSON_HIDDEN
            | TYPE_PERSON_UNHIDDEN
            | TYPE_PERSON_FEATURED
            | TYPE_PERSON_UNFEATURED
            | TYPE_FEATURED_PHOTO_SET
            | TYPE_FEATURED_PHOTO_CLEAR
            | TYPE_PERSON_ME_SET
            | TYPE_PERSON_ME_CLEAR
            | TYPE_PERSON_RENAMED
            | TYPE_PERSON_CONTACT_LINK_SET
            | TYPE_PERSON_CONTACT_LINK_CLEAR
    )
}

/// Whether `name` is a safe log directory component.
///
/// Matches Go `^[A-Za-z0-9][A-Za-z0-9._-]*$`.
pub fn valid_device(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn parse_digits(s: &[u8]) -> Option<u32> {
    if s.is_empty() || !s.iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut n = 0u32;
    for &b in s {
        n = n.checked_mul(10)?.checked_add(u32::from(b - b'0'))?;
    }
    Some(n)
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 → civil date.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (mp + if mp < 10 { 3 } else { -9 }) as u32;
    ((y + i64::from(m <= 2)) as i32, m, d)
}

fn format_unix(secs: i64, nanos: u32) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400) as u32;
    let (year, month, day) = civil_from_days(days);
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;
    let second = rem % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{nanos:09}Z")
}

/// Canonical timestamp for a newly appended event.
pub fn now_utc() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format_unix(dur.as_secs() as i64, dur.subsec_nanos())
}

/// Parse `YYYY-MM-DDTHH:MM:SS.fffffffffZ` and require the canonical spelling.
fn validate_ts(ts: &str) -> Result<(), Error> {
    if ts.len() != TS_FORMAT.len() {
        return Err(Error::Invalid(
            "ts must be UTC with 9 fractional digits".into(),
        ));
    }
    let b = ts.as_bytes();
    if b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'.'
        || b[29] != b'Z'
    {
        return Err(Error::Invalid(format!("ts: invalid timestamp {ts}")));
    }
    let year = parse_digits(&b[0..4]).ok_or_else(|| Error::Invalid(format!("ts: {ts}")))? as i32;
    let month = parse_digits(&b[5..7]).ok_or_else(|| Error::Invalid(format!("ts: {ts}")))?;
    let day = parse_digits(&b[8..10]).ok_or_else(|| Error::Invalid(format!("ts: {ts}")))?;
    let hour = parse_digits(&b[11..13]).ok_or_else(|| Error::Invalid(format!("ts: {ts}")))?;
    let minute = parse_digits(&b[14..16]).ok_or_else(|| Error::Invalid(format!("ts: {ts}")))?;
    let second = parse_digits(&b[17..19]).ok_or_else(|| Error::Invalid(format!("ts: {ts}")))?;
    let nanos = parse_digits(&b[20..29]).ok_or_else(|| Error::Invalid(format!("ts: {ts}")))?;
    if month < 1 || month > 12 || day < 1 || day > days_in_month(year, month) {
        return Err(Error::Invalid(format!("ts: invalid date {ts}")));
    }
    if hour > 23 || minute > 59 || second > 60 {
        return Err(Error::Invalid(format!("ts: invalid time {ts}")));
    }
    let rebuilt = format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{nanos:09}Z"
    );
    if rebuilt != ts {
        return Err(Error::Invalid(
            "ts must be UTC with 9 fractional digits".into(),
        ));
    }
    Ok(())
}

impl Event {
    pub fn new(
        id: impl Into<String>,
        ts: impl Into<String>,
        dev: impl Into<String>,
        event_type: impl Into<String>,
        body: serde_json::Value,
    ) -> Self {
        Self {
            id: id.into(),
            ts: ts.into(),
            dev: dev.into(),
            event_type: event_type.into(),
            body,
        }
    }

    /// Fields required to append.
    pub fn validate(&self) -> Result<(), Error> {
        if self.id.is_empty() {
            return Err(Error::Invalid("missing id".into()));
        }
        if self.ts.is_empty() {
            return Err(Error::Invalid("missing ts".into()));
        }
        validate_ts(&self.ts)?;
        if !valid_device(&self.dev) {
            return Err(Error::Invalid(format!("invalid device {:?}", self.dev)));
        }
        if !known_type(&self.event_type) {
            return Err(Error::Invalid(format!("unknown type {:?}", self.event_type)));
        }
        if self.body.is_null() {
            return Err(Error::Invalid("missing body".into()));
        }
        if !self.body.is_object() {
            return Err(Error::Invalid("body must be a JSON object".into()));
        }
        if self.event_type == TYPE_SUPERSEDE || self.event_type == TYPE_RETRACT {
            let target = self
                .body
                .get("target")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if target.is_empty() {
                return Err(Error::Invalid(format!(
                    "{} requires body.target",
                    self.event_type
                )));
            }
        }
        Ok(())
    }

    /// `YYYY-MM` partition for this event.
    pub fn month(&self) -> String {
        self.ts.get(..7).unwrap_or("").to_string()
    }

    /// Compact NDJSON line including the trailing newline.
    pub fn marshal_line(&self) -> Result<Vec<u8>, Error> {
        let mut buf = serde_json::to_vec(self)?;
        buf.push(b'\n');
        Ok(buf)
    }

    /// Sort before `other` by `(ts, id)`.
    pub fn less(&self, other: &Event) -> bool {
        if self.ts != other.ts {
            return self.ts < other.ts;
        }
        self.id < other.id
    }

    /// Content hash from `body.sha256` or `body.blob`.
    pub fn blob_sha256(&self) -> Option<String> {
        parse_blob_import(&self.body)
            .ok()
            .filter(|b| !b.sha256.is_empty())
            .map(|b| b.sha256)
    }

    /// `body.target` for supersede/retract events.
    pub fn target(&self) -> Option<String> {
        self.body
            .get("target")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }
}

/// Read a `blob_import` body. Written fields are `sha256`, `size`, `name`,
/// `kind`. Aliases `blob` and `orig_filename` are accepted when reading.
pub fn parse_blob_import(body: &serde_json::Value) -> Result<BlobImport, Error> {
    let obj = body
        .as_object()
        .ok_or_else(|| Error::Invalid("body must be a JSON object".into()))?;
    let sha256 = obj
        .get("sha256")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let blob = obj
        .get("blob")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let name = obj
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let orig = obj
        .get("orig_filename")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let kind = obj
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let size = obj.get("size").and_then(|v| v.as_i64()).unwrap_or(0);
    Ok(BlobImport {
        sha256: if sha256.is_empty() { blob } else { sha256 },
        size,
        name: if name.is_empty() { orig } else { name },
        kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn note(ts: &str) -> Event {
        Event::new(
            "01900000-0000-7000-8000-000000000001",
            ts,
            "manual",
            TYPE_NOTE,
            json!({"text": "ok"}),
        )
    }

    #[test]
    fn validate_utc() {
        let mut ev = note("2024-01-15T12:00:00.000000000Z");
        ev.validate().unwrap();
        ev.ts = "2024-01-15T12:00:00+00:00".into();
        assert!(ev.validate().is_err(), "accepted offset timestamp");
        ev.ts = "2024-01-15T12:00:00.000000000Z".into();
        ev.event_type = "not_a_type".into();
        assert!(ev.validate().is_err(), "accepted unknown type");
    }

    #[test]
    fn validate_ts_precision() {
        note("2024-01-15T12:00:00.000000000Z").validate().unwrap();
        for ts in ["2024-01-15T12:00:00Z", "2024-01-15T12:00:00.5Z"] {
            assert!(note(ts).validate().is_err(), "accepted short ts {ts}");
        }
    }

    #[test]
    fn device_rejects_traversal() {
        for name in ["", "..", "a/b", "a\\b", " foo"] {
            assert!(!valid_device(name), "accepted {name:?}");
        }
        assert!(valid_device("instinct-1"));
    }

    #[test]
    fn parse_blob_import_aliases() {
        let body = json!({
            "blob": "A70940623490FA4C251737CF74E1BF75A0327BB18766CC5620EDBDE3A985C96D",
            "orig_filename": "hello.txt"
        });
        let b = parse_blob_import(&body).unwrap();
        assert_eq!(
            b.sha256,
            "a70940623490fa4c251737cf74e1bf75a0327bb18766cc5620edbde3a985c96d"
        );
        assert_eq!(b.name, "hello.txt");
        let ev = Event::new(
            "id",
            "2024-01-15T12:00:00.000000000Z",
            "manual",
            TYPE_BLOB_IMPORT,
            body,
        );
        assert_eq!(ev.blob_sha256().as_deref(), Some(b.sha256.as_str()));
    }

    #[test]
    fn marshal_field_order() {
        let ev = note("2024-01-15T12:00:00.000000000Z");
        let line = ev.marshal_line().unwrap();
        let want = b"{\"id\":\"01900000-0000-7000-8000-000000000001\",\"ts\":\"2024-01-15T12:00:00.000000000Z\",\"dev\":\"manual\",\"type\":\"note\",\"body\":{\"text\":\"ok\"}}\n";
        assert_eq!(line, want);
    }

    #[test]
    fn gallery_types_are_known() {
        for t in [
            TYPE_PERSON_HIDDEN,
            TYPE_PERSON_UNHIDDEN,
            TYPE_PERSON_FEATURED,
            TYPE_PERSON_UNFEATURED,
            TYPE_FEATURED_PHOTO_SET,
            TYPE_FEATURED_PHOTO_CLEAR,
            TYPE_PERSON_ME_SET,
            TYPE_PERSON_ME_CLEAR,
            TYPE_PERSON_RENAMED,
            TYPE_PERSON_CONTACT_LINK_SET,
            TYPE_PERSON_CONTACT_LINK_CLEAR,
        ] {
            assert!(known_type(t), "{t}");
        }
    }

    #[test]
    fn now_utc_is_canonical() {
        let ts = now_utc();
        validate_ts(&ts).expect(&ts);
    }
}
