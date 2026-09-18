//! Pack root enumeration. [`gallery_ml::resolve_model_pack`] is the rule.

use std::fs;
use std::path::{Path, PathBuf};

use gallery_ml::{resolve_model_pack, PackSource};
use serde::Deserialize;

const MANIFEST: &str = "manifest.json";

/// Where we look for versioned pack directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRoots {
    /// `/usr/share/localgallery/pack` and a source-tree `build/pack`.
    pub bundled: Vec<PathBuf>,
    /// `$XDG_DATA_HOME/localgallery/pack` (or the iOS imported root).
    pub imported: Vec<PathBuf>,
}

/// A resolved pack, plus the cheap facts Settings can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackStatus {
    /// Directory that holds `manifest.json`.
    pub directory: PathBuf,
    /// Last path component (the version name used for resolution).
    pub name: String,
    /// Bundled vs user-imported.
    pub source: PackSource,
    /// `pack_version` from the manifest, if present.
    pub version: String,
    /// Whether the manifest declares face models.
    pub has_faces: bool,
}

impl PackStatus {
    /// Source as Settings shows it.
    pub fn source_label(&self) -> &'static str {
        match self.source {
            PackSource::Bundled => "Bundled",
            PackSource::Imported => "Imported",
        }
    }
}

/// Default search roots, plus `$LOCALGALLERY_PACK` when set.
pub fn default_roots() -> PackRoots {
    let mut bundled = vec![PathBuf::from("/usr/share/localgallery/pack")];
    if let Ok(cwd) = std::env::current_dir() {
        bundled.push(cwd.join("build/pack"));
        bundled.push(cwd.join("../build/pack"));
    }
    bundled.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("build")
            .join("pack"),
    );

    let mut imported = vec![data_pack_root()];
    if let Ok(extra) = std::env::var("LOCALGALLERY_PACK") {
        if !extra.is_empty() {
            imported.insert(0, PathBuf::from(extra));
        }
    }

    PackRoots { bundled, imported }
}

/// `$XDG_DATA_HOME/localgallery/pack`.
pub fn data_pack_root() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("localgallery")
        .join("pack")
}

/// Newest usable pack under the default roots.
pub fn discover_pack() -> Option<PackStatus> {
    resolve_in(&default_roots())
}

/// Pack the product UI uses: XDG install, optional `$LOCALGALLERY_PACK`,
/// and a distro copy under `/usr/share`. Checkout `build/pack` is a
/// **source** for [`download_pack`], not an install.
pub fn installed_pack() -> Option<PackStatus> {
    resolve_in(&product_roots())
}

/// Whether `~/.local/share/localgallery/pack` currently holds a pack.
pub fn xdg_pack_present() -> bool {
    !collect_candidates(&[data_pack_root()]).is_empty()
}

/// Where Download looks for the one gitignored checkout pack.
pub fn checkout_pack_dir() -> Option<PathBuf> {
    newest_pack_dir(&default_checkout_roots())
}

/// How the one product pack is obtained. Checkout copy today; HTTPS later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackFetch {
    /// Copy `build/model_packs` / `build/pack` into the XDG install root.
    Checkout,
}

/// Why Download / Remove failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackInstallError {
    /// No checkout pack (and no HTTPS source yet).
    NoSource,
    /// Filesystem copy or remove failed.
    Io(String),
}

impl std::fmt::Display for PackInstallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSource => formatter.write_str(
                "No model pack in this checkout. Build one with apps/gallery/scripts/build_model_pack.",
            ),
            Self::Io(detail) => formatter.write_str(detail),
        }
    }
}

impl std::error::Error for PackInstallError {}

/// Install the one pack. Today this copies the checkout weights; a later
/// [`PackFetch`] variant is the HTTPS path.
pub fn download_pack() -> Result<PackStatus, PackInstallError> {
    fetch_pack(PackFetch::Checkout)
}

/// Fetch via `origin` into [`data_pack_root`]. Replaces any previous XDG pack.
pub fn fetch_pack(origin: PackFetch) -> Result<PackStatus, PackInstallError> {
    match origin {
        PackFetch::Checkout => {
            let source = checkout_pack_dir().ok_or(PackInstallError::NoSource)?;
            install_pack_from(&source, &data_pack_root())
        }
    }
}

/// Copy `source` (a pack directory with `manifest.json`) into `dest_root`
/// as its version-named child. `dest_root` holds exactly one pack.
pub fn install_pack_from(source: &Path, dest_root: &Path) -> Result<PackStatus, PackInstallError> {
    if !has_manifest(source) {
        return Err(PackInstallError::Io(format!(
            "not a model pack: {}",
            source.display()
        )));
    }
    let name = file_name(source).to_string();
    if name.is_empty() {
        return Err(PackInstallError::Io("pack directory has no name".into()));
    }
    if dest_root.exists() {
        fs::remove_dir_all(dest_root).map_err(|error| PackInstallError::Io(error.to_string()))?;
    }
    let dest = dest_root.join(&name);
    copy_tree(source, &dest)?;
    status_from_dir(dest, PackSource::Imported)
        .ok_or_else(|| PackInstallError::Io("copied pack has no readable manifest".into()))
}

/// Delete the XDG install. Distro `/usr/share` is left alone.
pub fn remove_installed_pack() -> Result<(), PackInstallError> {
    remove_pack_at(&data_pack_root())
}

/// Delete every pack under `dest_root`.
pub fn remove_pack_at(dest_root: &Path) -> Result<(), PackInstallError> {
    if dest_root.exists() {
        fs::remove_dir_all(dest_root).map_err(|error| PackInstallError::Io(error.to_string()))?;
    }
    Ok(())
}

fn product_roots() -> PackRoots {
    let mut imported = vec![data_pack_root()];
    if let Ok(extra) = std::env::var("LOCALGALLERY_PACK") {
        if !extra.is_empty() {
            imported.insert(0, PathBuf::from(extra));
        }
    }
    PackRoots {
        bundled: vec![PathBuf::from("/usr/share/localgallery/pack")],
        imported,
    }
}

fn default_checkout_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut push_pair = |base: PathBuf| {
        roots.push(base.join("model_packs"));
        roots.push(base.join("pack"));
    };
    if let Ok(cwd) = std::env::current_dir() {
        push_pair(cwd.join("build"));
        push_pair(cwd.join("apps/gallery/build"));
        if let Some(parent) = cwd.parent() {
            push_pair(parent.join("build"));
            push_pair(parent.join("apps/gallery/build"));
        }
    }
    push_pair(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("build"),
    );
    roots
}

fn newest_pack_dir(roots: &[PathBuf]) -> Option<PathBuf> {
    let candidates = collect_candidates(roots);
    let names = names_of(&candidates);
    let picked = resolve_model_pack(&names, &[])?;
    candidates
        .into_iter()
        .find(|path| file_name(path) == picked.name.as_str())
}

fn status_from_dir(directory: PathBuf, source: PackSource) -> Option<PackStatus> {
    let peek = peek_manifest(&directory)?;
    Some(PackStatus {
        name: file_name(&directory).to_string(),
        directory,
        source,
        version: peek.pack_version.unwrap_or_default(),
        has_faces: peek.faces.is_some(),
    })
}

fn copy_tree(src: &Path, dest: &Path) -> Result<(), PackInstallError> {
    fs::create_dir_all(dest).map_err(|error| PackInstallError::Io(error.to_string()))?;
    for entry in fs::read_dir(src).map_err(|error| PackInstallError::Io(error.to_string()))? {
        let entry = entry.map_err(|error| PackInstallError::Io(error.to_string()))?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| PackInstallError::Io(error.to_string()))?;
        if file_type.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            fs::copy(&from, &to).map_err(|error| PackInstallError::Io(error.to_string()))?;
        }
    }
    Ok(())
}

/// Resolve against explicit roots (tests / iOS injected lists).
pub fn resolve_in(roots: &PackRoots) -> Option<PackStatus> {
    let bundled = collect_candidates(&roots.bundled);
    let imported = collect_candidates(&roots.imported);
    let picked = resolve_model_pack(&names_of(&bundled), &names_of(&imported))?;
    let pool = match picked.source {
        PackSource::Imported => &imported,
        PackSource::Bundled => &bundled,
    };
    let directory = pool
        .iter()
        .find(|p| file_name(p) == picked.name.as_str())?
        .clone();
    let peek = peek_manifest(&directory)?;
    Some(PackStatus {
        directory,
        name: picked.name,
        source: picked.source,
        version: peek.pack_version.unwrap_or_default(),
        has_faces: peek.faces.is_some(),
    })
}

/// Whether this binary was built with the ONNX tagging/faces path.
pub fn ml_enabled() -> bool {
    cfg!(feature = "ml")
}

fn names_of(dirs: &[PathBuf]) -> Vec<String> {
    dirs.iter().map(|p| file_name(p).to_string()).collect()
}

fn file_name(path: &Path) -> &str {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
}

fn collect_candidates(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        if has_manifest(root) {
            out.push(root.clone());
            continue;
        }
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && has_manifest(&path) {
                out.push(path);
            }
        }
    }
    out
}

fn has_manifest(dir: &Path) -> bool {
    dir.join(MANIFEST).is_file()
}

#[derive(Debug, Deserialize)]
struct ManifestPeek {
    #[serde(default)]
    pack_version: Option<String>,
    #[serde(default)]
    faces: Option<serde_json::Value>,
}

fn peek_manifest(dir: &Path) -> Option<ManifestPeek> {
    let bytes = fs::read(dir.join(MANIFEST)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_manifest(dir: &Path, version: &str, faces: bool) {
        fs::create_dir_all(dir).unwrap();
        let faces_json = if faces {
            r#", "faces": {"detector": {}}"#
        } else {
            ""
        };
        fs::write(
            dir.join(MANIFEST),
            format!(r#"{{"schema":1,"pack_version":"{version}"{faces_json}}}"#),
        )
        .unwrap();
    }

    #[test]
    fn newest_imported_beats_older_bundled() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let imported = tmp.path().join("imported");
        write_manifest(&bundled.join("pack-v1.9"), "v1.9", false);
        write_manifest(&imported.join("pack-v1.10"), "v1.10", true);
        let status = resolve_in(&PackRoots {
            bundled: vec![bundled],
            imported: vec![imported],
        })
        .expect("pack");
        assert_eq!(status.name, "pack-v1.10");
        assert_eq!(status.source, PackSource::Imported);
        assert!(status.has_faces);
    }

    #[test]
    fn checkout_picks_the_newest_named_pack() {
        let tmp = tempfile::tempdir().unwrap();
        write_manifest(
            &tmp.path().join("model_packs").join("pack-v1.9"),
            "v1.9",
            false,
        );
        write_manifest(&tmp.path().join("pack").join("pack-v1.10"), "v1.10", true);
        let found = newest_pack_dir(&[tmp.path().join("model_packs"), tmp.path().join("pack")])
            .expect("pack");
        assert_eq!(file_name(&found), "pack-v1.10");
    }

    #[test]
    fn install_replaces_the_previous_xdg_pack() {
        let tmp = tempfile::tempdir().unwrap();
        let source_old = tmp.path().join("src/old-v1");
        let source_new = tmp.path().join("src/new-v2");
        let dest = tmp.path().join("xdg");
        write_manifest(&source_old, "old", false);
        write_manifest(&source_new, "new", true);
        fs::write(source_new.join("weights.bin"), b"onnx").unwrap();

        install_pack_from(&source_old, &dest).unwrap();
        assert!(dest.join("old-v1/manifest.json").is_file());

        let status = install_pack_from(&source_new, &dest).unwrap();
        assert_eq!(status.name, "new-v2");
        assert_eq!(status.version, "new");
        assert!(status.has_faces);
        assert!(dest.join("new-v2/weights.bin").is_file());
        assert!(!dest.join("old-v1").exists(), "dest holds one pack");

        remove_pack_at(&dest).unwrap();
        assert!(!dest.exists());
    }

    #[test]
    fn fetch_checkout_without_a_source_is_no_source() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(newest_pack_dir(&[tmp.path().join("empty")]).is_none());
    }

    #[test]
    fn product_roots_omit_checkout_build() {
        let roots = product_roots();
        assert!(roots
            .bundled
            .iter()
            .all(|path| !path.ends_with("build/pack")));
        assert!(roots
            .imported
            .iter()
            .any(|path| path.ends_with("localgallery/pack")));
    }
}
