//! Person-state event log over [`localcore_log`].
//!
//! On disk, under the library root:
//!
//! `{library}/.gallery/log/<dev>/YYYY-MM.ndjson`
//!
//! `localcore-log` is given `{library}/.gallery` as its root. The device id
//! stays in UserDefaults (ADR 0005 R5) and is not written here.

use std::path::{Path, PathBuf};

use localcore_log::{
    append_person, migrate_from_snapshot_json, project_people_at, read_all_json, PeopleState,
};

/// Directory name under the library root that owns the synced event log.
pub const GALLERY_STATE_DIR: &str = ".gallery";

fn log_root(library: &str) -> PathBuf {
    Path::new(library).join(GALLERY_STATE_DIR)
}

/// Why a person-log call failed.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum PersonLogError {
    /// Bad device id, unknown type, or a body that is not a JSON object.
    Invalid {
        /// Log text.
        detail: String,
    },
    /// Filesystem said no.
    Io {
        /// Path that failed.
        path: String,
        /// OS message; for logs only.
        detail: String,
    },
    /// A complete line was not JSON.
    Json {
        /// Parser message; for logs only.
        detail: String,
    },
    /// A truncated last line (ADR 0005 R16). Not mid-file corruption.
    TornTail {
        /// File that ended mid-record.
        path: String,
        /// Byte offset of the torn line.
        offset: u64,
        /// Parser message; for logs only.
        detail: String,
    },
}

impl std::fmt::Display for PersonLogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PersonLogError::Invalid { detail } => write!(f, "{detail}"),
            PersonLogError::Io { path, detail } => write!(f, "io {path}: {detail}"),
            PersonLogError::Json { detail } => write!(f, "json: {detail}"),
            PersonLogError::TornTail {
                path,
                offset,
                detail,
            } => write!(f, "torn tail {path} at {offset}: {detail}"),
        }
    }
}

impl std::error::Error for PersonLogError {}

impl From<localcore_log::Error> for PersonLogError {
    fn from(e: localcore_log::Error) -> Self {
        match e {
            localcore_log::Error::Invalid(detail) => PersonLogError::Invalid { detail },
            localcore_log::Error::Io(err) => PersonLogError::Io {
                path: String::new(),
                detail: err.to_string(),
            },
            localcore_log::Error::Json { path, line, source } => PersonLogError::Json {
                detail: format!("{}:{line}: {source}", path.display()),
            },
            localcore_log::Error::TornTail(t) => PersonLogError::TornTail {
                path: t.path.display().to_string(),
                offset: t.offset,
                detail: t.err.to_string(),
            },
        }
    }
}

/// One path-keyed string in a projected person-state map.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PersonKeyedString {
    pub path: String,
    pub value: String,
}

/// Projected people-rail state after replaying `.gallery/log`.
///
/// `me` is empty when unset. `links` values: a contact id, or empty for
/// `PersonLink.disabled`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PersonStateRecord {
    pub hidden: Vec<String>,
    pub featured: Vec<String>,
    pub me: String,
    pub featured_photo: Vec<PersonKeyedString>,
    pub links: Vec<PersonKeyedString>,
}

impl From<PeopleState> for PersonStateRecord {
    fn from(state: PeopleState) -> Self {
        Self {
            hidden: state.hidden.into_iter().collect(),
            featured: state.featured,
            me: state.me.unwrap_or_default(),
            featured_photo: state
                .featured_photo
                .into_iter()
                .map(|(path, value)| PersonKeyedString { path, value })
                .collect(),
            links: state
                .links
                .into_iter()
                .map(|(path, value)| PersonKeyedString { path, value })
                .collect(),
        }
    }
}

/// Append one person-state operation. `body_json` is a JSON object.
#[uniffi::export]
pub fn person_log_append(
    root: String,
    device: String,
    event_type: String,
    body_json: String,
) -> Result<(), PersonLogError> {
    append_person(log_root(&root), &device, &event_type, &body_json).map_err(Into::into)
}

/// Replay every device file under `{root}/.gallery/log`.
#[uniffi::export]
pub fn person_log_project(root: String) -> Result<PersonStateRecord, PersonLogError> {
    Ok(PersonStateRecord::from(
        project_people_at(log_root(&root)).map_err(PersonLogError::from)?,
    ))
}

/// One-shot import of the five UserDefaults keys. Returns events written.
#[uniffi::export]
pub fn person_log_migrate_from_snapshot(
    root: String,
    device: String,
    snapshot_json: String,
) -> Result<u32, PersonLogError> {
    let n = migrate_from_snapshot_json(log_root(&root), &device, &snapshot_json)?;
    Ok(n as u32)
}

/// JSON array of every event under `{root}/.gallery/log`, sorted by (ts, id).
#[uniffi::export]
pub fn person_log_read(root: String) -> Result<String, PersonLogError> {
    read_all_json(log_root(&root)).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn m2_dump() -> String {
        fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../../core/localcore-log/tests/fixtures/m2/userdefaults-person-state.json"),
        )
        .unwrap()
    }

    #[test]
    fn layout_is_dot_gallery_log() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_string_lossy().into_owned();
        person_log_append(
            root.clone(),
            "ios".into(),
            "person_hidden".into(),
            r#"{"path":"People/Ada"}"#.into(),
        )
        .unwrap();
        let month = chrono_month_from_file(&root);
        let path = tmp
            .path()
            .join(".gallery")
            .join("log")
            .join("ios")
            .join(format!("{month}.ndjson"));
        assert!(path.is_file(), "missing {}", path.display());
        let line = fs::read_to_string(&path).unwrap();
        assert!(line.contains("\"type\":\"person_hidden\""), "{line}");
        assert!(line.contains("People/Ada"), "{line}");
        assert!(!line.contains("\\u003c"), "{line}");
    }

    fn chrono_month_from_file(root: &str) -> String {
        let dir = Path::new(root).join(".gallery/log/ios");
        let name = fs::read_dir(&dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .file_name();
        name.to_string_lossy()
            .strip_suffix(".ndjson")
            .unwrap()
            .to_string()
    }

    #[test]
    fn migrate_fixture_projects_to_dump() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_string_lossy().into_owned();
        let dump = m2_dump();
        let n = person_log_migrate_from_snapshot(root.clone(), "ios".into(), dump).unwrap();
        assert_eq!(n, 7);
        let state = person_log_project(root.clone()).unwrap();
        assert_eq!(state.hidden, vec!["People/Anna Schmidt".to_string()]);
        assert_eq!(
            state.featured,
            vec!["People/Ada".to_string(), "People/Cy".to_string()]
        );
        assert_eq!(state.me, "People/Ada");
        assert_eq!(
            state.featured_photo,
            vec![PersonKeyedString {
                path: "People/Ada".into(),
                value: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into(),
            }]
        );
        assert_eq!(
            state.links,
            vec![
                PersonKeyedString {
                    path: "People/Ada".into(),
                    value: "CN:ada-uuid".into(),
                },
                PersonKeyedString {
                    path: "People/Erin Hidden".into(),
                    value: String::new(),
                },
            ]
        );
        assert_eq!(
            person_log_migrate_from_snapshot(root, "ios".into(), m2_dump()).unwrap(),
            0
        );
    }

    #[test]
    fn rename_appends_person_renamed() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_string_lossy().into_owned();
        person_log_migrate_from_snapshot(root.clone(), "ios".into(), m2_dump()).unwrap();
        person_log_append(
            root.clone(),
            "ios".into(),
            "person_renamed".into(),
            r#"{"from":"People/Anna Schmidt","to":"People/Ann Schmidt"}"#.into(),
        )
        .unwrap();
        let state = person_log_project(root.clone()).unwrap();
        assert_eq!(state.hidden, vec!["People/Ann Schmidt".to_string()]);
        let raw = person_log_read(root).unwrap();
        assert!(raw.contains("person_renamed"), "{raw}");
    }
}
