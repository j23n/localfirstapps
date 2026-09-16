//! Portable-v1 export, restore, and read-only fsck.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use localcore_log::{parse_blob_import, Event, TYPE_BLOB_IMPORT};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Portable export format version.
pub const ARCHIVE_VERSION: i32 = 1;

const README: &str = include_str!("portable_readme.md");

/// Tool version written into manifests. Matches the Go CLI so v1 exports
/// stay interchangeable.
pub const TOOL_VERSION: &str = "0.2.0";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub archive_version: i32,
    pub exported_at: String,
    pub tool_version: String,
    pub event_count: usize,
    pub blob_count: usize,
    pub blobs: Vec<ManifestBlob>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestBlob {
    pub sha256: String,
    pub size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub kind: String,
    pub sha256: String,
    pub detail: String,
    pub event_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default)]
pub struct FsckReport {
    pub events: usize,
    pub blobs: usize,
    pub diagnostics: Vec<Diagnostic>,
    pub issues: Vec<Issue>,
}

impl FsckReport {
    pub fn ok(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Write `events.ndjson`, `blobs/`, `manifest.json`, and `README.md`.
pub fn export(root: impl AsRef<Path>, out_dir: impl AsRef<Path>) -> Result<Manifest> {
    let root = root
        .as_ref()
        .canonicalize()
        .unwrap_or_else(|_| root.as_ref().to_path_buf());
    let out_dir = out_dir.as_ref();
    let abs_out = if out_dir.exists() {
        out_dir.canonicalize()?
    } else {
        if let Some(parent) = out_dir.parent() {
            fs::create_dir_all(parent)?;
        }
        out_dir.to_path_buf()
    };
    reject_nested(&root, &abs_out)?;

    let report = localcore_log::read_report(&root)?;
    if !report.torn_tails.is_empty() {
        return Err(Error::Invalid(format!(
            "export: archive has {} torn tail(s)",
            report.torn_tails.len()
        )));
    }
    let listed = localcore_blob::list(&root)?;

    fs::create_dir_all(out_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(out_dir, fs::Permissions::from_mode(0o700));
    }

    write_events(&out_dir.join("events.ndjson"), &report.events)?;

    let mut man_blobs = Vec::with_capacity(listed.len());
    for blob in &listed {
        let dest = localcore_blob::path(out_dir, &blob.sha256)?;
        let size = copy_verified(&blob.path, &dest, &blob.sha256)?;
        man_blobs.push(ManifestBlob {
            sha256: blob.sha256.clone(),
            size: size as i64,
        });
    }

    fs::write(out_dir.join("README.md"), README)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(out_dir.join("README.md"), fs::Permissions::from_mode(0o600));
    }

    let man = Manifest {
        archive_version: ARCHIVE_VERSION,
        exported_at: localcore_log::now_utc(),
        tool_version: TOOL_VERSION.to_string(),
        event_count: report.events.len(),
        blob_count: man_blobs.len(),
        blobs: man_blobs,
    };
    write_manifest(&out_dir.join("manifest.json"), &man)?;
    Ok(man)
}

fn reject_nested(root: &Path, out: &Path) -> Result<()> {
    if out == root || out.starts_with(root) {
        return Err(Error::Invalid(
            "export: -out must not be inside the archive root".into(),
        ));
    }
    if root.starts_with(out) {
        return Err(Error::Invalid(
            "export: -out must not be a parent of the archive root".into(),
        ));
    }
    Ok(())
}

fn write_events(path: &Path, events: &[Event]) -> Result<()> {
    let mut file = File::create(path)?;
    for ev in events {
        file.write_all(&ev.marshal_line()?)?;
    }
    file.sync_all()?;
    Ok(())
}

fn write_manifest(path: &Path, man: &Manifest) -> Result<()> {
    let mut file = File::create(path)?;
    serde_json::to_writer_pretty(&mut file, man)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn copy_verified(src: &Path, dest: &Path, expect: &str) -> Result<u64> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(src, dest)?;
    let (got, size) = localcore_blob::hash_file(dest)?;
    if got != expect {
        let _ = fs::remove_file(dest);
        return Err(Error::Invalid(format!(
            "export: blob {expect} hashed to {got}"
        )));
    }
    Ok(size)
}

/// Replay a portable export into `dest_root`, keeping event ids.
pub fn restore(export_dir: impl AsRef<Path>, dest_root: impl AsRef<Path>) -> Result<()> {
    let export_dir = export_dir.as_ref();
    let dest_root = dest_root.as_ref();
    let listed = localcore_blob::list(export_dir)?;
    for blob in listed {
        let mut file = File::open(&blob.path)?;
        let put = localcore_blob::put(dest_root, &mut file)?;
        if put.hash != blob.sha256 {
            return Err(Error::Invalid(format!(
                "restore: blob {} hashed to {}",
                blob.sha256, put.hash
            )));
        }
    }

    let mut have = std::collections::HashSet::new();
    for ev in localcore_log::read_all(dest_root)? {
        have.insert(ev.id);
    }

    let events_path = export_dir.join("events.ndjson");
    let file = File::open(&events_path)?;
    let reader = BufReader::new(file);
    for (idx, line) in reader.lines().enumerate() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let ev: Event = serde_json::from_str(trimmed)
            .map_err(|err| Error::Invalid(format!("events.ndjson:{}: {err}", idx + 1)))?;
        if have.contains(&ev.id) {
            continue;
        }
        localcore_log::append(dest_root, &ev)?;
        have.insert(ev.id);
    }
    Ok(())
}

/// Verify blob hashes and references. Never repairs.
pub fn fsck(root: impl AsRef<Path>) -> Result<FsckReport> {
    let root = root.as_ref();
    let mut rep = FsckReport::default();
    let log_rep = localcore_log::read_report(root)?;
    for torn in &log_rep.torn_tails {
        rep.diagnostics.push(Diagnostic {
            kind: "torn_tail".into(),
            detail: format!("{} offset={}", torn.path.display(), torn.offset),
        });
    }
    rep.events = log_rep.events.len();
    let listed = localcore_blob::list(root)?;
    rep.blobs = listed.len();

    let on_disk: std::collections::BTreeMap<_, _> =
        listed.into_iter().map(|b| (b.sha256.clone(), b)).collect();
    let mut referenced = std::collections::BTreeSet::new();

    for ev in &log_rep.events {
        match ev.blob_sha256() {
            Some(hash) => {
                referenced.insert(hash.clone());
                if ev.event_type == TYPE_BLOB_IMPORT && !on_disk.contains_key(&hash) {
                    rep.issues.push(Issue {
                        kind: "missing".into(),
                        sha256: hash,
                        detail: String::new(),
                        event_id: ev.id.clone(),
                    });
                }
            }
            None if ev.event_type == TYPE_BLOB_IMPORT => {
                let _ = parse_blob_import(&ev.body);
                rep.issues.push(Issue {
                    kind: "missing".into(),
                    sha256: String::new(),
                    detail: "blob_import has no sha256".into(),
                    event_id: ev.id.clone(),
                });
            }
            None => {}
        }
    }

    let mut orphans = Vec::new();
    for (hash, info) in &on_disk {
        match localcore_blob::hash_file(&info.path) {
            Ok((got, _)) if got == *hash => {}
            Ok((got, _)) => rep.issues.push(Issue {
                kind: "mismatch".into(),
                sha256: hash.clone(),
                detail: format!("content hashes to {got}"),
                event_id: String::new(),
            }),
            Err(err) => rep.issues.push(Issue {
                kind: "mismatch".into(),
                sha256: hash.clone(),
                detail: err.to_string(),
                event_id: String::new(),
            }),
        }
        if !referenced.contains(hash) {
            orphans.push(hash.clone());
        }
    }
    orphans.sort();
    for hash in orphans {
        rep.issues.push(Issue {
            kind: "orphan".into(),
            sha256: hash,
            detail: String::new(),
            event_id: String::new(),
        });
    }
    Ok(rep)
}
