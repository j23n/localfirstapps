//! Gallery XMP conflict FFI (ADR 0005 R8–R11).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gallery_ffi::{
    is_conflict_name, ConflictError, ConflictKind, ConflictSession, MergeKind, ScanCommand,
    ScannerSession,
};

fn gallery_minimal() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../core/localcore-conflict/fixtures/trees/gallery-minimal")
}

fn preservation() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../gallery-meta/tests/fixtures/conflicts/syncthing-preservation")
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            fs::copy(entry.path(), dst.join(entry.file_name())).unwrap();
        }
    }
}

fn set_mtime(path: &Path, secs: u64) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
        .unwrap();
}

fn stamp_xmp_mtimes(dir: &Path, surviving: &str, phone: &str, laptop: &str) {
    let surviving_path = dir.join(surviving);
    if surviving_path.is_file() {
        set_mtime(&surviving_path, 1_000);
    }
    set_mtime(&dir.join(phone), 2_000);
    set_mtime(&dir.join(laptop), 3_000);
}

fn overlay_preservation_xmp(dst: &Path) {
    let src = preservation();
    fs::copy(src.join("photo.heic.xmp"), dst.join("photo.heic.xmp")).unwrap();
    fs::copy(
        src.join("photo.heic.sync-conflict-20260915-101500-PHONE01.xmp"),
        dst.join("photo.heic.sync-conflict-20200901-120000-PHONE01.xmp"),
    )
    .unwrap();
    fs::copy(
        src.join("photo.heic.sync-conflict-20260916-081000-LAPTOP02.xmp"),
        dst.join("photo.heic.sync-conflict-20200903-090000-LAPTOP02.xmp"),
    )
    .unwrap();
    stamp_xmp_mtimes(
        dst,
        "photo.heic.xmp",
        "photo.heic.sync-conflict-20200901-120000-PHONE01.xmp",
        "photo.heic.sync-conflict-20200903-090000-LAPTOP02.xmp",
    );
}

fn parseable_gallery_minimal() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&gallery_minimal(), dir.path());
    overlay_preservation_xmp(dir.path());
    dir
}

fn union_tags(value: &str) {
    for tag in [
        "Albums/Favorites",
        "Objects/Animal/Dog",
        "People/Family",
        "Trips/Italy",
    ] {
        assert!(value.contains(tag), "missing union tag {tag} in {value}");
    }
    assert!(
        !value.contains("Objects/Animal/Cat"),
        "old core Cat leaked into {value}"
    );
}

#[test]
fn ffi_scan_still_hides_copies_and_does_not_export_groups() {
    let root = gallery_minimal();
    let out = ScannerSession::new()
        .scan(
            root.to_str().unwrap().to_string(),
            ScanCommand {
                reuse_cached: false,
                cached_photos: Vec::new(),
                cached_sidecar_manifest: Vec::new(),
            },
            None,
        )
        .unwrap();
    assert_eq!(out.flat_photos.len(), 1);
    assert!(out
        .flat_photos
        .iter()
        .all(|photo| !photo.path.contains("sync-conflict")));
    assert_eq!(out.sidecar_manifest.len(), 1);
    assert!(out
        .sidecar_manifest
        .iter()
        .all(|row| !row.sidecar_path.contains("sync-conflict")));
}

#[test]
fn gallery_minimal_lists_image_and_xmp_rows() {
    let dir = parseable_gallery_minimal();
    let session = ConflictSession::open(dir.path().to_str().unwrap().to_string());
    let rows = session.conflict_rows().unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0].id, "photo.heic");
    assert_eq!(rows[0].title, "photo.heic");
    assert_eq!(rows[0].subtitle, "2 copies");
    assert_eq!(rows[0].trailing, "keep one");
    assert_eq!(rows[0].disposition, MergeKind::KeepOne);
    assert_eq!(rows[0].kind, ConflictKind::Image);
    assert_eq!(rows[1].id, "photo.heic.xmp");
    assert_eq!(rows[1].title, "photo.heic.xmp");
    assert_eq!(rows[1].subtitle, "2 copies");
    assert_eq!(rows[1].trailing, "auto");
    assert_eq!(rows[1].disposition, MergeKind::Auto);
    assert_eq!(rows[1].kind, ConflictKind::Xmp);
    assert!(!rows.iter().any(|row| row.disposition == MergeKind::Choice));
}

#[test]
fn conflict_name_matches_grammar() {
    assert!(is_conflict_name(
        "photo.heic.sync-conflict-20200901-120000-PHONE01.xmp".into()
    ));
    assert!(is_conflict_name(
        "alice.sync-conflict-20200901-120000-PHONE01.vcf".into()
    ));
    assert!(!is_conflict_name("photo.heic.xmp".into()));
    assert!(!is_conflict_name("alice.vcf".into()));
}

#[test]
fn unparseable_omitted_from_rows_preview_is_sidecar() {
    let root = gallery_minimal();
    let session = ConflictSession::open(root.to_str().unwrap().to_string());
    let rows = session.conflict_rows().unwrap();
    assert_eq!(
        rows.len(),
        1,
        "dummy XMP omitted; image group listed: {rows:?}"
    );
    assert_eq!(rows[0].id, "photo.heic");
    assert_eq!(rows[0].kind, ConflictKind::Image);
    assert!(!rows.iter().any(|row| row.id == "photo.heic.xmp"));
    let preview = session
        .conflict_preview("photo.heic.xmp".into())
        .unwrap_err();
    assert!(
        matches!(preview, ConflictError::Sidecar { .. }),
        "{preview:?}"
    );
    let resolve = session.resolve_group("photo.heic.xmp".into()).unwrap_err();
    assert!(
        matches!(resolve, ConflictError::Sidecar { .. }),
        "{resolve:?}"
    );
}

#[test]
fn auto_preview_does_not_write() {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&preservation(), dir.path());
    stamp_xmp_mtimes(
        dir.path(),
        "photo.heic.xmp",
        "photo.heic.sync-conflict-20260915-101500-PHONE01.xmp",
        "photo.heic.sync-conflict-20260916-081000-LAPTOP02.xmp",
    );
    let before: Vec<(String, Vec<u8>)> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect();

    let session = ConflictSession::open(dir.path().to_str().unwrap().to_string());
    let preview = session.conflict_preview("photo.heic.xmp".into()).unwrap();
    assert_eq!(preview.id, "photo.heic.xmp");
    assert_eq!(preview.title, "photo.heic.xmp");
    assert_eq!(preview.kind, MergeKind::Auto);
    assert!(preview.fields.is_empty());
    assert_eq!(
        preview.discarded,
        [
            "photo.heic.sync-conflict-20260915-101500-PHONE01.xmp",
            "photo.heic.sync-conflict-20260916-081000-LAPTOP02.xmp",
        ]
    );
    let tags = preview
        .merged_fields
        .iter()
        .find(|row| row.label == "Tags")
        .unwrap();
    union_tags(&tags.value);
    assert!(preview.merged_fields.iter().all(|row| !row.editable));

    let after: Vec<(String, Vec<u8>)> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect();
    assert_eq!(before, after, "preview must not touch disk");
}

#[test]
fn auto_resolve_writes_union_and_deletes_copies() {
    let dir = parseable_gallery_minimal();
    let photo = dir.path().join("photo.heic");
    let photo_before = fs::read(&photo).unwrap();
    let session = ConflictSession::open(dir.path().to_str().unwrap().to_string());
    session.resolve_group("photo.heic.xmp".into()).unwrap();

    assert!(!dir
        .path()
        .join("photo.heic.sync-conflict-20200901-120000-PHONE01.xmp")
        .exists());
    assert!(!dir
        .path()
        .join("photo.heic.sync-conflict-20200903-090000-LAPTOP02.xmp")
        .exists());
    assert!(dir
        .path()
        .join("photo.sync-conflict-20200901-120000-PHONE01.heic")
        .exists());
    assert_eq!(fs::read(&photo).unwrap(), photo_before);

    let text = String::from_utf8(fs::read(dir.path().join("photo.heic.xmp")).unwrap()).unwrap();
    union_tags(&text);
    let rows = session.conflict_rows().unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].kind, ConflictKind::Image);
    assert_eq!(rows[0].id, "photo.heic");
}

#[test]
fn deleted_versus_modified_recreates_surviving() {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&preservation(), dir.path());
    stamp_xmp_mtimes(
        dir.path(),
        "photo.heic.xmp",
        "photo.heic.sync-conflict-20260915-101500-PHONE01.xmp",
        "photo.heic.sync-conflict-20260916-081000-LAPTOP02.xmp",
    );
    fs::remove_file(dir.path().join("photo.heic.xmp")).unwrap();

    let session = ConflictSession::open(dir.path().to_str().unwrap().to_string());
    let rows = session.conflict_rows().unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].id, "photo.heic.xmp");
    assert_eq!(rows[0].kind, ConflictKind::Xmp);
    assert_eq!(rows[0].disposition, MergeKind::DeletedVersusModified);
    assert_eq!(rows[0].trailing, "keep copy");
    assert_ne!(rows[0].disposition, MergeKind::Choice);

    session.resolve_group("photo.heic.xmp".into()).unwrap();
    let surviving = dir.path().join("photo.heic.xmp");
    assert!(surviving.is_file());
    let text = String::from_utf8(fs::read(surviving).unwrap()).unwrap();
    for tag in ["Albums/Favorites", "Objects/Animal/Dog", "People/Family"] {
        assert!(text.contains(tag), "missing copy-union tag {tag} in {text}");
    }
    assert!(
        !text.contains("Objects/Animal/Cat"),
        "old core Cat leaked into {text}"
    );
    assert!(!dir
        .path()
        .join("photo.heic.sync-conflict-20260915-101500-PHONE01.xmp")
        .exists());
    assert!(!dir
        .path()
        .join("photo.heic.sync-conflict-20260916-081000-LAPTOP02.xmp")
        .exists());
    assert!(session.conflict_rows().unwrap().is_empty());
}

#[test]
fn image_group_is_not_an_xmp_group() {
    let root = gallery_minimal();
    let session = ConflictSession::open(root.to_str().unwrap().to_string());
    let preview = session.conflict_preview("photo.heic".into()).unwrap_err();
    assert!(
        matches!(preview, ConflictError::NotAnXmpGroup { .. }),
        "{preview:?}"
    );
    let resolve = session.resolve_group("photo.heic".into()).unwrap_err();
    assert!(
        matches!(resolve, ConflictError::NotAnXmpGroup { .. }),
        "{resolve:?}"
    );
    assert!(matches!(
        session.conflict_preview("missing.xmp".into()).unwrap_err(),
        ConflictError::NotFound
    ));
    assert!(matches!(
        session.image_preview("photo.heic.xmp".into()).unwrap_err(),
        ConflictError::NotAnImageGroup { .. }
    ));
    assert!(matches!(
        session
            .keep_image_copy("photo.heic.xmp".into(), "photo.heic.xmp".into())
            .unwrap_err(),
        ConflictError::NotAnImageGroup { .. }
    ));
}

#[test]
fn no_choice_rows_on_parseable_or_r11_groups() {
    let dir = parseable_gallery_minimal();
    let session = ConflictSession::open(dir.path().to_str().unwrap().to_string());
    assert!(session
        .conflict_rows()
        .unwrap()
        .iter()
        .all(|row| row.disposition != MergeKind::Choice));

    let r11 = tempfile::tempdir().unwrap();
    copy_tree(&preservation(), r11.path());
    stamp_xmp_mtimes(
        r11.path(),
        "photo.heic.xmp",
        "photo.heic.sync-conflict-20260915-101500-PHONE01.xmp",
        "photo.heic.sync-conflict-20260916-081000-LAPTOP02.xmp",
    );
    fs::remove_file(r11.path().join("photo.heic.xmp")).unwrap();
    let session = ConflictSession::open(r11.path().to_str().unwrap().to_string());
    assert!(session
        .conflict_rows()
        .unwrap()
        .iter()
        .all(|row| row.disposition != MergeKind::Choice));
}

#[test]
fn image_preview_lists_surviving_and_copies_without_writing() {
    let root = gallery_minimal();
    let before: Vec<(String, Vec<u8>)> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect();
    let session = ConflictSession::open(root.to_str().unwrap().to_string());
    let preview = session.image_preview("photo.heic".into()).unwrap();
    assert_eq!(preview.id, "photo.heic");
    assert_eq!(preview.title, "photo.heic");
    assert_eq!(
        preview.copies,
        [
            "photo.heic",
            "photo.sync-conflict-20200901-120000-PHONE01.heic",
            "photo.sync-conflict-20200902-130000-LAPTOP02.heic",
        ]
    );
    let after: Vec<(String, Vec<u8>)> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect();
    assert_eq!(before, after, "image preview must not touch disk");
}

#[test]
fn keep_image_surviving_deletes_copies_and_leaves_bytes() {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&gallery_minimal(), dir.path());
    let photo = dir.path().join("photo.heic");
    let photo_before = fs::read(&photo).unwrap();
    let session = ConflictSession::open(dir.path().to_str().unwrap().to_string());
    session
        .keep_image_copy("photo.heic".into(), "photo.heic".into())
        .unwrap();

    assert_eq!(fs::read(&photo).unwrap(), photo_before);
    assert!(!dir
        .path()
        .join("photo.sync-conflict-20200901-120000-PHONE01.heic")
        .exists());
    assert!(!dir
        .path()
        .join("photo.sync-conflict-20200902-130000-LAPTOP02.heic")
        .exists());
    assert!(dir.path().join("photo.heic.xmp").exists());
    let rows = session.conflict_rows().unwrap();
    assert!(
        !rows.iter().any(|row| row.kind == ConflictKind::Image),
        "{rows:?}"
    );
}

#[test]
fn keep_image_copy_renames_chosen_file_and_does_not_rewrite_bytes() {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&gallery_minimal(), dir.path());
    let chosen_name = "photo.sync-conflict-20200901-120000-PHONE01.heic";
    let chosen_bytes = fs::read(dir.path().join(chosen_name)).unwrap();
    let session = ConflictSession::open(dir.path().to_str().unwrap().to_string());
    session
        .keep_image_copy("photo.heic".into(), chosen_name.into())
        .unwrap();

    let surviving = dir.path().join("photo.heic");
    assert_eq!(fs::read(&surviving).unwrap(), chosen_bytes);
    assert!(!dir.path().join(chosen_name).exists());
    assert!(!dir
        .path()
        .join("photo.sync-conflict-20200902-130000-LAPTOP02.heic")
        .exists());
    assert!(dir.path().join("photo.heic.xmp").exists());
}

#[test]
fn keep_image_copy_promotes_when_surviving_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&gallery_minimal(), dir.path());
    fs::remove_file(dir.path().join("photo.heic")).unwrap();
    let chosen_name = "photo.sync-conflict-20200902-130000-LAPTOP02.heic";
    let chosen_bytes = fs::read(dir.path().join(chosen_name)).unwrap();
    let session = ConflictSession::open(dir.path().to_str().unwrap().to_string());
    let preview = session.image_preview("photo.heic".into()).unwrap();
    assert_eq!(
        preview.copies,
        [
            "photo.sync-conflict-20200901-120000-PHONE01.heic",
            chosen_name,
        ]
    );
    session
        .keep_image_copy("photo.heic".into(), chosen_name.into())
        .unwrap();
    assert_eq!(
        fs::read(dir.path().join("photo.heic")).unwrap(),
        chosen_bytes
    );
    assert!(!dir.path().join(chosen_name).exists());
    assert!(!dir
        .path()
        .join("photo.sync-conflict-20200901-120000-PHONE01.heic")
        .exists());
}

#[test]
fn keep_image_copy_unknown_name_is_not_found() {
    let root = gallery_minimal();
    let session = ConflictSession::open(root.to_str().unwrap().to_string());
    assert!(matches!(
        session
            .keep_image_copy("photo.heic".into(), "missing.heic".into())
            .unwrap_err(),
        ConflictError::NotFound
    ));
    assert!(matches!(
        session.image_preview("missing.heic".into()).unwrap_err(),
        ConflictError::NotFound
    ));
}
