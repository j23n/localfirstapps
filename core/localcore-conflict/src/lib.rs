//! Syncthing conflict-copy grammar (ADR 0005 R7, ADR 0002 R7).
//!
//! A file matching
//! `<base>.sync-conflict-<YYYYMMDD>-<HHMMSS>-<device>.<ext>` is **never
//! content**. Callers exclude it from indexing, identity, enrichment, and
//! playback, and surface it only as a member of a [`ConflictGroup`].
//!
//! # Identity
//!
//! Conflict copies **never receive a stable id**. A [`ConflictGroup`] has an
//! opaque, directory-qualified key so two equal basenames in different
//! folders cannot collide, but the copies themselves are not content
//! identities. An app that derives ids from paths (ADR 0002 R4) must skip
//! every name this crate accepts.
//!
//! Resolution policy (ADR 0005 R8–R11) lives in each app core, not here.
//!
//! ```
//! use localcore_conflict::{groups, is_conflict_name, parse_name};
//!
//! let name = "IMG_1234.sync-conflict-20200901-120000-DEVICEABC.heic";
//! assert!(is_conflict_name(name));
//! let copy = parse_name(name).unwrap();
//! assert_eq!(copy.base_stem, "IMG_1234");
//! assert_eq!(copy.extension, "heic");
//! assert_eq!(copy.origin_device, "DEVICEABC");
//!
//! let grouped = groups([name]);
//! assert_eq!(grouped[0].canonical_name, "IMG_1234.heic");
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::cmp::Ordering;
use std::collections::BTreeMap;

/// Syncthing inserts this marker immediately before the final extension.
const MARKER: &str = ".sync-conflict-";

/// One Syncthing conflict copy, parsed from its filename.
///
/// No id is assigned. The surviving original (the name without this marker)
/// is not represented here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictCopy {
    /// Full path if the input was a path, otherwise the name as given.
    pub path: String,
    /// Basename of [`Self::path`].
    pub name: String,
    /// Logical name without the conflict suffix, including inner dots
    /// (`photo.jpg`, `IMG_1234`).
    pub base_stem: String,
    /// Final extension, lowercased, without the dot.
    pub extension: String,
    /// `YYYYMMDD` taken from the filename (not validated as a calendar date).
    pub date: String,
    /// `HHMMSS` taken from the filename (not validated as a clock time).
    pub time: String,
    /// Origin device id from the filename.
    ///
    /// Same charset as health event device ids:
    /// `[A-Za-z0-9][A-Za-z0-9._-]*`.
    pub origin_device: String,
}

/// Conflict copies that share a surviving basename.
///
/// `canonical_name` is the name **without** the conflict marker — the file
/// that remains after Syncthing renamed the losing write. This group lists
/// only the copies, never that original.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictGroup {
    /// Surviving file basename: `{base_stem}.{extension}`, or `base_stem`
    /// alone when the extension is empty.
    pub canonical_name: String,
    /// Directory that contains the copies and the surviving file, as
    /// given on the paths passed to [`groups`]. Empty when the path
    /// was a bare basename. Two `photo.jpg` files in different
    /// folders are two groups.
    pub dir: String,
    /// Copies sharing [`Self::canonical_name`] *in [`Self::dir`]*, sorted by
    /// `(date, time, origin_device, name, path)`.
    pub copies: Vec<ConflictCopy>,
}

impl ConflictGroup {
    /// Opaque group key: `dir/canonical_name`, or just the basename when
    /// `dir` is empty.
    #[must_use]
    pub fn id(&self) -> String {
        join_under(&self.dir, &self.canonical_name)
    }

    /// Path of the surviving file. Relative directories are resolved under
    /// `root`; absolute directories are already rooted.
    #[must_use]
    pub fn surviving_path(&self, root: &str) -> String {
        if self.dir.is_empty() || is_absolute_dir(&self.dir) {
            join_under(
                if self.dir.is_empty() { root } else { &self.dir },
                &self.canonical_name,
            )
        } else {
            join_under(root, &join_under(&self.dir, &self.canonical_name))
        }
    }
}

fn join_under(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else if dir.ends_with(['/', '\\']) {
        format!("{dir}{name}")
    } else if dir.contains('\\') && !dir.contains('/') {
        format!(r"{dir}\{name}")
    } else {
        format!("{dir}/{name}")
    }
}

fn is_absolute_dir(dir: &str) -> bool {
    dir.starts_with(['/', '\\'])
        || dir
            .as_bytes()
            .get(1)
            .is_some_and(|separator| *separator == b':')
}

/// Parse a basename. [`ConflictCopy::path`] equals `name`.
#[must_use]
pub fn parse_name(name: &str) -> Option<ConflictCopy> {
    parse(name, name)
}

/// Parse a path using its basename. [`ConflictCopy::path`] is `path` as given.
#[must_use]
pub fn parse_path(path: &str) -> Option<ConflictCopy> {
    parse(path, basename(path))
}

/// `true` when `name` is a Syncthing conflict copy.
#[must_use]
pub fn is_conflict_name(name: &str) -> bool {
    parse_name(name).is_some()
}

/// Group conflict copies that share `(directory, base_stem, extension)`.
///
/// Non-conflict paths are ignored — the surviving original is not a member.
/// Groups are sorted by [`ConflictGroup::id`]; copies inside a group are
/// sorted by `(date, time, origin_device, name, path)`. The result does
/// not depend on walk order.
#[must_use]
pub fn groups<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<ConflictGroup> {
    let mut by_key: BTreeMap<(String, String, String), Vec<ConflictCopy>> = BTreeMap::new();
    for path in paths {
        if let Some(copy) = parse_path(path) {
            let dir = parent_dir(&copy.path).to_owned();
            by_key
                .entry((dir, copy.base_stem.clone(), copy.extension.clone()))
                .or_default()
                .push(copy);
        }
    }

    let mut out: Vec<ConflictGroup> = by_key
        .into_iter()
        .map(|((dir, base_stem, extension), mut copies)| {
            copies.sort_by(copy_order);
            ConflictGroup {
                canonical_name: canonical_name(&base_stem, &extension),
                dir,
                copies,
            }
        })
        .collect();
    out.sort_by(|a, b| a.id().cmp(&b.id()));
    out
}

fn parent_dir(path: &str) -> &str {
    match path.rfind(['/', '\\']) {
        Some(0) => &path[..1],
        Some(i) => &path[..i],
        _ => "",
    }
}

fn copy_order(a: &ConflictCopy, b: &ConflictCopy) -> Ordering {
    a.date
        .cmp(&b.date)
        .then_with(|| a.time.cmp(&b.time))
        .then_with(|| a.origin_device.cmp(&b.origin_device))
        .then_with(|| a.name.cmp(&b.name))
        .then_with(|| a.path.cmp(&b.path))
}

fn canonical_name(base_stem: &str, extension: &str) -> String {
    if extension.is_empty() {
        base_stem.to_owned()
    } else {
        format!("{base_stem}.{extension}")
    }
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(path)
}

fn parse(path: &str, name: &str) -> Option<ConflictCopy> {
    let marker_at = name.rfind(MARKER)?;
    let base_stem = &name[..marker_at];
    if base_stem.is_empty() {
        return None;
    }
    let rest = &name[marker_at + MARKER.len()..];
    let (date, time, origin_device, extension) = parse_suffix(rest)?;
    Some(ConflictCopy {
        path: path.to_owned(),
        name: name.to_owned(),
        base_stem: base_stem.to_owned(),
        extension,
        date,
        time,
        origin_device,
    })
}

/// `YYYYMMDD-HHMMSS-<device>.<ext>`
fn parse_suffix(rest: &str) -> Option<(String, String, String, String)> {
    // 8 date + '-' + 6 time + '-' + device + '.' + ext
    if rest.len() < 8 + 1 + 6 + 1 + 1 + 1 + 1 {
        return None;
    }
    let date = rest.get(..8)?;
    if !is_digits(date) {
        return None;
    }
    if rest.as_bytes().get(8) != Some(&b'-') {
        return None;
    }
    let time = rest.get(9..15)?;
    if !is_digits(time) {
        return None;
    }
    if rest.as_bytes().get(15) != Some(&b'-') {
        return None;
    }
    let device_and_ext = rest.get(16..)?;
    let dot = device_and_ext.rfind('.')?;
    let origin_device = &device_and_ext[..dot];
    let extension = &device_and_ext[dot + 1..];
    if extension.is_empty() || !is_device_id(origin_device) {
        return None;
    }
    Some((
        date.to_owned(),
        time.to_owned(),
        origin_device.to_owned(),
        extension.to_lowercase(),
    ))
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Health event device ids: `[A-Za-z0-9][A-Za-z0-9._-]*`.
fn is_device_id(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_name_sets_path_equal_to_name() {
        let name = "IMG_1234.sync-conflict-20200901-120000-DEVICEABC.heic";
        let copy = parse_name(name).unwrap();
        assert_eq!(copy.path, name);
        assert_eq!(copy.name, name);
        assert_eq!(copy.base_stem, "IMG_1234");
        assert_eq!(copy.extension, "heic");
        assert_eq!(copy.date, "20200901");
        assert_eq!(copy.time, "120000");
        assert_eq!(copy.origin_device, "DEVICEABC");
    }

    #[test]
    fn parse_path_uses_basename_and_keeps_the_full_path() {
        let path = "/library/nested/photo.jpg.sync-conflict-20200901-120000-DEVICEABC.xmp";
        let copy = parse_path(path).unwrap();
        assert_eq!(copy.path, path);
        assert_eq!(
            copy.name,
            "photo.jpg.sync-conflict-20200901-120000-DEVICEABC.xmp"
        );
        assert_eq!(copy.base_stem, "photo.jpg");
        assert_eq!(copy.extension, "xmp");
    }

    #[test]
    fn parse_path_accepts_backslash_separators() {
        let path = r"C:\photos\alice.sync-conflict-20200901-120000-DEVICEABC.vcf";
        let copy = parse_path(path).unwrap();
        assert_eq!(
            copy.name,
            "alice.sync-conflict-20200901-120000-DEVICEABC.vcf"
        );
        assert_eq!(copy.base_stem, "alice");
    }

    #[test]
    fn extension_is_lowercased() {
        let copy = parse_name("IMG_1234.sync-conflict-20200901-120000-DEVICEABC.HEIC").unwrap();
        assert_eq!(copy.extension, "heic");
        assert_eq!(
            groups(["IMG_1234.sync-conflict-20200901-120000-DEVICEABC.HEIC"])[0].canonical_name,
            "IMG_1234.heic"
        );
    }

    #[test]
    fn same_basename_in_two_directories_is_two_groups() {
        let grouped = groups([
            "/lib/2024/photo.sync-conflict-20200901-120000-PHONE01.jpg",
            "/lib/Archive/photo.sync-conflict-20200901-120000-PHONE01.jpg",
        ]);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].dir, "/lib/2024");
        assert_eq!(grouped[1].dir, "/lib/Archive");
        assert_eq!(grouped[0].canonical_name, "photo.jpg");
        assert_eq!(grouped[0].id(), "/lib/2024/photo.jpg");
        assert_eq!(grouped[0].surviving_path("/lib"), "/lib/2024/photo.jpg");
    }

    #[test]
    fn relative_directory_resolves_under_root() {
        let group = groups(["Archive/photo.sync-conflict-20200901-120000-PHONE01.jpg"]).remove(0);
        assert_eq!(group.id(), "Archive/photo.jpg");
        assert_eq!(group.surviving_path("/lib"), "/lib/Archive/photo.jpg");
    }

    #[test]
    fn last_marker_wins_when_a_conflict_is_itself_conflicted() {
        let name = "foo.sync-conflict-20200901-120000-AAA.sync-conflict-20200902-130000-BBB.heic";
        let copy = parse_name(name).unwrap();
        assert_eq!(copy.base_stem, "foo.sync-conflict-20200901-120000-AAA");
        assert_eq!(copy.date, "20200902");
        assert_eq!(copy.origin_device, "BBB");
    }
}
