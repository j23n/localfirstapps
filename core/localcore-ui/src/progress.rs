//! Display-ready progress for chrome `progress` and Settings `progress-row`.
//!
//! Shells render this record natively. Timing policy lives here so GTK and
//! iOS cannot drift (ADR 0007 R5).

use std::time::{Duration, Instant};

/// Chrome progress stays hidden until the work has lasted this long.
pub const REVEAL_AFTER: Duration = Duration::from_millis(500);

/// ETA text stays off until this much throughput has been observed.
pub const ETA_AFTER: Duration = Duration::from_secs(1);

/// Values one `progress-row` / chrome `progress` binding can show.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressDisplay {
    /// Phase word (`Scanning`, `Tagging`, `Reloading`).
    pub label: String,
    /// Optional count / ETA, already formatted.
    pub detail: Option<String>,
    /// `Some` when a total is known. `None` is indeterminate.
    pub fraction: Option<f64>,
    /// Whether Cancel is offered.
    pub cancel: bool,
}

/// One in-flight long job. `started` is fixed for the job's life.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkProgress {
    started: Instant,
    display: ProgressDisplay,
}

impl WorkProgress {
    /// Start a job. Chrome stays hidden until [`REVEAL_AFTER`].
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            started: Instant::now(),
            display: ProgressDisplay {
                label: label.into(),
                detail: None,
                fraction: None,
                cancel: true,
            },
        }
    }

    /// Start at an explicit instant (tests).
    #[must_use]
    pub fn started_at(label: impl Into<String>, started: Instant) -> Self {
        let mut progress = Self::new(label);
        progress.started = started;
        progress
    }

    /// Offer or hide Cancel.
    #[must_use]
    pub fn with_cancel(mut self, cancel: bool) -> Self {
        self.display.cancel = cancel;
        self
    }

    /// Instant this job began.
    #[must_use]
    pub fn started(&self) -> Instant {
        self.started
    }

    /// Latest display values, including before the chrome reveal.
    #[must_use]
    pub fn display(&self) -> &ProgressDisplay {
        &self.display
    }

    /// Update the live readout. Does not reset `started`.
    pub fn update(
        &mut self,
        label: impl Into<String>,
        detail: Option<String>,
        fraction: Option<f64>,
    ) {
        self.display.label = label.into();
        self.display.detail = detail;
        self.display.fraction = fraction;
    }

    /// True when chrome may show this job.
    #[must_use]
    pub fn revealed(&self, now: Instant) -> bool {
        now.duration_since(self.started) >= REVEAL_AFTER
    }

    /// Chrome binding input. `None` before [`REVEAL_AFTER`].
    #[must_use]
    pub fn chrome(&self, now: Instant) -> Option<&ProgressDisplay> {
        self.revealed(now).then_some(&self.display)
    }
}

/// `"4,821 found"` while a walk has no total.
#[must_use]
pub fn found_detail(processed: u64) -> String {
    format!("{} found", format_count(processed))
}

/// `"16 / 18,220"` then `"16 / 18,220 · ~8:40"` after [`ETA_AFTER`].
#[must_use]
pub fn count_detail(processed: u64, total: u64, started: Instant, now: Instant) -> String {
    if total == 0 {
        return format_count(processed);
    }
    let counts = format!("{} / {}", format_count(processed), format_count(total));
    let elapsed = now.duration_since(started);
    if processed == 0 || elapsed <= ETA_AFTER {
        return counts;
    }
    let remaining = total.saturating_sub(processed);
    let secs = ((remaining as f64) / (processed as f64 / elapsed.as_secs_f64()))
        .round()
        .max(0.0) as u64;
    if secs == 0 {
        return counts;
    }
    format!("{counts} · {}", format_remaining(secs))
}

/// Group an integer with commas (`4821` → `4,821`).
#[must_use]
pub fn format_count(value: u64) -> String {
    let raw = value.to_string();
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len() + raw.len() / 3);
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 && (bytes.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(char::from(*byte));
    }
    out
}

/// `~45s`, `~3:20`, or `~1h 12m`.
#[must_use]
pub fn format_remaining(seconds: u64) -> String {
    if seconds >= 3600 {
        let hours = seconds / 3600;
        let minutes = (seconds % 3600) / 60;
        if minutes > 0 {
            format!("~{hours}h {minutes}m")
        } else {
            format!("~{hours}h")
        }
    } else if seconds >= 60 {
        format!("~{}:{:02}", seconds / 60, seconds % 60)
    } else {
        format!("~{seconds}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_stays_hidden_before_reveal() {
        let started = Instant::now();
        let progress = WorkProgress::started_at("Scanning", started);
        assert!(progress.chrome(started).is_none());
        assert!(progress
            .chrome(started + REVEAL_AFTER - Duration::from_millis(1))
            .is_none());
        let shown = progress.chrome(started + REVEAL_AFTER).expect("revealed");
        assert_eq!(shown.label, "Scanning");
    }

    #[test]
    fn settings_display_is_immediate() {
        let progress = WorkProgress::new("Tagging");
        assert_eq!(progress.display().label, "Tagging");
        assert!(progress.display().cancel);
    }

    #[test]
    fn count_detail_waits_for_throughput() {
        let started = Instant::now();
        assert_eq!(
            count_detail(16, 100, started, started + Duration::from_millis(400)),
            "16 / 100"
        );
        let later = started + Duration::from_secs(2);
        let text = count_detail(16, 100, started, later);
        assert!(text.starts_with("16 / 100 · ~"), "{text}");
    }

    #[test]
    fn found_detail_groups_thousands() {
        assert_eq!(found_detail(4821), "4,821 found");
        assert_eq!(format_count(18_220), "18,220");
        assert_eq!(format_remaining(45), "~45s");
        assert_eq!(format_remaining(200), "~3:20");
        assert_eq!(format_remaining(4320), "~1h 12m");
    }
}
