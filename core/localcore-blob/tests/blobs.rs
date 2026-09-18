use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use localcore_blob::{
    exists, exists_on, hash_file, list, list_on, open, path, path_on, put, put_on, verify,
    verify_on, Error,
};
use localcore_vfs::{MemVfs, Vfs, VfsError};

const HELLO_SHA: &str = "a70940623490fa4c251737cf74e1bf75a0327bb18766cc5620edbde3a985c96d";

fn hello_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hello.txt")
}

fn go_m0_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/health/testdata/m0")
}

fn rust_m0_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/m0")
}

fn collect_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    walk_files(root, root, &mut out);
    out
}

fn walk_files(base: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for ent in fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display())) {
        let ent = ent.unwrap();
        let path = ent.path();
        if ent.file_type().unwrap().is_dir() {
            walk_files(base, &path, out);
        } else {
            let rel = path.strip_prefix(base).unwrap().to_path_buf();
            out.insert(rel, fs::read(path).unwrap());
        }
    }
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
    let root = rust_m0_root();
    let listed = list(&root).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].sha256, HELLO_SHA);
    assert_eq!(listed[0].size, 40);
    verify(&root, HELLO_SHA).unwrap();
    let listed_empty = list(tempfile::tempdir().unwrap().path()).unwrap();
    assert!(listed_empty.is_empty());
}

#[test]
fn go_and_rust_m0_blobs_are_byte_and_hash_identical() {
    let go_root = go_m0_root();
    let rust_root = rust_m0_root();
    let go_files = collect_files(&go_root.join("blobs"));
    let rust_files = collect_files(&rust_root.join("blobs"));
    assert_eq!(
        go_files.keys().collect::<Vec<_>>(),
        rust_files.keys().collect::<Vec<_>>(),
        "m0 blob file set drifted between health Go and the Rust copy"
    );
    assert_eq!(go_files, rust_files, "m0 blob bytes drifted");

    let go = list(&go_root).unwrap();
    let rust = list(&rust_root).unwrap();
    assert_eq!(go.len(), 1);
    assert_eq!(rust.len(), 1);
    assert_eq!(go[0].sha256, HELLO_SHA);
    assert_eq!(rust[0].sha256, HELLO_SHA);
    assert_eq!(go[0].size, rust[0].size);

    let fixture = fs::read(hello_path()).unwrap();
    let go_bytes = fs::read(&go[0].path).unwrap();
    let rust_bytes = fs::read(&rust[0].path).unwrap();
    assert_eq!(
        go_bytes, fixture,
        "Go m0 content drifted from hello fixture"
    );
    assert_eq!(
        rust_bytes, fixture,
        "Rust m0 content drifted from hello fixture"
    );
    assert_eq!(hash_file(&go[0].path).unwrap(), (HELLO_SHA.into(), 40));
    assert_eq!(hash_file(&rust[0].path).unwrap(), (HELLO_SHA.into(), 40));
    verify(&go_root, HELLO_SHA).unwrap();
    verify(&rust_root, HELLO_SHA).unwrap();
}

#[test]
fn vfs_error_stays_typed() {
    let vfs = MemVfs::new();
    match verify_on(&vfs, "/store", HELLO_SHA) {
        Err(Error::Vfs(VfsError::NotFound { path })) => {
            assert!(path.contains(HELLO_SHA), "{path}");
        }
        other => panic!("want Error::Vfs(NotFound), got {other:?}"),
    }
    let err = Error::from(VfsError::NotFound {
        path: "/missing".into(),
    });
    assert!(matches!(err, Error::Vfs(VfsError::NotFound { .. })));
    assert_eq!(err.to_string(), "not found: /missing");
}

#[test]
fn put_on_existed_verifies_content() {
    let vfs = MemVfs::new();
    let first = put_on(&vfs, "/store", &b"hello"[..]).unwrap();
    assert!(!first.existed);
    verify_on(&vfs, "/store", &first.hash).unwrap();

    let again = put_on(&vfs, "/store", &b"hello"[..]).unwrap();
    assert!(again.existed);
    assert_eq!(again.hash, first.hash);

    let dest = path_on("/store", &first.hash).unwrap();
    vfs.write_atomic(&dest, b"CORRUPT").unwrap();
    match put_on(&vfs, "/store", &b"hello"[..]) {
        Err(Error::VerifyMismatch { hash, actual }) => {
            assert_eq!(hash, first.hash);
            assert_ne!(actual, first.hash);
        }
        other => panic!("want VerifyMismatch, got {other:?}"),
    }
    assert!(
        vfs.paths().iter().all(|p| !p.contains("/blobs/tmp/put-")),
        "temp must be removed after a verify mismatch: {:?}",
        vfs.paths()
    );
    assert!(exists_on(&vfs, "/store", &first.hash).unwrap());
}

#[test]
#[cfg(unix)]
fn list_skips_hash_named_symlink() {
    let root = tempfile::tempdir().unwrap();
    let stored = put(root.path(), &b"hello"[..]).unwrap();
    let dest = path(root.path(), &stored.hash).unwrap();
    let planted = "0".repeat(64);
    let link = dest.parent().unwrap().join(&planted);
    std::os::unix::fs::symlink(&dest, &link).unwrap();

    let listed = list(root.path()).unwrap();
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].sha256, stored.hash);

    let vfs = localcore_vfs::StdVfs::new(localcore_blob::TEMP_PREFIX);
    let via_vfs = list_on(&vfs, root.path().to_str().unwrap()).unwrap();
    assert_eq!(via_vfs.len(), 1);
    assert_eq!(via_vfs[0].sha256, stored.hash);
}
