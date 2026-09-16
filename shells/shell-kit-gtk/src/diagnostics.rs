//! Process-local, bounded diagnostics shared by GTK shells.
//!
//! This is shell state, never a synced domain-operation log. Callers are
//! responsible for recording redacted messages rather than user content.

use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub sequence: u64,
    pub timestamp: SystemTime,
    pub level: LogLevel,
    pub category: String,
    pub message: String,
}

impl LogEntry {
    /// UTC time of day without using a locale service.
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

/// Newest bounded diagnostics. Nothing is persisted, synced, or uploaded.
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
    fn bounded_store_keeps_newest_and_filters_without_persistence() {
        let mut store = LogStore::new(2);
        store.record(LogLevel::Info, "app", "first");
        store.record(LogLevel::Warning, "folder", "second");
        store.record(LogLevel::Error, "folder", "third");

        let entries = store.filtered("", None);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].message, "second");
        assert_eq!(entries[1].message, "third");
        assert_eq!(store.filtered("FOLDER", Some(LogLevel::Error)).len(), 1);
        store.clear();
        assert!(store.is_empty());
    }

    #[test]
    fn zero_capacity_discards_entries() {
        let mut store = LogStore::new(0);
        store.record(LogLevel::Info, "app", "discarded");
        assert!(store.is_empty());
    }
}
