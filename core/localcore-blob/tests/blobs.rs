use std::fs;
use std::io::Read;
use std::path::PathBuf;

use localcore_blob::{exists, hash_file, list, open, path, put, verify};

const HELLO_SHA: &str = "a70940623490fa4c251737cf74e1bf75a0327bb18766cc5620edbde3a985c96d";

fn hello_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hello.txt")
}

fn m0_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/m0")
}

#[test]
fn put_golden_hello() {
    let src = fs::read(hello_path()).unwrap();
    assert_eq!(src.len(), 40);
    let root = tempfile::tempdir().unwrap();
    let first = put(root.path(), src.as_slice()).unwrap();
    assert!(!first.existed, "first put existed");
    assert_eq!(first.hash, HELLO_SHA);
    assert_eq!(first.size, 40);
    let p = path(root.path(), &first.hash).unwrap();
    let got = fs::read(&p).unwrap();
    assert_eq!(got, src);
    let second = put(root.path(), src.as_slice()).unwrap();
    assert!(second.existed, "second put should exist");
    assert_eq!(second.hash, HELLO_SHA);
    assert_eq!(second.size, 40);
    assert!(exists(root.path(), HELLO_SHA).unwrap());
    let (hashed, n) = hash_file(&p).unwrap();
    assert_eq!(hashed, HELLO_SHA);
    assert_eq!(n, 40);
    let mut f = open(root.path(), HELLO_SHA).unwrap();
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).unwrap();
    assert_eq!(buf, src);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "blob file mode {mode:o}");
        let dir_mode = fs::metadata(p.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700, "blob dir mode {dir_mode:o}");
    }
}

#[test]
fn path_rejects_bad_hash() {
    assert!(path("/tmp", "abcd").is_err());
}

#[test]
fn list_and_verify_fixture() {
    let root = m0_root();
    let listed = list(&root).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].sha256, HELLO_SHA);
    assert_eq!(listed[0].size, 40);
    verify(&root, HELLO_SHA).unwrap();
    let listed_empty = list(tempfile::tempdir().unwrap().path()).unwrap();
    assert!(listed_empty.is_empty());
}
