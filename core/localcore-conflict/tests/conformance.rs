//! Fixture-pinned grammar and grouping (ADR 0005 R7).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use localcore_conflict::{
    groups, is_conflict_name, parse_name, parse_path, ConflictCopy, ConflictGroup,
};
use serde::Deserialize;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn fixture_text(rel: &str) -> String {
    let path = fixtures_dir().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn grammar_lines(rel: &str) -> Vec<String> {
    fixture_text(rel)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

fn tree_dir(name: &str) -> PathBuf {
    fixtures_dir().join("trees").join(name)
}

fn walk_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk_files_into(root, &mut out);
    out
}

fn walk_files_into(dir: &Path, out: &mut Vec<String>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .map(|e| e.expect("dirent"))
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            walk_files_into(&path, out);
        } else {
            out.push(path.to_string_lossy().into_owned());
        }
    }
}

fn relativize(groups: Vec<ConflictGroup>, root: &Path) -> Vec<ConflictGroup> {
    groups
        .into_iter()
        .map(|g| ConflictGroup {
            canonical_name: g.canonical_name,
            dir: strip_root(&g.dir, root),
            copies: g
                .copies
                .into_iter()
                .map(|c| ConflictCopy {
                    path: strip_root(&c.path, root),
                    ..c
                })
                .collect(),
        })
        .collect()
}

fn strip_root(path: &str, root: &Path) -> String {
    Path::new(path)
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_owned())
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct ExpectedCopy {
    path: String,
    name: String,
    base_stem: String,
    extension: String,
    date: String,
    time: String,
    origin_device: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct ExpectedGroup {
    canonical_name: String,
    dir: String,
    copies: Vec<ExpectedCopy>,
}

fn as_expected(groups: &[ConflictGroup]) -> Vec<ExpectedGroup> {
    groups
        .iter()
        .map(|g| ExpectedGroup {
            canonical_name: g.canonical_name.clone(),
            dir: g.dir.clone(),
            copies: g
                .copies
                .iter()
                .map(|c| ExpectedCopy {
                    path: c.path.clone(),
                    name: c.name.clone(),
                    base_stem: c.base_stem.clone(),
                    extension: c.extension.clone(),
                    date: c.date.clone(),
                    time: c.time.clone(),
                    origin_device: c.origin_device.clone(),
                })
                .collect(),
        })
        .collect()
}

fn expected_by_tree() -> BTreeMap<String, Vec<ExpectedGroup>> {
    serde_json::from_str(&fixture_text("expected/groups.json")).expect("groups.json")
}

fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut state = seed;
    for i in (1..items.len()).rev() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let j = (state as usize) % (i + 1);
        items.swap(i, j);
    }
}

#[test]
fn every_valid_grammar_line_parses() {
    let lines = grammar_lines("grammar/valid.txt");
    assert!(!lines.is_empty(), "valid.txt must pin at least one name");
    for name in &lines {
        let copy = parse_name(name).unwrap_or_else(|| panic!("valid line did not parse: {name}"));
        assert_eq!(copy.path, *name);
        assert_eq!(copy.name, *name);
        assert!(is_conflict_name(name), "{name}");
        assert!(!copy.base_stem.is_empty(), "{name}");
        assert!(!copy.extension.is_empty(), "{name}");
        assert_eq!(copy.date.len(), 8, "{name}");
        assert_eq!(copy.time.len(), 6, "{name}");
    }
}

#[test]
fn every_invalid_grammar_line_is_rejected() {
    let lines = grammar_lines("grammar/invalid.txt");
    assert!(!lines.is_empty(), "invalid.txt must pin at least one name");
    for name in &lines {
        assert!(
            parse_name(name).is_none(),
            "invalid line parsed as conflict: {name}"
        );
        assert!(!is_conflict_name(name), "{name}");
    }
}

#[test]
fn tree_walk_groups_match_expected() {
    let expected = expected_by_tree();
    assert_eq!(
        expected.keys().collect::<Vec<_>>(),
        ["contacts-minimal", "gallery-minimal", "music-minimal"]
    );
    for (tree, want) in &expected {
        let root = tree_dir(tree);
        let paths = walk_files(&root);
        assert!(
            paths
                .iter()
                .any(|p| !is_conflict_name(Path::new(p).file_name().unwrap().to_str().unwrap())),
            "{tree} must include a surviving original"
        );
        let got = relativize(groups(paths.iter().map(String::as_str)), &root);
        assert_eq!(as_expected(&got), *want, "{tree}");
    }
}

#[test]
fn groups_are_independent_of_walk_order() {
    for tree in ["gallery-minimal", "contacts-minimal", "music-minimal"] {
        let root = tree_dir(tree);
        let mut paths = walk_files(&root);
        let baseline = groups(paths.iter().map(String::as_str));
        shuffle(&mut paths, 0xC0FF_EE42);
        let shuffled = groups(paths.iter().map(String::as_str));
        assert_eq!(baseline, shuffled, "{tree}");
        paths.reverse();
        let reversed = groups(paths.iter().map(String::as_str));
        assert_eq!(baseline, reversed, "{tree} reversed");
        let ids: Vec<_> = baseline.iter().map(ConflictGroup::id).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "{tree} groups must be sorted by id");
    }
}

#[test]
fn health_log_month_file_is_not_a_conflict() {
    assert!(parse_path("log/dev/2026-09.ndjson").is_none());
    assert!(parse_name("2026-09.ndjson").is_none());
    assert!(!is_conflict_name("2026-09.ndjson"));
    assert!(!is_conflict_name("log/dev/2026-09.ndjson"));
}
