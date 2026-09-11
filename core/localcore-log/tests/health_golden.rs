//! Health m0 golden: the Rust fixture copy cannot drift from Go.
//!
//! Health `internal/log` remains the writer until Phase 6. This file is the
//! port contract. The Go tree is `../../../apps/health/testdata/m0` from
//! this tests/ directory.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use localcore_log::{has_blob_import, read_all, Event, TYPE_NOTE};
use serde_json::json;

const HELLO_SHA: &str = "a70940623490fa4c251737cf74e1bf75a0327bb18766cc5620edbde3a985c96d";

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Health Go fixture. From `tests/` that is `../../../apps/health/testdata/m0`.
fn go_m0_root() -> PathBuf {
    crate_dir().join("tests").join("../../../apps/health/testdata/m0")
}

fn rust_m0_root() -> PathBuf {
    crate_dir().join("tests/fixtures/m0")
}

fn collect_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    walk_files(root, root, &mut out);
    out
}

fn walk_files(base: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display()));
    for ent in entries {
        let ent = ent.unwrap();
        let path = ent.path();
        if ent.file_type().unwrap().is_dir() {
            walk_files(base, &path, out);
        } else {
            let rel = path
                .strip_prefix(base)
                .unwrap_or_else(|_| panic!("{} not under {}", path.display(), base.display()))
                .to_path_buf();
            out.insert(rel, fs::read(&path).unwrap());
        }
    }
}

#[test]
fn go_and_rust_m0_log_files_are_byte_identical() {
    let go_log = go_m0_root().join("log");
    let rust_log = rust_m0_root().join("log");
    let go_files = collect_files(&go_log);
    let rust_files = collect_files(&rust_log);
    assert!(
        go_files.contains_key(Path::new("manual/2024-01.ndjson"))
            && go_files.contains_key(Path::new("garmin/2024-01.ndjson")),
        "Go m0 log missing expected files; found {:?}",
        go_files.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        go_files.keys().collect::<Vec<_>>(),
        rust_files.keys().collect::<Vec<_>>(),
        "m0 log file set drifted between health Go and the Rust copy"
    );
    for (rel, go_bytes) in &go_files {
        let rust_bytes = rust_files.get(rel).unwrap();
        assert_eq!(
            go_bytes, rust_bytes,
            "byte drift in {rel:?} (Go {} bytes, Rust {} bytes)",
            go_bytes.len(),
            rust_bytes.len()
        );
    }
}

#[test]
fn read_all_go_tree_matches_rust_copy() {
    let go_evs = read_all(go_m0_root()).unwrap();
    let rust_evs = read_all(rust_m0_root()).unwrap();
    assert_eq!(go_evs.len(), 6);
    assert_eq!(go_evs.len(), rust_evs.len());
    for (i, (go, rust)) in go_evs.iter().zip(rust_evs.iter()).enumerate() {
        assert_eq!(go.id, rust.id, "id mismatch at {i}");
        assert_eq!(go.ts, rust.ts, "ts mismatch at {i}");
        assert_eq!(go.dev, rust.dev, "dev mismatch at {i}");
        assert_eq!(go.event_type, rust.event_type, "type mismatch at {i}");
        assert_eq!(go.body, rust.body, "body mismatch at {i}");
    }
}

#[test]
fn marshal_note_does_not_html_escape() {
    // Go `json.Encoder` + `SetEscapeHTML(false)`: `<`, `>`, `&` stay literal.
    let ev = Event::new(
        "01900000-0000-7000-8000-0000000000aa",
        "2024-06-02T08:00:00.000000000Z",
        "manual",
        TYPE_NOTE,
        json!({"text": "a<b>&c"}),
    );
    let line = ev.marshal_line().unwrap();
    let want = b"{\"id\":\"01900000-0000-7000-8000-0000000000aa\",\"ts\":\"2024-06-02T08:00:00.000000000Z\",\"dev\":\"manual\",\"type\":\"note\",\"body\":{\"text\":\"a<b>&c\"}}\n";
    assert_eq!(
        line, want,
        "marshal drifted from Go SetEscapeHTML(false) golden:\n{}",
        String::from_utf8_lossy(&line)
    );
}

#[test]
fn has_blob_import_on_go_tree() {
    assert!(
        has_blob_import(go_m0_root(), HELLO_SHA).unwrap(),
        "Go m0 fixture missing blob_import for hello.txt"
    );
}
