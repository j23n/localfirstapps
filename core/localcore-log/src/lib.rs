//! Append-only NDJSON event log.
//!
//! Files are append-only. Never rewrite, edit, or delete a line.
//! Layout: `{root}/log/<dev>/YYYY-MM.ndjson`
//! Field order: `id`, `ts`, `dev`, `type`, `body`.
//! `ts` is UTC with 9 fractional digits. Marshal matches Go
//! `SetEscapeHTML(false)` (`<`, `>`, `&` stay literal).
//!
//! Ported from health `internal/log` + `internal/event`. Gallery person-state
//! and memory-chrome types are accepted in the schema so one log serves both
//! consumers. Health Go remains the writer until Phase 6;
//! `tests/health_golden.rs` is the port contract (byte-identical m0 log +
//! `read_all` parity). Gallery writes under `{library}/.gallery` (M2); this
//! crate's `root` is that directory. I/O goes through [`localcore_vfs::Vfs`].
//! Type tokens are open (`valid_type`); `known_type` is not a monorepo enum.

#![forbid(unsafe_code)]

use std::io;
use std::path::{Path, PathBuf};

use localcore_vfs::{EntryKind, StdVfs, Vfs, VfsError};
use serde_json::Error as JsonError;

pub mod event;
pub mod gallery;
pub mod memories;

pub use event::{
    known_type, new_event_id, now_utc, parse_blob_import, ts_with_nanos, valid_device, valid_type,
    BlobImport, Event, TS_FORMAT, TYPE_BLOB_IMPORT, TYPE_EPISODE, TYPE_EXTRACTION,
    TYPE_FEATURED_PHOTO_CLEAR, TYPE_FEATURED_PHOTO_SET, TYPE_MEDITATION, TYPE_MED_EVENT,
    TYPE_MED_START, TYPE_MED_STOP, TYPE_MEMORY_BIRTHDAYS_SET, TYPE_MEMORY_CLUSTER_SURFACED,
    TYPE_MEMORY_GENERATED_DAY, TYPE_MEMORY_GENERATED_DAY_CLEAR, TYPE_MEMORY_HIDDEN,
    TYPE_MEMORY_MIGRATED, TYPE_MEMORY_SEEN, TYPE_MEMORY_UNHIDDEN, TYPE_NOTE, TYPE_OBSERVATION,
    TYPE_PERSON_CONTACT_LINK_CLEAR, TYPE_PERSON_CONTACT_LINK_SET, TYPE_PERSON_FEATURED,
    TYPE_PERSON_HIDDEN, TYPE_PERSON_ME_CLEAR, TYPE_PERSON_ME_SET, TYPE_PERSON_MIGRATED,
    TYPE_PERSON_RENAMED, TYPE_PERSON_UNFEATURED, TYPE_PERSON_UNHIDDEN, TYPE_RETRACT,
    TYPE_SUPERSEDE,
};
pub use gallery::{
    append_person, is_person_event_type, migrate_from_snapshot, migrate_from_snapshot_json,
    project_people, project_people_at, project_people_report_at, PeopleProjection, PeopleState,
    PersonSnapshot,
};
pub use memories::{
    append_memory, is_memory_event_type, migrate_memories_from_snapshot,
    migrate_memories_from_snapshot_json, project_memories, project_memories_at,
    project_memories_report_at, MemoryProjection, MemorySnapshot, MemoryState,
};

/// A final line that lacks a trailing newline and is not valid JSON.
///
/// ADR 0005 R16: a torn tail is a normal state, not mid-file corruption.
/// The append-only log must not be rewritten; callers should report, not repair.
#[derive(Debug)]
pub struct TornTail {
    pub path: PathBuf,
    pub offset: u64,
    pub err: JsonError,
}

/// Events recovered from all complete lines plus any ignored torn tails.
#[derive(Debug)]
pub struct ReadReport {
    pub events: Vec<Event>,
    pub torn_tails: Vec<TornTail>,
}

/// Sendable/display-only form of a torn tail for projections that recover
/// complete events while surfacing the ignored final fragment to a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TornTailDiagnostic {
    pub path: PathBuf,
    pub offset: u64,
    pub detail: String,
}

impl From<&TornTail> for TornTailDiagnostic {
    fn from(value: &TornTail) -> Self {
        Self {
            path: value.path.clone(),
            offset: value.offset,
            detail: value.err.to_string(),
        }
    }
}

impl std::fmt::Display for TornTail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "torn tail {} at offset {}: {}",
            self.path.display(),
            self.offset,
            self.err
        )
    }
}

impl std::error::Error for TornTail {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.err)
    }
}

/// Why a log operation failed.
#[derive(Debug)]
pub enum Error {
    Invalid(String),
    Io(io::Error),
    /// A [`Vfs`] operation failed. The inner type is preserved.
    Vfs(VfsError),
    Json {
        path: PathBuf,
        line: usize,
        source: JsonError,
    },
    TornTail(TornTail),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn as_torn_tail(&self) -> Option<&TornTail> {
        match self {
            Error::TornTail(t) => Some(t),
            _ => None,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Invalid(msg) => write!(f, "{msg}"),
            Error::Io(e) => write!(f, "{e}"),
            Error::Vfs(e) => write!(f, "{e}"),
            Error::Json { path, line, source } => {
                write!(f, "{}:{line}: {source}", path.display())
            }
            Error::TornTail(t) => write!(f, "{t}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Vfs(e) => Some(e),
            Error::Json { source, .. } => Some(source),
            Error::TornTail(t) => Some(t),
            Error::Invalid(_) => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<VfsError> for Error {
    fn from(e: VfsError) -> Self {
        Error::Vfs(e)
    }
}

impl From<JsonError> for Error {
    fn from(e: JsonError) -> Self {
        Error::Invalid(e.to_string())
    }
}

/// Temp prefix for Path wrappers over [`StdVfs`].
pub const TEMP_PREFIX: &str = ".localcore-log-tmp-";

fn std_vfs() -> StdVfs {
    StdVfs::new(TEMP_PREFIX)
}

fn root_str(root: impl AsRef<Path>) -> Result<String> {
    root.as_ref()
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::Invalid("log root is not UTF-8".into()))
}

fn join_root(root: &str, rest: &str) -> String {
    let root = root.trim_end_matches('/');
    if rest.is_empty() {
        root.to_owned()
    } else {
        format!("{root}/{rest}")
    }
}

/// Write one event line to `log/<dev>/YYYY-MM.ndjson`.
pub fn append(root: impl AsRef<Path>, ev: &Event) -> Result<()> {
    append_on(&std_vfs(), &root_str(root)?, ev)
}

/// [`append`] through an explicit [`Vfs`].
pub fn append_on(vfs: &dyn Vfs, root: &str, ev: &Event) -> Result<()> {
    ev.validate()?;
    let month = ev.month();
    if month.is_empty() {
        return Err(Error::Invalid("event ts has no month".into()));
    }
    let dir = join_root(root, &format!("log/{}", ev.dev));
    vfs.create_dir_all(&dir)?;
    let line = ev.marshal_line()?;
    let path = join_root(&dir, &format!("{month}.ndjson"));
    vfs.append(&path, &line)?;
    Ok(())
}

/// [`Event::fresh`] plus [`append_on`]. Used by contacts/music folder logs.
pub fn append_op(
    vfs: &dyn Vfs,
    root: &str,
    device: &str,
    event_type: &str,
    body: serde_json::Value,
) -> Result<Event> {
    let ev = Event::fresh(device, event_type, body);
    append_on(vfs, root, &ev)?;
    Ok(ev)
}

/// Walk `log/*/*.ndjson` and return events sorted by `(ts, id)`.
pub fn read_all(root: impl AsRef<Path>) -> Result<Vec<Event>> {
    Ok(read_report_on(&std_vfs(), &root_str(root)?)?.events)
}

/// [`read_all`] through an explicit [`Vfs`].
pub fn read_all_on(vfs: &dyn Vfs, root: &str) -> Result<Vec<Event>> {
    Ok(read_report_on(vfs, root)?.events)
}

/// Walk a log and return valid events plus diagnostics for ignored torn tails.
pub fn read_report(root: impl AsRef<Path>) -> Result<ReadReport> {
    read_report_on(&std_vfs(), &root_str(root)?)
}

/// [`read_report`] through an explicit [`Vfs`].
pub fn read_report_on(vfs: &dyn Vfs, root: &str) -> Result<ReadReport> {
    let dir = join_root(root, "log");
    if !vfs.try_exists(&dir)? {
        return Ok(ReadReport {
            events: Vec::new(),
            torn_tails: Vec::new(),
        });
    }
    let mut files = Vec::new();
    walk_ndjson(vfs, &dir, &mut files)?;
    let mut events = Vec::new();
    let mut torn_tails = Vec::new();
    for path in files {
        let (mut recovered, torn) = read_file(vfs, &path)?;
        events.append(&mut recovered);
        torn_tails.extend(torn);
    }
    events.sort_by(|a, b| a.ts.cmp(&b.ts).then_with(|| a.id.cmp(&b.id)));
    Ok(ReadReport { events, torn_tails })
}

fn walk_ndjson(vfs: &dyn Vfs, dir: &str, out: &mut Vec<String>) -> Result<()> {
    for ent in vfs.list(dir)? {
        let path = join_root(dir, &ent.name);
        match ent.kind {
            EntryKind::Dir => {
                walk_ndjson(vfs, &path, out)?;
                continue;
            }
            EntryKind::Symlink => continue,
            EntryKind::File => {}
        }
        if ent.name.ends_with(".ndjson") {
            out.push(path);
        }
    }
    Ok(())
}

fn read_file(vfs: &dyn Vfs, path: &str) -> Result<(Vec<Event>, Option<TornTail>)> {
    let bytes = vfs.read(path)?;
    let mut out = Vec::new();
    let mut torn = None;
    let mut offset = 0u64;
    let mut line_no = 0usize;
    let mut rest = bytes.as_slice();
    while !rest.is_empty() {
        let (line, next) = match rest.iter().position(|&b| b == b'\n') {
            Some(i) => (&rest[..=i], &rest[i + 1..]),
            None => (rest, &[][..]),
        };
        let n = line.len();
        let has_nl = line.last() == Some(&b'\n');
        let raw = line.trim_ascii();
        if raw.is_empty() {
            offset += n as u64;
            rest = next;
            continue;
        }
        line_no += 1;
        match serde_json::from_slice::<Event>(raw) {
            Ok(ev) => out.push(ev),
            Err(jerr) => {
                if !has_nl {
                    torn = Some(TornTail {
                        path: PathBuf::from(path),
                        offset,
                        err: jerr,
                    });
                    break;
                }
                return Err(Error::Json {
                    path: PathBuf::from(path),
                    line: line_no,
                    source: jerr,
                });
            }
        }
        offset += n as u64;
        rest = next;
    }
    Ok((out, torn))
}

/// `read_all` as a JSON array. Used by the gallery FFI read helper.
pub fn read_all_json(root: impl AsRef<Path>) -> Result<String> {
    Ok(serde_json::to_string(&read_all(root)?)?)
}

/// Whether a `blob_import` for `sha256` already exists.
pub fn has_blob_import(root: impl AsRef<Path>, sha256: &str) -> Result<bool> {
    let evs = read_all(root)?;
    let want = sha256.to_ascii_lowercase();
    for ev in evs {
        if ev.event_type != TYPE_BLOB_IMPORT {
            continue;
        }
        if ev.blob_sha256().as_deref() == Some(want.as_str()) {
            return Ok(true);
        }
    }
    Ok(false)
}
