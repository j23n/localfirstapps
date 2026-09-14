use std::fs;
use std::path::{Path, PathBuf};

use localcore_log::{
    append, has_blob_import, read_all, read_report, Error, Event, TYPE_NOTE, TYPE_SUPERSEDE,
};
use serde_json::json;

fn m0_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/m0")
}

fn copy_log_tree(dst: &Path, src: &Path) {
    for ent in fs::read_dir(src).unwrap() {
        let ent = ent.unwrap();
        let dest = dst.join(ent.file_name());
        if ent.file_type().unwrap().is_dir() {
            fs::create_dir_all(&dest).unwrap();
            copy_log_tree(&dest, &ent.path());
        } else {
            fs::create_dir_all(dest.parent().unwrap()).unwrap();
            fs::copy(ent.path(), dest).unwrap();
        }
    }
}

#[test]
fn append_writes_line() {
    let root = tempfile::tempdir().unwrap();
    let ev = Event::new(
        "01900000-0000-7000-8000-0000000000aa",
        "2024-06-02T08:00:00.000000000Z",
        "manual",
        TYPE_NOTE,
        json!({"text": "hello"}),
    );
    append(root.path(), &ev).unwrap();
    let path = root.path().join("log/manual/2024-06.ndjson");
    let b = fs::read_to_string(&path).unwrap();
    let want = "{\"id\":\"01900000-0000-7000-8000-0000000000aa\",\"ts\":\"2024-06-02T08:00:00.000000000Z\",\"dev\":\"manual\",\"type\":\"note\",\"body\":{\"text\":\"hello\"}}\n";
    assert_eq!(b, want);
    let mut ev2 = ev.clone();
    ev2.id = "01900000-0000-7000-8000-0000000000ab".into();
    ev2.ts = "2024-06-02T09:00:00.000000000Z".into();
    append(root.path(), &ev2).unwrap();
    let got = fs::read_to_string(&path).unwrap();
    assert!(
        got.len() >= 2 * want.len() && got.starts_with(want),
        "append-only violated:\n{got}"
    );
}

#[test]
fn read_all_fixture() {
    let root = m0_root();
    let evs = read_all(&root).unwrap();
    assert_eq!(evs.len(), 6);
    assert_eq!(evs[0].id, "01900000-0000-7000-8000-000000000001");
    assert_eq!(evs[0].event_type, TYPE_NOTE);
    assert!(has_blob_import(
        &root,
        "a70940623490fa4c251737cf74e1bf75a0327bb18766cc5620edbde3a985c96d"
    )
    .unwrap());
    assert_eq!(evs[2].event_type, TYPE_SUPERSEDE);
    assert_eq!(
        evs[2].target().as_deref(),
        Some("01900000-0000-7000-8000-000000000002")
    );
}

#[test]
fn read_all_torn_tail() {
    let dst = tempfile::tempdir().unwrap();
    copy_log_tree(&dst.path().join("log"), &m0_root().join("log"));
    let path = dst.path().join("log/manual/2024-01.ndjson");
    let offset = fs::metadata(&path).unwrap().len();
    let mut f = fs::OpenOptions::new()
        .append(true)
        .write(true)
        .open(&path)
        .unwrap();
    use std::io::Write;
    f.write_all(br#"{"id":"01900000-torn"#).unwrap();
    drop(f);

    let report = read_report(dst.path()).unwrap();
    assert_eq!(
        report.events.len(),
        6,
        "complete records must remain readable"
    );
    let torn = report.torn_tails.first().expect("torn-tail diagnostic");
    assert_eq!(torn.path, path);
    assert_eq!(torn.offset, offset);
}

#[test]
fn read_all_mid_file_malformed_is_hard_error() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("log/manual");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("2024-01.ndjson");
    fs::write(
        &path,
        concat!(
            r#"{"id":"a","ts":"2024-01-15T12:00:00.000000000Z","dev":"manual","type":"note","body":{"text":"ok"}}"#,
            "\n",
            "{not-json}\n",
            r#"{"id":"b","ts":"2024-01-15T12:01:00.000000000Z","dev":"manual","type":"note","body":{"text":"ok"}}"#,
            "\n",
        ),
    )
    .unwrap();
    match read_all(root.path()) {
        Err(Error::Json { line, .. }) => assert_eq!(line, 2),
        other => panic!("want mid-file Json error, got {other:?}"),
    }
}

#[test]
#[cfg(unix)]
fn append_file_mode() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let ev = Event::new(
        "01900000-0000-7000-8000-0000000000aa",
        "2024-06-02T08:00:00.000000000Z",
        "manual",
        TYPE_NOTE,
        json!({"text": "hello"}),
    );
    append(root.path(), &ev).unwrap();
    let dir = root.path().join("log/manual");
    let dir_mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
    assert_eq!(dir_mode, 0o700, "log dir mode {dir_mode:o}");
    let file_mode = fs::metadata(dir.join("2024-06.ndjson"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(file_mode, 0o600, "log file mode {file_mode:o}");
}
