//! Lexical path policy shared by identity and playlist resolution.

/// Join a folder and one relative component.
#[must_use]
pub fn join(folder: &str, child: &str) -> String {
    if folder.ends_with('/') {
        format!("{folder}{child}")
    } else {
        format!("{folder}/{child}")
    }
}

/// Parent path, or an empty string for a bare filename.
#[must_use]
pub fn parent(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    trimmed.rfind('/').map_or_else(String::new, |index| {
        if index == 0 {
            "/".into()
        } else {
            trimmed[..index].to_owned()
        }
    })
}

/// Final component.
#[must_use]
pub fn basename(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
}

/// Filename without its final extension.
#[must_use]
pub fn file_stem(path: &str) -> String {
    let name = basename(path);
    name.rsplit_once('.')
        .map_or(name, |(stem, _)| stem)
        .to_owned()
}

/// Lexically collapse `.` and `..`, matching the relevant behavior of
/// Foundation's `URL.standardized.path` without resolving symlinks.
#[must_use]
pub fn standardize(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut components: Vec<&str> = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." if components.last().is_some_and(|last| *last != "..") => {
                components.pop();
            }
            ".." if !absolute => components.push(component),
            ".." => {}
            other => components.push(other),
        }
    }
    let joined = components.join("/");
    match (absolute, joined.is_empty()) {
        (true, true) => "/".into(),
        (true, false) => format!("/{joined}"),
        (false, true) => ".".into(),
        (false, false) => joined,
    }
}

/// Resolve a playlist entry when it is a local path with a supported audio
/// extension. HTTP(S) entries remain in the parsed playlist but are not
/// handed to a media host.
#[must_use]
pub fn resolve_audio(raw_path: &str, playlist_dir: &str) -> Option<String> {
    let path = raw_path.trim();
    if path.is_empty()
        || path
            .get(..7)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
        || path
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
    {
        return None;
    }
    let resolved = if path.starts_with('/') {
        standardize(path)
    } else {
        standardize(&join(playlist_dir, path))
    };
    matches!(
        crate::model::classify_name(basename(&resolved)),
        Some(crate::model::FileClass::Audio)
    )
    .then_some(resolved)
}

/// Prefer a path relative to the playlist folder; keep an absolute path when
/// the track is outside it.
#[must_use]
pub fn relative_path(track_path: &str, playlist_dir: &str) -> String {
    let track = standardize(track_path);
    let base = standardize(playlist_dir);
    let prefix = if base == "/" {
        "/".to_owned()
    } else {
        format!("{}/", base.trim_end_matches('/'))
    };
    track
        .strip_prefix(&prefix)
        .filter(|relative| !relative.is_empty())
        .map_or(track.clone(), str::to_owned)
}

/// Path relative to the selected folder for display/opaque ids.
#[must_use]
pub fn relative_to(root: &str, path: &str) -> String {
    let root = root.trim_end_matches('/');
    path.strip_prefix(root)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(path)
        .to_owned()
}
