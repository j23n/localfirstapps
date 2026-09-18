//! Process-local debug traces shared by every LocalFiles app.
//!
//! This is **not** [`localcore_log`] (append-only user-data NDJSON) and not
//! the GTK Diagnostics `LogStore`. Cores emit; binaries call [`init`].
//!
//! ```text
//! LOCALFILES_DEBUG=1     summaries + spans ≥5ms
//! LOCALFILES_DEBUG=2     every event
//! RUST_LOG=lf=debug      raw EnvFilter (wins)
//! LOCALFILES_LOG=lf=info raw EnvFilter (wins if RUST_LOG is unset)
//! ```
//!
//! Events use `target: "lf"` so one filter covers every crate. `[lf main]`
//! is the thread that called [`init`] (usually the UI loop). `[lf work]`
//! is every other thread.
//!
//! Each line is stamped `HH:MM:SS.mmmZ +S.sss` — UTC wall clock plus seconds
//! since [`init`] (or the first printed line if a test never called it).

#![forbid(unsafe_code)]

use std::cell::Cell;
use std::fmt::Display;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub use tracing::{debug, debug_span, info, info_span, warn};

#[cfg(feature = "subscriber")]
mod subscriber;

#[cfg(feature = "subscriber")]
pub use subscriber::{init, Init, Lane};

thread_local! {
    static MAIN: Cell<bool> = const { Cell::new(false) };
}

static ORIGIN: OnceLock<Instant> = OnceLock::new();

/// Mark this thread as `[lf main]`. [`init`] calls this; tests may too.
pub fn mark_main_thread() {
    MAIN.with(|flag| flag.set(true));
}

/// Pin the `+S.sss` origin. [`init`] calls this; the first stamp also does.
pub fn mark_origin() {
    let _ = ORIGIN.get_or_init(Instant::now);
}

/// `main` on the thread that called [`mark_main_thread`], otherwise `work`.
#[must_use]
pub fn thread_kind() -> &'static str {
    if MAIN.with(Cell::get) {
        "main"
    } else {
        "work"
    }
}

/// `0` off, `1` summaries, `2` every event.
#[must_use]
pub fn level() -> u8 {
    level_from(
        std::env::var("LOCALFILES_DEBUG").ok().as_deref(),
        std::env::var("LOCALGALLERY_DEBUG").ok().as_deref(),
    )
}

/// Parse `LOCALFILES_DEBUG`, falling back to the Gallery alias.
#[must_use]
pub fn level_from(files: Option<&str>, gallery_alias: Option<&str>) -> u8 {
    parse_debug_level(files)
        .or_else(|| parse_debug_level(gallery_alias))
        .unwrap_or(0)
}

fn parse_debug_level(raw: Option<&str>) -> Option<u8> {
    match raw.unwrap_or("").to_ascii_lowercase().as_str() {
        "" => None,
        "2" | "verbose" | "trace" => Some(2),
        "1" | "true" | "yes" | "on" | "debug" => Some(1),
        "0" | "false" | "no" | "off" => Some(0),
        _ => Some(1),
    }
}

#[must_use]
pub fn enabled() -> bool {
    level() > 0 || env_nonempty("RUST_LOG") || env_nonempty("LOCALFILES_LOG")
}

#[must_use]
pub fn verbose() -> bool {
    level() >= 2
}

fn env_nonempty(key: &str) -> bool {
    std::env::var(key).is_ok_and(|value| !value.is_empty())
}

/// EnvFilter directive. `RUST_LOG` / `LOCALFILES_LOG` win; else `lf=info|debug`.
#[must_use]
pub fn filter_directive() -> String {
    filter_from(
        std::env::var("RUST_LOG").ok().as_deref(),
        std::env::var("LOCALFILES_LOG").ok().as_deref(),
        level(),
    )
}

/// Build the filter string from explicit pieces (tests).
#[must_use]
pub fn filter_from(rust_log: Option<&str>, files_log: Option<&str>, debug: u8) -> String {
    if let Some(value) = rust_log.filter(|value| !value.is_empty()) {
        return value.to_string();
    }
    if let Some(value) = files_log.filter(|value| !value.is_empty()) {
        return value.to_string();
    }
    match debug {
        2 => "lf=debug".into(),
        1 => "lf=info".into(),
        _ => "off".into(),
    }
}

/// One-line event at info (`LOCALFILES_DEBUG=1`).
pub fn event(area: &str, message: impl Display) {
    let message = message.to_string();
    tracing::info!(target: "lf", area, "{message}");
    if !subscriber_installed() && enabled() {
        eprint_event(area, &message);
    }
}

/// Verbose-only event (`LOCALFILES_DEBUG=2`).
pub fn detail(area: &str, message: impl Display) {
    let message = message.to_string();
    tracing::debug!(target: "lf", area, "{message}");
    if !subscriber_installed() && verbose() {
        eprint_event(area, &message);
    }
}

/// Start a timed span. At info, closes ≥5 ms are printed.
#[must_use]
pub fn span(area: &'static str, label: impl Display) -> Span {
    span_inner(area, label, false)
}

/// Like [`span`] but the close is always printed at info.
#[must_use]
pub fn span_always(area: &'static str, label: impl Display) -> Span {
    span_inner(area, label, true)
}

fn span_inner(area: &'static str, label: impl Display, always: bool) -> Span {
    let label = label.to_string();
    let span = tracing::info_span!(
        target: "lf",
        "span",
        area,
        label = label.as_str(),
        extras = tracing::field::Empty,
        always,
    );
    let enter = span.clone().entered();
    Span {
        span,
        extras: String::new(),
        start: Instant::now(),
        always,
        area,
        label,
        _enter: enter,
    }
}

/// RAII span. Extra fields are recorded on the live `tracing` span.
pub struct Span {
    span: tracing::Span,
    extras: String,
    start: Instant,
    always: bool,
    area: &'static str,
    label: String,
    _enter: tracing::span::EnteredSpan,
}

impl Span {
    #[must_use]
    pub fn extra(mut self, key: &str, value: impl Display) -> Self {
        if !self.extras.is_empty() {
            self.extras.push(' ');
        }
        self.extras.push_str(key);
        self.extras.push('=');
        self.extras.push_str(&value.to_string());
        self.span.record("extras", self.extras.as_str());
        self
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        // With `subscriber`, the layer prints the close. Without one this
        // is the only line, so we keep the ≥5ms rule here too.
        if subscriber_installed() {
            return;
        }
        if !enabled() {
            return;
        }
        let ms = self.start.elapsed().as_secs_f64() * 1000.0;
        if !self.always && !verbose() && ms < 5.0 {
            return;
        }
        eprint_span(self.area, &self.label, &self.extras, ms);
    }
}

pub(crate) fn eprint_span(area: &str, label: &str, extras: &str, ms: f64) {
    if extras.is_empty() {
        eprintln!("{} {:<12} {ms:8.1}ms  {label}", line_prefix(), area);
    } else {
        eprintln!(
            "{} {:<12} {ms:8.1}ms  {label}  {extras}",
            line_prefix(),
            area
        );
    }
}

pub(crate) fn eprint_event(area: &str, message: &str) {
    eprintln!("{} {area:<12} {message}", line_prefix());
}

fn line_prefix() -> String {
    format!("[lf {:>4}] {}", thread_kind(), format_stamp())
}

/// `09:39:02.147Z +1.234s` — UTC, then elapsed since [`mark_origin`].
#[must_use]
pub fn format_stamp() -> String {
    mark_origin();
    let elapsed = ORIGIN
        .get()
        .map(|start| start.elapsed().as_secs_f64())
        .unwrap_or(0.0);
    format!("{} +{elapsed:.3}s", format_utc_clock())
}

fn format_utc_clock() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let tod = now.as_secs() % 86_400;
    let h = tod / 3600;
    let m = (tod % 3600) / 60;
    let s = tod % 60;
    format!("{h:02}:{m:02}:{s:02}.{:03}Z", now.subsec_millis())
}

fn subscriber_installed() -> bool {
    tracing::dispatcher::has_been_set()
}

#[must_use]
pub fn fmt_ms(elapsed: Duration) -> String {
    format!("{:.1}ms", elapsed.as_secs_f64() * 1000.0)
}

/// One lane line printed by [`init`] / [`banner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneDesc {
    /// `FOREGROUND`, `BACKGROUND`, or `IDLE`.
    pub kind: &'static str,
    /// What runs on that lane.
    pub detail: &'static str,
}

/// Print the filter + lane map. [`init`] calls this after installing.
pub fn banner(app: &str, lanes: &[LaneDesc]) {
    if !enabled() {
        return;
    }
    eprintln!(
        "{} trace        app={app} LOCALFILES_DEBUG={} filter={}  [main]=init thread  [work]=other",
        line_prefix(),
        level(),
        filter_directive()
    );
    for lane in lanes {
        eprintln!(
            "{} lanes        {} {}",
            line_prefix(),
            lane.kind,
            lane.detail
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_level_parses() {
        assert_eq!(level_from(Some("2"), None), 2);
        assert_eq!(level_from(Some("verbose"), None), 2);
        assert_eq!(level_from(Some("1"), None), 1);
        assert_eq!(level_from(None, Some("true")), 1);
        assert_eq!(level_from(None, None), 0);
        assert_eq!(level_from(Some("0"), Some("2")), 0);
    }

    #[test]
    fn rust_log_wins_filter() {
        assert_eq!(
            filter_from(Some("lf=debug"), Some("lf=info"), 1),
            "lf=debug"
        );
        assert_eq!(filter_from(None, Some("lf=info"), 2), "lf=info");
        assert_eq!(filter_from(None, None, 1), "lf=info");
        assert_eq!(filter_from(None, None, 2), "lf=debug");
        assert_eq!(filter_from(None, None, 0), "off");
    }

    #[test]
    fn span_drop_does_not_panic_when_disabled() {
        let _span = span("test", "noop").extra("n", 0);
    }

    #[test]
    fn thread_kind_is_work_until_marked() {
        assert_eq!(thread_kind(), "work");
        mark_main_thread();
        assert_eq!(thread_kind(), "main");
    }

    #[test]
    fn stamp_is_utc_clock_and_elapsed() {
        mark_origin();
        let stamp = format_stamp();
        let (clock, elapsed) = stamp.split_once(" +").expect("clock + elapsed");
        assert!(
            clock.len() == 13 && clock.ends_with('Z') && clock.as_bytes()[2] == b':',
            "{clock}"
        );
        assert!(elapsed.ends_with('s'), "{elapsed}");
        let secs: f64 = elapsed.trim_end_matches('s').parse().expect("elapsed");
        assert!(secs >= 0.0, "{secs}");
    }
}
