//! Process-local diagnostics for the GTK shell.
//!
//! This bounded in-memory store is deliberately separate from the selected
//! contacts folder and its `.contacts/log` domain-operation log.

use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

/// Severity shown by the app diagnostics screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

impl LogLevel {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warning => "WARNING",
            Self::Error => "ERROR",
        }
    }
}

/// One redacted shell diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub sequence: u64,
    pub timestamp: SystemTime,
    pub level: LogLevel,
    pub category: String,
    pub message: String,
}

impl LogEntry {
    /// UTC time of day without bringing a locale service into the shell.
    #[must_use]
    pub fn time_label(&self) -> String {
        let millis = self
            .timestamp
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let day_millis = millis % 86_400_000;
        let hours = day_millis / 3_600_000;
        let minutes = (day_millis / 60_000) % 60;
        let seconds = (day_millis / 1_000) % 60;
        let fraction = day_millis % 1_000;
        format!("{hours:02}:{minutes:02}:{seconds:02}.{fraction:03}")
    }
}

/// Newest bounded shell diagnostics. Nothing is persisted or synced.
#[derive(Debug)]
pub struct LogStore {
    capacity: usize,
    next_sequence: u64,
    entries: VecDeque<LogEntry>,
}

impl LogStore {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            next_sequence: 0,
            entries: VecDeque::with_capacity(capacity),
        }
    }

    pub fn record(
        &mut self,
        level: LogLevel,
        category: impl Into<String>,
        message: impl Into<String>,
    ) {
        let entry = LogEntry {
            sequence: self.next_sequence,
            timestamp: SystemTime::now(),
            level,
            category: category.into(),
            message: message.into(),
        };
        self.next_sequence = self.next_sequence.wrapping_add(1);
        if self.capacity == 0 {
            return;
        }
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    #[must_use]
    pub fn filtered(&self, query: &str, level: Option<LogLevel>) -> Vec<&LogEntry> {
        let needle = query.trim().to_lowercase();
        self.entries
            .iter()
            .filter(|entry| level.is_none_or(|wanted| entry.level == wanted))
            .filter(|entry| {
                needle.is_empty()
                    || entry.message.to_lowercase().contains(&needle)
                    || entry.category.to_lowercase().contains(&needle)
                    || entry.level.label().to_lowercase().contains(&needle)
            })
            .collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_store_keeps_the_newest_entries() {
        let mut store = LogStore::new(2);
        store.record(LogLevel::Info, "app", "first");
        store.record(LogLevel::Warning, "folder", "second");
        store.record(LogLevel::Error, "folder", "third");

        let entries = store.filtered("", None);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].message, "second");
        assert_eq!(entries[1].message, "third");
        assert_eq!(entries[0].sequence, 1);
        assert_eq!(entries[1].sequence, 2);
    }

    #[test]
    fn zero_capacity_store_stays_empty() {
        let mut store = LogStore::new(0);
        store.record(LogLevel::Info, "app", "discarded");
        assert!(store.is_empty());
    }

    #[test]
    fn filters_level_category_and_message_case_insensitively() {
        let mut store = LogStore::new(8);
        store.record(LogLevel::Info, "app", "Application started");
        store.record(LogLevel::Warning, "Folder", "Saved folder unavailable");
        store.record(LogLevel::Error, "contact", "Save failed");

        assert_eq!(store.filtered("FOLDER", None).len(), 1);
        assert_eq!(store.filtered("save", Some(LogLevel::Warning)).len(), 1);
        assert_eq!(store.filtered("save", Some(LogLevel::Error)).len(), 1);
        assert!(store.filtered("started", Some(LogLevel::Error)).is_empty());
        assert_eq!(store.filtered("warning", None).len(), 1);
    }

    #[test]
    fn clear_does_not_change_the_bound() {
        let mut store = LogStore::new(1);
        store.record(LogLevel::Info, "app", "first");
        store.clear();
        store.record(LogLevel::Info, "app", "second");
        store.record(LogLevel::Info, "app", "third");
        assert_eq!(store.len(), 1);
        assert_eq!(store.filtered("", None)[0].message, "third");
    }
}
