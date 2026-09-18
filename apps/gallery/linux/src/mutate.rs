//! Headless photo disk mutations. GTK only calls these from a worker.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use gallery_meta::{alt_sidecar_path, sidecar_path};

use crate::decode::{decode_limited, RgbFrame};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationTarget {
    pub id: String,
    pub path: PathBuf,
    pub is_video: bool,
    pub live_photo_video_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeleteResult {
    pub deleted_ids: Vec<String>,
    pub failed: Vec<String>,
    pub partial_ids: Vec<String>,
    pub needs_reconciliation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovedPhoto {
    pub old_id: String,
    pub new_path: PathBuf,
    pub new_live_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MoveResult {
    pub moved: Vec<MovedPhoto>,
    pub failed: Vec<String>,
    pub partial_ids: Vec<String>,
    pub skipped: Vec<String>,
    pub needs_reconciliation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExportResult {
    pub saved: Vec<PathBuf>,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareQuality {
    Original,
    High,
    Medium,
    Small,
}

impl ShareQuality {
    pub const fn max_side(self) -> Option<u32> {
        match self {
            Self::Original => None,
            Self::High => Some(4096),
            Self::Medium => Some(2048),
            Self::Small => Some(1024),
        }
    }

    pub const fn slug(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Small => "small",
        }
    }

    pub const fn jpeg_quality(self) -> u8 {
        match self {
            Self::Original => 100,
            Self::High => 90,
            Self::Medium => 85,
            Self::Small => 80,
        }
    }
}

#[derive(Debug)]
pub enum MutateError {
    DestOutsideLibrary,
    DestNotDirectory,
    InvalidFolderName,
    Io(io::Error),
}

impl std::fmt::Display for MutateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DestOutsideLibrary => {
                formatter.write_str("Destination is outside the photo folder")
            }
            Self::DestNotDirectory => formatter.write_str("Destination is not a folder"),
            Self::InvalidFolderName => formatter.write_str("Folder name is not usable"),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for MutateError {}

impl From<io::Error> for MutateError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Copy)]
enum StepStatus {
    Succeeded,
    Missing,
    Failed,
}

pub fn delete_photos(targets: &[MutationTarget]) -> DeleteResult {
    let mut result = DeleteResult::default();
    for target in targets {
        match delete_one(target) {
            DeleteVerdict::Deleted => result.deleted_ids.push(target.id.clone()),
            DeleteVerdict::Partial => {
                result.deleted_ids.push(target.id.clone());
                result.partial_ids.push(target.id.clone());
                result.needs_reconciliation = true;
            }
            DeleteVerdict::Failed => result.failed.push(target.id.clone()),
        }
    }
    result
}

enum DeleteVerdict {
    Deleted,
    Partial,
    Failed,
}

fn delete_one(target: &MutationTarget) -> DeleteVerdict {
    let primary = target.path.as_path();
    let primary_was_file = primary.is_file();
    match remove_if_present(primary) {
        StepStatus::Failed => return DeleteVerdict::Failed,
        StepStatus::Succeeded | StepStatus::Missing => {}
    }
    if primary.exists() {
        return DeleteVerdict::Failed;
    }

    let mut leftover = false;
    delete_companions(target, &mut leftover);
    leftover = leftover || companions_still_present(target);
    if leftover {
        return DeleteVerdict::Partial;
    }
    if primary_was_file {
        DeleteVerdict::Deleted
    } else {
        DeleteVerdict::Failed
    }
}

fn delete_companions(target: &MutationTarget, leftover: &mut bool) {
    for path in companion_paths(target) {
        match remove_if_present(&path) {
            StepStatus::Failed => *leftover = true,
            StepStatus::Succeeded | StepStatus::Missing => {}
        }
        if path.exists() {
            *leftover = true;
        }
    }
}

fn companions_still_present(target: &MutationTarget) -> bool {
    companion_paths(target)
        .into_iter()
        .any(|path| path.exists())
}

fn companion_paths(target: &MutationTarget) -> Vec<PathBuf> {
    let mut paths = sidecar_pair(&target.path);
    if let Some(live) = &target.live_photo_video_path {
        paths.push(live.clone());
        paths.extend(sidecar_pair(live));
    }
    paths
}

fn sidecar_pair(path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(text) = path.to_str() {
        paths.push(PathBuf::from(sidecar_path(text)));
        if let Some(alt) = alt_sidecar_path(text) {
            let alt = PathBuf::from(alt);
            if !paths.iter().any(|path| path == &alt) {
                paths.push(alt);
            }
        }
    }
    paths
}

fn remove_if_present(path: &Path) -> StepStatus {
    if !path.exists() {
        return StepStatus::Missing;
    }
    if !path.is_file() {
        return StepStatus::Failed;
    }
    match fs::remove_file(path) {
        Ok(()) => {
            if path.exists() {
                StepStatus::Failed
            } else {
                StepStatus::Succeeded
            }
        }
        Err(_) => {
            if path.exists() {
                StepStatus::Failed
            } else {
                StepStatus::Succeeded
            }
        }
    }
}

pub fn dest_is_under_root(dest: &Path, library_root: &Path) -> bool {
    let Ok(root) = library_root.canonicalize() else {
        return false;
    };
    resolved_path(dest).is_some_and(|dest| dest.starts_with(&root))
}

fn resolved_path(path: &Path) -> Option<PathBuf> {
    if let Ok(canon) = path.canonicalize() {
        return Some(canon);
    }
    let mut suffix = Vec::new();
    let mut cursor = path.to_path_buf();
    loop {
        if let Ok(canon) = cursor.canonicalize() {
            let mut out = canon;
            for part in suffix.iter().rev() {
                out.push(part);
            }
            return Some(out);
        }
        match cursor.file_name() {
            Some(name) => {
                suffix.push(name.to_os_string());
                if !cursor.pop() {
                    return None;
                }
            }
            None => return None,
        }
    }
}

pub fn move_photos(
    targets: &[MutationTarget],
    dest: &Path,
    library_root: &Path,
) -> Result<MoveResult, MutateError> {
    if !dest_is_under_root(dest, library_root) {
        return Err(MutateError::DestOutsideLibrary);
    }
    if !dest.is_dir() {
        return Err(MutateError::DestNotDirectory);
    }
    let dest = dest.canonicalize().map_err(MutateError::Io)?;
    let mut result = MoveResult::default();
    for target in targets {
        match move_one(target, &dest) {
            MoveVerdict::Moved(photo) => result.moved.push(photo),
            MoveVerdict::Skipped => result.skipped.push(target.id.clone()),
            MoveVerdict::Failed => result.failed.push(target.id.clone()),
            MoveVerdict::Partial(photo) => {
                result.moved.push(photo);
                result.partial_ids.push(target.id.clone());
                result.needs_reconciliation = true;
            }
        }
    }
    Ok(result)
}

enum MoveVerdict {
    Moved(MovedPhoto),
    Skipped,
    Failed,
    Partial(MovedPhoto),
}

struct MoveStep {
    from: PathBuf,
    to: PathBuf,
    primary: bool,
}

fn move_one(target: &MutationTarget, dest: &Path) -> MoveVerdict {
    let Some(parent) = target.path.parent() else {
        return MoveVerdict::Failed;
    };
    let already = match (parent.canonicalize(), dest.canonicalize()) {
        (Ok(here), Ok(there)) => here == there,
        _ => parent == dest,
    };
    if already {
        return MoveVerdict::Skipped;
    }
    if !target.path.is_file() {
        return MoveVerdict::Failed;
    }

    let names = unique_names(target, dest);
    let new_path = dest.join(&names.photo);
    let new_live = names.live.as_ref().map(|name| dest.join(name));
    let steps = move_plan(target, dest, &names);
    let mut completed = Vec::new();
    let mut primary_ok = false;
    let mut companion_failed = false;

    for step in &steps {
        let existed = step.from.exists();
        if !existed {
            continue;
        }
        match relocate(&step.from, &step.to) {
            Ok(()) if step.to.exists() => {
                completed.push(step);
                if step.primary {
                    primary_ok = true;
                }
            }
            _ => {
                if step.primary {
                    return MoveVerdict::Failed;
                }
                companion_failed = true;
                break;
            }
        }
    }

    if !primary_ok {
        return MoveVerdict::Failed;
    }
    if companion_failed {
        return match rollback(&completed) {
            Rollback::Restored => MoveVerdict::Failed,
            Rollback::Dirty => MoveVerdict::Partial(MovedPhoto {
                old_id: target.id.clone(),
                new_path,
                new_live_path: new_live,
            }),
        };
    }
    MoveVerdict::Moved(MovedPhoto {
        old_id: target.id.clone(),
        new_path,
        new_live_path: new_live,
    })
}

struct UniqueNames {
    photo: String,
    live: Option<String>,
}

fn unique_names(target: &MutationTarget, dest: &Path) -> UniqueNames {
    let photo = file_name(&target.path);
    let live = target
        .live_photo_video_path
        .as_ref()
        .map(|path| file_name(path));
    if !is_taken(dest, &photo, live.as_deref()) {
        return UniqueNames { photo, live };
    }
    let (stem, ext) = split_name(&photo);
    let mut n = 2;
    loop {
        let numbered = format!("{stem} {n}");
        let next = if ext.is_empty() {
            numbered.clone()
        } else {
            format!("{numbered}.{ext}")
        };
        let live_next = live.as_ref().map(|name| restem(name, &numbered));
        if !is_taken(dest, &next, live_next.as_deref()) {
            return UniqueNames {
                photo: next,
                live: live_next,
            };
        }
        n += 1;
    }
}

fn move_plan(target: &MutationTarget, dest: &Path, names: &UniqueNames) -> Vec<MoveStep> {
    let new_path = dest.join(&names.photo);
    let mut steps = vec![MoveStep {
        from: target.path.clone(),
        to: new_path.clone(),
        primary: true,
    }];
    for (from, to) in sidecar_pair(&target.path)
        .into_iter()
        .zip(sidecar_pair(&new_path))
    {
        steps.push(MoveStep {
            from,
            to,
            primary: false,
        });
    }
    if let (Some(live), Some(name)) = (&target.live_photo_video_path, &names.live) {
        let dest_live = dest.join(name);
        steps.push(MoveStep {
            from: live.clone(),
            to: dest_live.clone(),
            primary: false,
        });
        for (from, to) in sidecar_pair(live).into_iter().zip(sidecar_pair(&dest_live)) {
            steps.push(MoveStep {
                from,
                to,
                primary: false,
            });
        }
    }
    steps
}

enum Rollback {
    Restored,
    Dirty,
}

fn rollback(completed: &[&MoveStep]) -> Rollback {
    let mut restored = true;
    for step in completed.iter().rev() {
        match relocate(&step.to, &step.from) {
            Ok(()) => {
                if step.to.exists() && !step.from.exists() {
                    restored = false;
                }
            }
            Err(_) => restored = false,
        }
    }
    if restored {
        Rollback::Restored
    } else {
        Rollback::Dirty
    }
}

fn relocate(from: &Path, to: &Path) -> io::Result<()> {
    if !from.exists() {
        return Ok(());
    }
    if let (Ok(a), Ok(b)) = (from.canonicalize(), to.canonicalize()) {
        if a == b {
            return Ok(());
        }
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(error) if is_exdev(&error) => {
            if from.is_file() {
                fs::copy(from, to)?;
                fs::remove_file(from)?;
                Ok(())
            } else {
                Err(error)
            }
        }
        Err(error) => Err(error),
    }
}

fn is_exdev(error: &io::Error) -> bool {
    error.raw_os_error() == Some(libc::EXDEV)
}

fn is_taken(dest: &Path, name: &str, live: Option<&str>) -> bool {
    taken_path(&dest.join(name)) || live.is_some_and(|name| taken_path(&dest.join(name)))
}

fn taken_path(path: &Path) -> bool {
    if path.exists() {
        return true;
    }
    sidecar_pair(path).iter().any(|sidecar| sidecar.exists())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("photo")
        .to_string()
}

fn split_name(name: &str) -> (String, String) {
    match Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| !ext.is_empty())
    {
        Some(ext) => {
            let stem = Path::new(name)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or(name);
            (stem.to_string(), ext.to_string())
        }
        None => (name.to_string(), String::new()),
    }
}

fn restem(filename: &str, new_stem: &str) -> String {
    let ext = Path::new(filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");
    if ext.is_empty() {
        new_stem.to_string()
    } else {
        format!("{new_stem}.{ext}")
    }
}

pub fn create_subfolder(
    parent: &Path,
    name: &str,
    library_root: &Path,
) -> Result<PathBuf, MutateError> {
    let name = sanitize_folder_name(name)?;
    if !dest_is_under_root(parent, library_root) {
        return Err(MutateError::DestOutsideLibrary);
    }
    if !parent.is_dir() {
        return Err(MutateError::DestNotDirectory);
    }
    let dest = parent.join(name);
    if !dest_is_under_root(&dest, library_root) {
        return Err(MutateError::DestOutsideLibrary);
    }
    fs::create_dir_all(&dest)?;
    if !dest_is_under_root(&dest, library_root) {
        let _ = fs::remove_dir(&dest);
        return Err(MutateError::DestOutsideLibrary);
    }
    dest.canonicalize().map_err(MutateError::Io)
}

fn sanitize_folder_name(name: &str) -> Result<&str, MutateError> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." {
        return Err(MutateError::InvalidFolderName);
    }
    if name.contains('/') || name.contains('\0') {
        return Err(MutateError::InvalidFolderName);
    }
    Ok(name)
}

pub fn export_photos(
    targets: &[MutationTarget],
    dest_dir: &Path,
    quality: ShareQuality,
) -> Result<ExportResult, MutateError> {
    if !dest_dir.is_dir() {
        return Err(MutateError::DestNotDirectory);
    }
    let mut result = ExportResult::default();
    for target in targets {
        match export_one(target, dest_dir, quality) {
            Ok(path) => result.saved.push(path),
            Err(_) => result.failed.push(target.id.clone()),
        }
    }
    Ok(result)
}

fn export_one(
    target: &MutationTarget,
    dest_dir: &Path,
    quality: ShareQuality,
) -> Result<PathBuf, MutateError> {
    if target.is_video || quality == ShareQuality::Original {
        let name = unique_export_name(dest_dir, &file_name(&target.path));
        let dest = dest_dir.join(name);
        fs::copy(&target.path, &dest)?;
        return Ok(dest);
    }
    let Some(max_side) = quality.max_side() else {
        return export_one(target, dest_dir, ShareQuality::Original);
    };
    let Some(text) = target.path.to_str() else {
        return Err(MutateError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "photo path is not UTF-8",
        )));
    };
    let Some(frame) = decode_limited(text, max_side) else {
        return Err(MutateError::Io(io::Error::other("decode failed")));
    };
    let stem = target
        .path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("photo");
    let name = unique_export_name(dest_dir, &format!("{stem}-{}.jpg", quality.slug()));
    let dest = dest_dir.join(name);
    write_jpeg(&dest, &frame, quality.jpeg_quality())?;
    Ok(dest)
}

fn unique_export_name(dest_dir: &Path, name: &str) -> String {
    if !dest_dir.join(name).exists() {
        return name.to_string();
    }
    let (stem, ext) = split_name(name);
    let mut n = 2;
    loop {
        let next = if ext.is_empty() {
            format!("{stem} {n}")
        } else {
            format!("{stem} {n}.{ext}")
        };
        if !dest_dir.join(&next).exists() {
            return next;
        }
        n += 1;
    }
}

fn write_jpeg(path: &Path, frame: &RgbFrame, quality: u8) -> Result<(), MutateError> {
    let image = image::RgbImage::from_raw(frame.width, frame.height, frame.rgb.clone())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "RGB buffer does not match size")
        })?;
    let mut file = fs::File::create(path)?;
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, quality);
    encoder
        .encode(
            image.as_raw(),
            frame.width,
            frame.height,
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|error| io::Error::other(error))?;
    file.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::tests_jpeg;

    fn target(id: &str, path: &Path) -> MutationTarget {
        MutationTarget {
            id: id.to_string(),
            path: path.to_path_buf(),
            is_video: false,
            live_photo_video_path: None,
        }
    }

    fn write_photo(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, tests_jpeg()).unwrap();
        path
    }

    #[test]
    fn delete_primary_and_both_sidecar_spellings() {
        let tmp = tempfile::tempdir().unwrap();
        let photo = write_photo(tmp.path(), "IMG_1.jpg");
        let canon = PathBuf::from(sidecar_path(photo.to_str().unwrap()));
        let alt = PathBuf::from(alt_sidecar_path(photo.to_str().unwrap()).unwrap());
        fs::write(&canon, b"<xmp/>").unwrap();
        fs::write(&alt, b"<xmp/>").unwrap();
        let missing_ok = write_photo(tmp.path(), "IMG_2.jpg");

        let result = delete_photos(&[target("a", &photo), target("b", &missing_ok)]);
        assert_eq!(result.deleted_ids, vec!["a".to_string(), "b".to_string()]);
        assert!(result.failed.is_empty());
        assert!(result.partial_ids.is_empty());
        assert!(!result.needs_reconciliation);
        assert!(!photo.exists());
        assert!(!canon.exists());
        assert!(!alt.exists());
        assert!(!missing_ok.exists());
    }

    #[test]
    fn delete_primary_gone_leftover_sidecar_is_partial() {
        let tmp = tempfile::tempdir().unwrap();
        let photo = write_photo(tmp.path(), "gone.jpg");
        let sidecar = PathBuf::from(sidecar_path(photo.to_str().unwrap()));
        fs::create_dir(&sidecar).unwrap();

        let result = delete_photos(&[target("gone", &photo)]);
        assert!(result.deleted_ids.contains(&"gone".to_string()));
        assert!(result.partial_ids.contains(&"gone".to_string()));
        assert!(result.needs_reconciliation);
        assert!(!photo.exists());
        assert!(sidecar.exists());
    }

    #[test]
    fn delete_missing_or_undeletable_primary_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("no-file.jpg");
        let as_dir = tmp.path().join("not-a-file.jpg");
        fs::create_dir(&as_dir).unwrap();

        let result = delete_photos(&[target("missing", &missing), target("dir", &as_dir)]);
        assert!(result.failed.contains(&"missing".to_string()));
        assert!(result.failed.contains(&"dir".to_string()));
        assert!(result.deleted_ids.is_empty());
        assert!(as_dir.is_dir());
    }

    #[test]
    fn move_into_dest_under_root() {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("album");
        fs::create_dir(&dest).unwrap();
        let photo = write_photo(root.path(), "shot.jpg");
        let sidecar = PathBuf::from(sidecar_path(photo.to_str().unwrap()));
        fs::write(&sidecar, b"<xmp/>").unwrap();

        let result = move_photos(&[target("p1", &photo)], &dest, root.path()).unwrap();
        assert_eq!(result.moved.len(), 1);
        assert!(result.failed.is_empty());
        assert!(result.skipped.is_empty());
        assert!(!photo.exists());
        assert!(!sidecar.exists());
        let moved = dest.join("shot.jpg");
        assert!(moved.is_file());
        assert!(PathBuf::from(sidecar_path(moved.to_str().unwrap())).is_file());
        assert_eq!(result.moved[0].new_path, moved);
    }

    #[test]
    fn move_dest_outside_root_is_error_and_untouched() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let photo = write_photo(root.path(), "keep.jpg");
        let bytes = fs::read(&photo).unwrap();

        let err = move_photos(&[target("p1", &photo)], outside.path(), root.path()).unwrap_err();
        assert!(matches!(err, MutateError::DestOutsideLibrary));
        assert!(photo.is_file());
        assert_eq!(fs::read(&photo).unwrap(), bytes);
        assert!(outside.path().read_dir().unwrap().next().is_none());
    }

    #[test]
    fn move_already_in_dest_is_skipped() {
        let root = tempfile::tempdir().unwrap();
        let photo = write_photo(root.path(), "here.jpg");
        let result = move_photos(&[target("p1", &photo)], root.path(), root.path()).unwrap();
        assert_eq!(result.skipped, vec!["p1".to_string()]);
        assert!(result.moved.is_empty());
        assert!(photo.is_file());
    }

    #[test]
    fn create_subfolder_rejects_invalid_and_escaping_names() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        assert!(matches!(
            create_subfolder(root.path(), "", root.path()),
            Err(MutateError::InvalidFolderName)
        ));
        assert!(matches!(
            create_subfolder(root.path(), "..", root.path()),
            Err(MutateError::InvalidFolderName)
        ));
        assert!(matches!(
            create_subfolder(root.path(), "a/b", root.path()),
            Err(MutateError::InvalidFolderName)
        ));
        assert!(matches!(
            create_subfolder(outside.path(), "sneak", root.path()),
            Err(MutateError::DestOutsideLibrary)
        ));
        let made = create_subfolder(root.path(), "Trip", root.path()).unwrap();
        assert!(made.starts_with(root.path().canonicalize().unwrap()));
        assert!(made.is_dir());
    }

    #[test]
    fn export_original_copies_bytes_and_high_writes_jpeg() {
        let root = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let photo = write_photo(root.path(), "src.jpg");
        let before = fs::read(&photo).unwrap();

        let copied =
            export_photos(&[target("e1", &photo)], dest.path(), ShareQuality::Original).unwrap();
        assert_eq!(copied.saved.len(), 1);
        assert_eq!(fs::read(&copied.saved[0]).unwrap(), before);
        assert_eq!(fs::read(&photo).unwrap(), before);

        let resized =
            export_photos(&[target("e1", &photo)], dest.path(), ShareQuality::High).unwrap();
        assert_eq!(resized.saved.len(), 1);
        let out = &resized.saved[0];
        assert_eq!(
            out.file_name().and_then(|n| n.to_str()),
            Some("src-high.jpg")
        );
        let jpeg = fs::read(out).unwrap();
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        assert_eq!(fs::read(&photo).unwrap(), before);
        assert_ne!(jpeg, before);
    }

    #[test]
    fn dest_under_root_uses_canonical_paths() {
        let root = tempfile::tempdir().unwrap();
        let child = root.path().join("nested");
        fs::create_dir(&child).unwrap();
        assert!(dest_is_under_root(&child, root.path()));
        let outside = tempfile::tempdir().unwrap();
        assert!(!dest_is_under_root(outside.path(), root.path()));
        let photos = root.path().join("photos");
        let backup = root.path().join("photos-backup");
        fs::create_dir(&photos).unwrap();
        fs::create_dir(&backup).unwrap();
        assert!(dest_is_under_root(&photos, &photos));
        assert!(!dest_is_under_root(&backup, &photos));
    }

    #[test]
    fn move_collision_uses_numbered_name() {
        let root = tempfile::tempdir().unwrap();
        let dest = root.path().join("album");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("shot.jpg"), b"taken").unwrap();
        let photo = write_photo(root.path(), "shot.jpg");
        let result = move_photos(&[target("p1", &photo)], &dest, root.path()).unwrap();
        assert_eq!(result.moved.len(), 1);
        assert_eq!(result.moved[0].new_path, dest.join("shot 2.jpg"));
        assert!(!photo.exists());
        assert!(dest.join("shot.jpg").is_file());
        assert!(dest.join("shot 2.jpg").is_file());
    }
}
