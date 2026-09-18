//! Append-only log record.
//!
//! Ported from health `internal/event`, with gallery person-state types added
//! so one schema serves both consumers (ADR 0005 R13/R14).

use serde::{Deserialize, Serialize};
use serde_json::ser::{CharEscape, CompactFormatter, Formatter, Serializer};
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
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
pub const TYPE_EPISODE: &str = "episode";

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
/// Written last by UserDefaults → log migrate. Absence means migrate may retry.
pub const TYPE_PERSON_MIGRATED: &str = "person_migrated";

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

/// Whether `t` is a legal type token. Shape only: `[a-z][a-z0-9_]*`.
///
/// The envelope does not own a monorepo enum of types (Phase 3.4).
/// [`known_type`] lists the health and gallery types this crate's helpers
/// understand; apps may append any token that passes here.
pub fn valid_type(t: &str) -> bool {
    let mut chars = t.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Whether `t` is a health or gallery type this crate documents.
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
            | TYPE_EPISODE
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
            | TYPE_PERSON_MIGRATED
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

/// Replace the 9-digit fractional-second field of a canonical `ts`.
pub fn ts_with_nanos(ts: &str, nanos: u32) -> Result<String, Error> {
    validate_ts(ts)?;
    if nanos >= 1_000_000_000 {
        return Err(Error::Invalid("nanos must be < 1e9".into()));
    }
    let mut out = ts.to_string();
    out.replace_range(20..29, &format!("{nanos:09}"));
    Ok(out)
}

static EVENT_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Time-ordered UUIDv7 (8-4-4-4-12 hex), matching health `internal/uuid`.
pub fn new_event_id() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let ms = dur.as_millis() as u64;
    let n = EVENT_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mix = n
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(dur.subsec_nanos() as u64);
    let mut b = [0u8; 16];
    b[0] = (ms >> 40) as u8;
    b[1] = (ms >> 32) as u8;
    b[2] = (ms >> 24) as u8;
    b[3] = (ms >> 16) as u8;
    b[4] = (ms >> 8) as u8;
    b[5] = ms as u8;
    b[6] = ((mix >> 48) as u8 & 0x0f) | 0x70;
    b[7] = (mix >> 40) as u8;
    b[8] = ((mix >> 32) as u8 & 0x3f) | 0x80;
    b[9] = (mix >> 24) as u8;
    b[10] = (mix >> 16) as u8;
    b[11] = (mix >> 8) as u8;
    b[12] = mix as u8;
    b[13] = (n >> 16) as u8;
    b[14] = (n >> 8) as u8;
    b[15] = n as u8;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13],
        b[14], b[15]
    )
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
    let rebuilt =
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{nanos:09}Z");
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

    /// A new event with a UUIDv7 id and canonical UTC timestamp.
    pub fn fresh(
        dev: impl Into<String>,
        event_type: impl Into<String>,
        body: serde_json::Value,
    ) -> Self {
        Self::new(new_event_id(), now_utc(), dev, event_type, body)
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
        if !valid_type(&self.event_type) {
            return Err(Error::Invalid(format!(
                "invalid type {:?}",
                self.event_type
            )));
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
    ///
    /// Matches Go `json.Encoder` with `SetEscapeHTML(false)`: `<`, `>`, `&`
    /// stay literal so health notes are the same bytes on both sides.
    pub fn marshal_line(&self) -> Result<Vec<u8>, Error> {
        let mut buf = Vec::new();
        let mut ser = Serializer::with_formatter(&mut buf, NoHtmlEscape);
        self.serialize(&mut ser)?;
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

/// Compact JSON, but `<` `>` `&` are not `\u00xx`-escaped (Go `SetEscapeHTML(false)`).
struct NoHtmlEscape;

impl Formatter for NoHtmlEscape {
    fn write_char_escape<W>(&mut self, writer: &mut W, char_escape: CharEscape) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        match char_escape {
            CharEscape::AsciiControl(b @ (b'<' | b'>' | b'&')) => writer.write_all(&[b]),
            other => CompactFormatter.write_char_escape(writer, other),
        }
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
        ev.event_type = "Not-A-Type".into();
        assert!(ev.validate().is_err(), "accepted invalid type shape");
        ev.event_type = "contact_saved".into();
        ev.validate().unwrap();
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
    fn marshal_does_not_html_escape() {
        let ev = Event::new(
            "01900000-0000-7000-8000-000000000001",
            "2024-01-15T12:00:00.000000000Z",
            "manual",
            TYPE_NOTE,
            json!({"text": "a<b>&c"}),
        );
        let line = ev.marshal_line().unwrap();
        let s = String::from_utf8(line).unwrap();
        assert!(s.contains("a<b>&c"), "{s}");
        assert!(!s.contains("\\u003c"), "{s}");
    }

    #[test]
    fn type_shape_is_open() {
        assert!(valid_type("contact_saved"));
        assert!(valid_type("a"));
        assert!(!valid_type(""));
        assert!(!valid_type("Note"));
        assert!(!valid_type("bad-type"));
        assert!(!known_type("contact_saved"));
        assert!(known_type(TYPE_EPISODE));
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
            TYPE_PERSON_MIGRATED,
        ] {
            assert!(known_type(t), "{t}");
        }
    }

    #[test]
    fn now_utc_is_canonical() {
        let ts = now_utc();
        validate_ts(&ts).expect(&ts);
    }

    #[test]
    fn new_event_id_is_version_7() {
        let id = new_event_id();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "7");
        let second = new_event_id();
        assert_ne!(id, second);
    }

    #[test]
    fn ts_with_nanos_rewrites_fraction() {
        let ts = ts_with_nanos("2024-07-01T10:00:00.000000000Z", 42).unwrap();
        assert_eq!(ts, "2024-07-01T10:00:00.000000042Z");
        validate_ts(&ts).unwrap();
    }
}
