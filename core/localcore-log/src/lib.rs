//! Append-only NDJSON event log.
//!
//! Files are append-only. Never rewrite, edit, or delete a line.
//! Layout: `{root}/log/<dev>/YYYY-MM.ndjson`
//!
//! Ported from health `internal/log` + `internal/event`. Gallery person-state
//! types are accepted in the schema so one log serves both consumers. The Go
//! packages stay until Phase 6. Gallery writes under `{library}/.gallery`
//! (M2); this crate's `root` is that directory.

#![forbid(unsafe_code)]

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde_json::Error as JsonError;

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

pub mod event;
pub mod gallery;

pub use event::{
    known_type, new_event_id, now_utc, parse_blob_import, ts_with_nanos, valid_device, BlobImport,
    Event, TS_FORMAT, TYPE_BLOB_IMPORT, TYPE_EXTRACTION, TYPE_FEATURED_PHOTO_CLEAR,
    TYPE_FEATURED_PHOTO_SET, TYPE_MEDITATION, TYPE_MED_EVENT, TYPE_MED_START, TYPE_MED_STOP,
    TYPE_NOTE, TYPE_OBSERVATION, TYPE_PERSON_CONTACT_LINK_CLEAR, TYPE_PERSON_CONTACT_LINK_SET,
    TYPE_PERSON_FEATURED, TYPE_PERSON_HIDDEN, TYPE_PERSON_ME_CLEAR, TYPE_PERSON_ME_SET,
    TYPE_PERSON_RENAMED, TYPE_PERSON_UNFEATURED, TYPE_PERSON_UNHIDDEN, TYPE_RETRACT,
    TYPE_SUPERSEDE,
};
pub use gallery::{
    append_person, is_person_event_type, migrate_from_snapshot, migrate_from_snapshot_json,
    project_people, project_people_at, PersonSnapshot, PeopleState,
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

impl From<JsonError> for Error {
    fn from(e: JsonError) -> Self {
        Error::Invalid(e.to_string())
    }
}

fn mkdir_private(path: &Path) -> io::Result<()> {
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    b.mode(0o700);
    b.create(path)
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    let d = File::open(dir)?;
    d.sync_all()
}

/// Write one event line to `log/<dev>/YYYY-MM.ndjson`.
pub fn append(root: impl AsRef<Path>, ev: &Event) -> Result<()> {
    ev.validate()?;
    let month = ev.month();
    if month.is_empty() {
        return Err(Error::Invalid("event ts has no month".into()));
    }
    let dir = root.as_ref().join("log").join(&ev.dev);
    mkdir_private(&dir)?;
    let line = ev.marshal_line()?;
    let path = dir.join(format!("{month}.ndjson"));
    let created = match fs::metadata(&path) {
        Ok(_) => false,
        Err(e) if e.kind() == io::ErrorKind::NotFound => true,
        Err(e) => return Err(e.into()),
    };
    let mut opts = OpenOptions::new();
    opts.append(true).create(true).write(true);
    #[cfg(unix)]
    opts.mode(0o600);
    let mut f = opts.open(&path)?;
    f.write_all(&line)?;
    f.sync_all()?;
    if created {
        sync_dir(&dir)?;
    }
    Ok(())
}

/// Walk `log/*/*.ndjson` and return events sorted by `(ts, id)`.
pub fn read_all(root: impl AsRef<Path>) -> Result<Vec<Event>> {
    let dir = root.as_ref().join("log");
    match fs::metadata(&dir) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
        Ok(_) => {}
    }
    let mut files = Vec::new();
    walk_ndjson(&dir, &mut files)?;
    let mut out = Vec::new();
    for path in files {
        out.extend(read_file(&path)?);
    }
    out.sort_by(|a, b| a.ts.cmp(&b.ts).then_with(|| a.id.cmp(&b.id)));
    Ok(out)
}

fn walk_ndjson(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for ent in fs::read_dir(dir)? {
        let ent = ent?;
        let path = ent.path();
        if ent.file_type()?.is_dir() {
            walk_ndjson(&path, out)?;
            continue;
        }
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".ndjson"))
        {
            out.push(path);
        }
    }
    Ok(())
}

fn read_file(path: &Path) -> Result<Vec<Event>> {
    let f = File::open(path)?;
    let mut br = BufReader::with_capacity(64 * 1024, f);
    let mut out = Vec::new();
    let mut offset = 0u64;
    let mut line_no = 0usize;
    loop {
        let mut line = Vec::new();
        let n = br.read_until(b'\n', &mut line)?;
        if n == 0 {
            break;
        }
        let has_nl = line.last() == Some(&b'\n');
        let raw = line.trim_ascii();
        if raw.is_empty() {
            offset += n as u64;
            continue;
        }
        line_no += 1;
        match serde_json::from_slice::<Event>(raw) {
            Ok(ev) => out.push(ev),
            Err(jerr) => {
                if !has_nl {
                    return Err(Error::TornTail(TornTail {
                        path: path.to_path_buf(),
                        offset,
                        err: jerr,
                    }));
                }
                return Err(Error::Json {
                    path: path.to_path_buf(),
                    line: line_no,
                    source: jerr,
                });
            }
        }
        offset += n as u64;
    }
    Ok(out)
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
