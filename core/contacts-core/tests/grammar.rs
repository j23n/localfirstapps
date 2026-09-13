//! Grammar is one source of truth: `localcore-conflict` fixtures.

use contacts_core::is_conflict_name;

fn grammar_lines(file: &str) -> Vec<String> {
    let path = format!(
        "{}/../localcore-conflict/fixtures/grammar/{file}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {path}: {e}"))
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

#[test]
fn every_valid_grammar_line_is_a_conflict_name() {
    let names = grammar_lines("valid.txt");
    assert!(!names.is_empty());
    for name in names {
        assert!(is_conflict_name(&name), "expected conflict: {name}");
    }
}

#[test]
fn every_invalid_grammar_line_is_rejected() {
    let names = grammar_lines("invalid.txt");
    assert!(!names.is_empty());
    for name in names {
        assert!(!is_conflict_name(&name), "expected survivor: {name}");
    }
}
