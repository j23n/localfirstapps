//! Projection knobs and source ranks. Not part of the event log.
//!
//! A rebuild depends only on the archive root and the binary: `<root>/config/`
//! overrides the embedded defaults. The process working directory is never
//! consulted.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

const RANK_UNLISTED: i32 = 100;
const EMBEDDED_PROJECTION: &str = include_str!("../config/projection.toml");
const EMBEDDED_SOURCES: &str = include_str!("../config/sources.toml");

/// Loaded archive configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub pause_threshold: Duration,
    pub source_ranks: HashMap<String, i32>,
}

impl Default for Config {
    fn default() -> Self {
        Self::from_texts(EMBEDDED_PROJECTION, EMBEDDED_SOURCES)
    }
}

impl Config {
    /// Load `<root>/config/*.toml` when present, otherwise the embedded files.
    pub fn load(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        let projection = read_or_embedded(root.join("config/projection.toml"), EMBEDDED_PROJECTION);
        let sources = read_or_embedded(root.join("config/sources.toml"), EMBEDDED_SOURCES);
        Self::from_texts(&projection, &sources)
    }

    fn from_texts(projection: &str, sources: &str) -> Self {
        Self {
            pause_threshold: Duration::from_secs(parse_pause_threshold(projection, 60) as u64),
            source_ranks: parse_sources(sources),
        }
    }

    /// Configured rank, or 100 if the source is unlisted.
    pub fn source_rank(&self, name: &str) -> i32 {
        self.source_ranks
            .get(name)
            .copied()
            .unwrap_or(RANK_UNLISTED)
    }
}

fn read_or_embedded(path: impl AsRef<Path>, embedded: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|_| embedded.to_string())
}

fn parse_pause_threshold(text: &str, fallback: i32) -> i32 {
    for line in toml_lines(text) {
        let Some((key, value)) = split_kv(&line) else {
            continue;
        };
        if key != "pause_threshold_s" {
            continue;
        }
        if let Ok(n) = value.parse::<i32>() {
            if n > 0 {
                return n;
            }
        }
    }
    fallback
}

fn parse_sources(text: &str) -> HashMap<String, i32> {
    let mut out = HashMap::new();
    let mut in_sources = false;
    for line in toml_lines(text) {
        if line.starts_with('[') && line.ends_with(']') {
            in_sources = line[1..line.len() - 1].trim() == "sources";
            continue;
        }
        if !in_sources {
            continue;
        }
        let Some((key, value)) = split_kv(&line) else {
            continue;
        };
        if let Ok(n) = value.parse::<i32>() {
            out.insert(key, n);
        }
    }
    out
}

fn toml_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(strip_inline_comment)
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

fn strip_inline_comment(line: &str) -> String {
    let mut in_quote = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => in_quote = !in_quote,
            '#' if !in_quote => return line[..i].trim_end().to_string(),
            _ => {}
        }
    }
    line.to_string()
}

fn split_kv(line: &str) -> Option<(String, String)> {
    let i = line.find('=')?;
    let key = line[..i].trim().trim_matches('"').to_string();
    let value = line[i + 1..].trim().trim_matches('"').to_string();
    if key.is_empty() {
        None
    } else {
        Some((key, value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_defaults_rank_watch_above_phone() {
        let cfg = Config::default();
        assert_eq!(cfg.source_rank("Apple Watch"), 10);
        assert_eq!(cfg.source_rank("iPhone"), 20);
        assert_eq!(cfg.source_rank("unknown"), 100);
        assert_eq!(cfg.pause_threshold, Duration::from_secs(60));
    }
}
