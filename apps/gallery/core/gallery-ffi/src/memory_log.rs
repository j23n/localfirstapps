//! Memory-chrome event log over [`localcore_log`].
//!
//! On disk, under the library root (same files as the person log):
//!
//! `{library}/.gallery/log/<dev>/YYYY-MM.ndjson`
//!
//! `localcore-log` is given `{library}/.gallery` as its root. The device id
//! stays in UserDefaults (ADR 0005 R5) and is not written here. Memory
//! types are independent of person-state types.

use std::path::{Path, PathBuf};

use localcore_log::{
    append_memory, migrate_memories_from_snapshot_json, project_memories_at,
    project_memories_report_at, read_all_json, MemoryState,
};

use crate::person_log::GALLERY_STATE_DIR;

fn log_root(library: &str) -> PathBuf {
    Path::new(library).join(GALLERY_STATE_DIR)
}

/// Why a memory-log call failed.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum MemoryLogError {
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

impl std::fmt::Display for MemoryLogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryLogError::Invalid { detail } => write!(f, "{detail}"),
            MemoryLogError::Io { path, detail } => write!(f, "io {path}: {detail}"),
            MemoryLogError::Json { detail } => write!(f, "json: {detail}"),
            MemoryLogError::TornTail {
                path,
                offset,
                detail,
            } => write!(f, "torn tail {path} at {offset}: {detail}"),
        }
    }
}

impl std::error::Error for MemoryLogError {}

impl From<localcore_log::Error> for MemoryLogError {
    fn from(e: localcore_log::Error) -> Self {
        match e {
            localcore_log::Error::Invalid(detail) => MemoryLogError::Invalid { detail },
            localcore_log::Error::Io(err) => MemoryLogError::Io {
                path: String::new(),
                detail: err.to_string(),
            },
            localcore_log::Error::Json { path, line, source } => MemoryLogError::Json {
                detail: format!("{}:{line}: {source}", path.display()),
            },
            localcore_log::Error::TornTail(t) => MemoryLogError::TornTail {
                path: t.path.display().to_string(),
                offset: t.offset,
                detail: t.err.to_string(),
            },
            localcore_log::Error::Vfs(err) => MemoryLogError::Io {
                path: err.path().to_owned(),
                detail: err.to_string(),
            },
        }
    }
}

/// One key/timestamp pair in a projected memory-chrome map.
///
/// R6 role: structure DTO.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MemoryDatePair {
    pub key: String,
    pub at: String,
}

/// Projected memory-chrome state after replaying `.gallery/log`.
///
/// `generated_day` is empty when unset. `birthdays_enabled` defaults to
/// true when the log has no `memory_birthdays_set` event.
///
/// R6 role: structure DTO.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MemoryStateStructure {
    pub hidden: Vec<String>,
    pub seen: Vec<MemoryDatePair>,
    pub surfaced: Vec<MemoryDatePair>,
    pub birthdays_enabled: bool,
    pub generated_day: String,
}

impl Default for MemoryStateStructure {
    fn default() -> Self {
        Self {
            hidden: Vec::new(),
            seen: Vec::new(),
            surfaced: Vec::new(),
            birthdays_enabled: true,
            generated_day: String::new(),
        }
    }
}

/// One recovered torn final line. Complete events before this offset were
/// projected and remain authoritative.
///
/// R6 role: structure DTO.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MemoryTornTailRecord {
    pub path: String,
    pub offset: u64,
    pub detail: String,
}

/// Projected state plus non-fatal append-only-log diagnostics.
///
/// R6 role: structure DTO.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MemoryProjectionRecord {
    pub state: MemoryStateStructure,
    pub torn_tails: Vec<MemoryTornTailRecord>,
}

impl From<MemoryState> for MemoryStateStructure {
    fn from(state: MemoryState) -> Self {
        Self {
            hidden: state.hidden.into_iter().collect(),
            seen: state
                .seen
                .into_iter()
                .map(|(key, at)| MemoryDatePair { key, at })
                .collect(),
            surfaced: state
                .surfaced
                .into_iter()
                .map(|(key, at)| MemoryDatePair { key, at })
                .collect(),
            birthdays_enabled: state.birthdays_enabled,
            generated_day: state.generated_day.unwrap_or_default(),
        }
    }
}

/// Append one memory-chrome operation. `body_json` is a JSON object.
#[uniffi::export]
pub fn memory_log_append(
    root: String,
    device: String,
    event_type: String,
    body_json: String,
) -> Result<(), MemoryLogError> {
    let log = log_root(&root);
    localcore_trace::event(
        "memories",
        format!(
            "memory_log_append type={event_type} device={device} library={root} log={}",
            log.display()
        ),
    );
    match append_memory(&log, &device, &event_type, &body_json) {
        Ok(()) => {
            localcore_trace::event(
                "memories",
                format!("memory_log_append ok type={event_type}"),
            );
            Ok(())
        }
        Err(error) => {
            localcore_trace::event(
                "memories",
                format!("memory_log_append failed type={event_type}: {error}"),
            );
            Err(error.into())
        }
    }
}

/// Replay every device file under `{root}/.gallery/log`.
#[uniffi::export]
pub fn memory_log_project(root: String) -> Result<MemoryStateStructure, MemoryLogError> {
    Ok(MemoryStateStructure::from(
        project_memories_at(log_root(&root)).map_err(MemoryLogError::from)?,
    ))
}

/// Replay complete events and return any ignored torn final lines. Hosts must
/// surface these diagnostics and must not fall back to a stale local snapshot.
#[uniffi::export]
pub fn memory_log_project_report(root: String) -> Result<MemoryProjectionRecord, MemoryLogError> {
    let log = log_root(&root);
    match project_memories_report_at(&log) {
        Ok(report) => {
            let record = MemoryProjectionRecord {
                state: report.state.into(),
                torn_tails: report
                    .torn_tails
                    .into_iter()
                    .map(|tail| MemoryTornTailRecord {
                        path: tail.path.display().to_string(),
                        offset: tail.offset,
                        detail: tail.detail,
                    })
                    .collect(),
            };
            localcore_trace::event(
                "memories",
                format!(
                    "memory_log_project library={root} hidden={} seen={} surfaced={} birthdays={} torn={}",
                    record.state.hidden.len(),
                    record.state.seen.len(),
                    record.state.surfaced.len(),
                    record.state.birthdays_enabled,
                    record.torn_tails.len()
                ),
            );
            Ok(record)
        }
        Err(error) => {
            localcore_trace::event(
                "memories",
                format!("memory_log_project failed library={root}: {error}"),
            );
            Err(MemoryLogError::from(error))
        }
    }
}

/// One-shot import of the five UserDefaults keys. Returns events written
/// (0 once this device has a `memory_migrated` marker).
#[uniffi::export]
pub fn memory_log_migrate_from_snapshot(
    root: String,
    device: String,
    snapshot_json: String,
) -> Result<u32, MemoryLogError> {
    let n = migrate_memories_from_snapshot_json(log_root(&root), &device, &snapshot_json)?;
    Ok(n as u32)
}

/// JSON array of every event under `{root}/.gallery/log`, sorted by (ts, id).
#[uniffi::export]
pub fn memory_log_read(root: String) -> Result<String, MemoryLogError> {
    read_all_json(log_root(&root)).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn m2_dump() -> String {
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "../../../../core/localcore-log/tests/fixtures/m2/userdefaults-memory-state.json",
        ))
        .unwrap()
    }

    #[test]
    fn layout_is_dot_gallery_log() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_string_lossy().into_owned();
        memory_log_append(
            root.clone(),
            "ios".into(),
            "memory_hidden".into(),
            r#"{"id":"onThisDay-2024-06-11"}"#.into(),
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
        assert!(line.contains("\"type\":\"memory_hidden\""), "{line}");
        assert!(line.contains("onThisDay-2024-06-11"), "{line}");
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
        let n = memory_log_migrate_from_snapshot(root.clone(), "ios".into(), dump).unwrap();
        assert_eq!(n, 6);
        let state = memory_log_project(root.clone()).unwrap();
        assert_eq!(state.hidden, vec!["onThisDay-2024-06-11".to_string()]);
        assert_eq!(
            state.seen,
            vec![MemoryDatePair {
                key: "onThisDay-2023-01-01".into(),
                at: "2024-06-01T12:00:00Z".into(),
            }]
        );
        assert_eq!(
            state.surfaced,
            vec![MemoryDatePair {
                key: "onThisDay".into(),
                at: "2024-06-10T08:00:00Z".into(),
            }]
        );
        assert!(!state.birthdays_enabled);
        assert_eq!(state.generated_day, "2024-06-11T00:00:00Z");
        assert_eq!(
            memory_log_migrate_from_snapshot(root, "ios".into(), m2_dump()).unwrap(),
            0
        );
    }

    #[test]
    fn append_then_project() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_string_lossy().into_owned();
        memory_log_migrate_from_snapshot(root.clone(), "ios".into(), m2_dump()).unwrap();
        memory_log_append(
            root.clone(),
            "ios".into(),
            "memory_unhidden".into(),
            r#"{"id":"onThisDay-2024-06-11"}"#.into(),
        )
        .unwrap();
        memory_log_append(
            root.clone(),
            "ios".into(),
            "memory_seen".into(),
            r#"{"id":"yearsAgo-5-2024-06-11","at":"2024-07-01T10:00:00Z"}"#.into(),
        )
        .unwrap();
        let state = memory_log_project(root.clone()).unwrap();
        assert!(state.hidden.is_empty());
        assert!(state
            .seen
            .iter()
            .any(|p| p.key == "yearsAgo-5-2024-06-11" && p.at == "2024-07-01T10:00:00Z"));
        let raw = memory_log_read(root).unwrap();
        assert!(raw.contains("memory_unhidden"), "{raw}");
        assert!(raw.contains("memory_seen"), "{raw}");
    }

    #[test]
    fn append_rejects_person_type() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_string_lossy().into_owned();
        let err = memory_log_append(
            root,
            "ios".into(),
            "person_hidden".into(),
            r#"{"path":"People/Ada"}"#.into(),
        )
        .unwrap_err();
        match err {
            MemoryLogError::Invalid { detail } => {
                assert!(detail.contains("unknown memory event type"), "{detail}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn report_surfaces_torn_tail_with_recovered_projection() {
        let tmp = tempfile::tempdir().unwrap();
        let device_dir = tmp.path().join(".gallery/log/phone");
        fs::create_dir_all(&device_dir).unwrap();
        let path = device_dir.join("2024-07.ndjson");
        let complete = concat!(
            "{\"id\":\"01900000-0000-7000-8000-0000000000f1\",",
            "\"ts\":\"2024-07-01T10:00:00.000000000Z\",",
            "\"dev\":\"phone\",\"type\":\"memory_hidden\",",
            "\"body\":{\"id\":\"onThisDay-recovered\"}}\n"
        );
        fs::write(&path, format!("{complete}{{\"id\":\"torn\"")).unwrap();

        let report = memory_log_project_report(tmp.path().to_string_lossy().into_owned()).unwrap();
        assert_eq!(report.state.hidden, vec!["onThisDay-recovered".to_string()]);
        assert_eq!(report.torn_tails.len(), 1);
        assert_eq!(report.torn_tails[0].path, path.display().to_string());
        assert_eq!(report.torn_tails[0].offset, complete.len() as u64);
        assert!(!report.torn_tails[0].detail.is_empty());
    }
}
