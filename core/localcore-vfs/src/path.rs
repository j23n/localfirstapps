//! Lexical path helpers shared by the confined VFS and by app cores.
//!
//! `/` and `\` are both separators. Nothing here talks to the filesystem.

/// Join `root` and `child` with one separator.
///
/// Trailing separators on `root` are trimmed. The separator is `/` unless
/// `root` is Windows-backslash style (contains `\` and no `/`), matching
/// `localcore-conflict`'s `join_under`.
///
/// `child` is appended as given. A `..` or empty segment is not rewritten
/// here; [`is_under_root`] on the result rejects a `..` that leaves `root`.
#[must_use]
pub fn join(root: &str, child: &str) -> String {
    let windows = is_windows_backslash_style(root);
    let trimmed = root.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        if root.is_empty() {
            return child.to_owned();
        }
        let sep = if windows { '\\' } else { '/' };
        return format!("{sep}{child}");
    }
    if windows {
        format!(r"{trimmed}\{child}")
    } else {
        format!("{trimmed}/{child}")
    }
}

/// Strip `root` and one leading separator. If `path` does not start with
/// `root`, return `path` unchanged.
#[must_use]
pub fn relative_to(root: &str, path: &str) -> String {
    let root = root.trim_end_matches(['/', '\\']);
    path.strip_prefix(root)
        .and_then(|rest| rest.strip_prefix(['/', '\\']))
        .unwrap_or(path)
        .to_owned()
}

/// Leading `/` or `\`, or a Windows drive `X:`.
#[must_use]
pub fn is_absolute(path: &str) -> bool {
    path.starts_with(['/', '\\'])
        || path
            .as_bytes()
            .get(1)
            .is_some_and(|separator| *separator == b':')
}

/// True when the lexically normalized `path` stays inside `root`.
///
/// `.` and empty segments are dropped, `..` pops a component, and a `..`
/// that would leave `root` fails. The path need not exist.
#[must_use]
pub fn is_under_root(root: &str, path: &str) -> bool {
    let root_n = normalize(root);
    let path_n = normalize(path);
    if root_n.drive != path_n.drive || root_n.absolute != path_n.absolute {
        return false;
    }
    path_n.components.starts_with(&root_n.components)
}

fn is_windows_backslash_style(path: &str) -> bool {
    path.contains('\\') && !path.contains('/')
}

#[derive(Debug, PartialEq, Eq)]
struct Normalized {
    drive: Option<String>,
    absolute: bool,
    components: Vec<String>,
}

fn normalize(path: &str) -> Normalized {
    let drive = path
        .as_bytes()
        .get(1)
        .is_some_and(|b| *b == b':')
        .then(|| path[..2].to_ascii_uppercase());
    let rest = if drive.is_some() { &path[2..] } else { path };
    let absolute = drive.is_some() || rest.starts_with(['/', '\\']);
    let mut components = Vec::new();
    for part in rest.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                if !components.is_empty() {
                    components.pop();
                } else if !absolute {
                    components.push("..".into());
                }
            }
            other => components.push(other.to_string()),
        }
    }
    Normalized {
        drive,
        absolute,
        components,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_uses_slash_unless_the_root_is_windows_backslash_style() {
        assert_eq!(join("/lib", "a.jpg"), "/lib/a.jpg");
        assert_eq!(join("/lib/", "a.jpg"), "/lib/a.jpg");
        assert_eq!(join("/lib\\", "a.jpg"), "/lib/a.jpg");
        assert_eq!(join("/", "a.jpg"), "/a.jpg");
        assert_eq!(join("", "a.jpg"), "a.jpg");
        assert_eq!(join(r"C:\photos", "a.jpg"), r"C:\photos\a.jpg");
        assert_eq!(join(r"C:\photos\", "a.jpg"), r"C:\photos\a.jpg");
        assert_eq!(join(r"C:\", "a.jpg"), r"C:\a.jpg");
        assert_eq!(join("C:/photos/", "a.jpg"), "C:/photos/a.jpg");
    }

    #[test]
    fn relative_to_strips_a_root_prefix() {
        assert_eq!(relative_to("/lib", "/lib/a.jpg"), "a.jpg");
        assert_eq!(relative_to("/lib/", "/lib/nested/a.jpg"), "nested/a.jpg");
        assert_eq!(relative_to(r"C:\lib\", r"C:\lib\a.jpg"), "a.jpg");
        assert_eq!(relative_to("/lib", "/other/a.jpg"), "/other/a.jpg");
        assert_eq!(relative_to("/lib", "/lib"), "/lib");
    }

    #[test]
    fn is_absolute_accepts_slashes_and_drive_letters() {
        assert!(is_absolute("/a"));
        assert!(is_absolute(r"\a"));
        assert!(is_absolute(r"C:\a"));
        assert!(is_absolute("D:/a"));
        assert!(is_absolute("C:"));
        assert!(!is_absolute("a/b"));
        assert!(!is_absolute(""));
        assert!(!is_absolute("C"));
    }

    #[test]
    fn is_under_root_keeps_descendants_and_rejects_escapes() {
        assert!(is_under_root("/lib", "/lib"));
        assert!(is_under_root("/lib/", "/lib/a.jpg"));
        assert!(is_under_root("/lib", "/lib/nested/a.jpg"));
        assert!(is_under_root("/lib", "/lib/foo/../bar.jpg"));
        assert!(is_under_root(r"C:\lib", r"C:\lib\a.jpg"));
        assert!(is_under_root(r"c:\lib", r"C:\lib\a.jpg"));

        assert!(!is_under_root("/lib", "/other"));
        assert!(!is_under_root("/lib", "/library/a.jpg"));
        assert!(!is_under_root("/lib", "/lib/../etc/passwd"));
        assert!(!is_under_root("/lib", &join("/lib", "..")));
        assert!(!is_under_root("/lib", &join("/lib", "../secret.txt")));
        assert!(!is_under_root("/lib", &join("/lib", "foo/../..")));
        assert!(!is_under_root(r"C:\lib", r"C:\lib\..\Windows"));
        assert!(!is_under_root(r"C:\lib", r"D:\lib\a.jpg"));
        assert!(!is_under_root("/lib", "a.jpg"));
    }
}
