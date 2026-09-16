//! Playlist parsing and canonical atomic serialization.
//!
//! Unknown M3U comments/directives and unknown PLS fields are retained as
//! opaque lines. Canonical writes normalize line endings and order those
//! preserved lines, but do not silently discard them.

use std::collections::BTreeMap;

use localcore_vfs::Vfs;
use sha2::{Digest, Sha256};

use crate::model::{Playlist, PlaylistEntry, PlaylistFormat, Track};
use crate::path::{file_stem, parent, resolve_audio};
use crate::StoreError;

/// Parse one playlist according to its filename extension.
pub fn parse_playlist(path: &str, bytes: &[u8]) -> Result<Playlist, StoreError> {
    let format = PlaylistFormat::from_extension(
        path.rsplit_once('.').map_or("", |(_, extension)| extension),
    )
    .ok_or_else(|| StoreError::InvalidCommand("Unsupported playlist extension".into()))?;
    let text = decode(path, bytes, format)?;
    let (parsed_entries, preserved_lines) = match format {
        PlaylistFormat::M3u | PlaylistFormat::M3u8 => parse_m3u(&text),
        PlaylistFormat::Pls => parse_pls(&text),
    };
    let mut playlist = Playlist {
        id: localcore_id::derive(path).to_string(),
        path: path.to_owned(),
        name: file_stem(path),
        format,
        entries: parsed_entries
            .into_iter()
            .map(|parsed| PlaylistEntry {
                id: String::new(),
                resolved_path: resolve_audio(&parsed.raw_path, &parent(path)),
                raw_path: parsed.raw_path,
                track_id: None,
                directives: parsed.directives,
                pls_fields: parsed.pls_fields,
            })
            .collect(),
        preserved_lines,
        content_token: content_token(bytes),
    };
    refresh_entry_ids(&mut playlist);
    Ok(playlist)
}

fn decode(path: &str, bytes: &[u8], format: PlaylistFormat) -> Result<String, StoreError> {
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    match String::from_utf8(bytes.to_vec()) {
        Ok(text) => Ok(text),
        Err(error) if format != PlaylistFormat::M3u8 => {
            Ok(error.into_bytes().into_iter().map(char::from).collect())
        }
        Err(_) => Err(StoreError::InvalidPlaylist {
            path: path.into(),
            message: "The file is not valid UTF-8".into(),
        }),
    }
}

struct ParsedEntry {
    raw_path: String,
    directives: Vec<String>,
    pls_fields: Vec<(String, String)>,
}

fn parse_m3u(text: &str) -> (Vec<ParsedEntry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut preserved = Vec::new();
    let mut pending_directives = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_matches('\r').trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            if !line.eq_ignore_ascii_case("#EXTM3U") {
                if is_entry_directive(line) {
                    pending_directives.push(line.to_owned());
                } else {
                    preserved.push(line.to_owned());
                }
            }
        } else {
            entries.push(ParsedEntry {
                raw_path: line.to_owned(),
                directives: std::mem::take(&mut pending_directives),
                pls_fields: Vec::new(),
            });
        }
    }
    preserved.append(&mut pending_directives);
    (entries, preserved)
}

fn is_entry_directive(line: &str) -> bool {
    let upper = line.to_ascii_uppercase();
    upper.starts_with("#EXTINF:") || upper.starts_with("#EXTVLCOPT:")
}

fn parse_pls(text: &str) -> (Vec<ParsedEntry>, Vec<String>) {
    let mut entries = BTreeMap::<usize, String>::new();
    let mut indexed_fields = BTreeMap::<usize, Vec<(String, String)>>::new();
    let mut preserved = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_matches('\r').trim();
        if line.is_empty() || line.eq_ignore_ascii_case("[playlist]") {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            preserved.push(line.to_owned());
            continue;
        };
        let lower = key.to_ascii_lowercase();
        if let Some(index) = lower.strip_prefix("file").and_then(|n| n.parse().ok()) {
            entries.insert(index, value.trim().to_owned());
        } else if lower != "numberofentries" && lower != "version" {
            if let Some((field, index)) = split_pls_index(key) {
                indexed_fields
                    .entry(index)
                    .or_default()
                    .push((field.to_owned(), value.trim().to_owned()));
            } else {
                preserved.push(line.to_owned());
            }
        }
    }
    let parsed = entries
        .into_iter()
        .map(|(index, raw_path)| {
            let mut pls_fields = indexed_fields.remove(&index).unwrap_or_default();
            pls_fields.sort();
            ParsedEntry {
                raw_path,
                directives: Vec::new(),
                pls_fields,
            }
        })
        .collect();
    for (index, fields) in indexed_fields {
        preserved.extend(
            fields
                .into_iter()
                .map(|(field, value)| format!("{field}{index}={value}")),
        );
    }
    (parsed, preserved)
}

fn split_pls_index(key: &str) -> Option<(&str, usize)> {
    let split = key
        .char_indices()
        .find(|(_, character)| character.is_ascii_digit())
        .map(|(index, _)| index)?;
    let (field, number) = key.split_at(split);
    if field.is_empty() {
        None
    } else {
        number.parse().ok().map(|index| (field, index))
    }
}

/// Re-resolve entry ids against the current track projection.
pub fn hydrate_entries(playlist: &mut Playlist, tracks: &[Track]) {
    let by_path: BTreeMap<&str, &str> = tracks
        .iter()
        .map(|track| (track.path.as_str(), track.id.as_str()))
        .collect();
    for entry in &mut playlist.entries {
        entry.track_id = entry
            .resolved_path
            .as_deref()
            .and_then(|path| by_path.get(path).copied())
            .map(str::to_owned);
    }
    refresh_entry_ids(playlist);
}

/// Recompute per-document entry ids after an ordered edit.
pub fn refresh_entry_ids(playlist: &mut Playlist) {
    for (index, entry) in playlist.entries.iter_mut().enumerate() {
        entry.id =
            localcore_id::derive(&format!("{}\n{}\n{}", playlist.path, index, entry.raw_path))
                .to_string();
    }
}

/// Deterministic bytes for an M3U/M3U8/PLS document.
#[must_use]
pub fn canonical_bytes(playlist: &Playlist) -> Vec<u8> {
    let mut preserved: Vec<&str> = playlist
        .preserved_lines
        .iter()
        .map(String::as_str)
        .filter(|line| !line.trim().is_empty())
        .collect();
    preserved.sort_unstable();
    let mut lines = Vec::new();
    match playlist.format {
        PlaylistFormat::M3u | PlaylistFormat::M3u8 => {
            lines.push("#EXTM3U".to_owned());
            lines.extend(preserved.into_iter().map(str::to_owned));
            for entry in &playlist.entries {
                lines.extend(entry.directives.iter().cloned());
                lines.push(entry.raw_path.clone());
            }
        }
        PlaylistFormat::Pls => {
            lines.push("[playlist]".to_owned());
            for (index, entry) in playlist.entries.iter().enumerate() {
                let number = index + 1;
                lines.push(format!("File{number}={}", entry.raw_path));
                let mut fields: Vec<_> = entry.pls_fields.iter().collect();
                fields.sort_unstable();
                lines.extend(
                    fields
                        .into_iter()
                        .map(|(key, value)| format!("{key}{number}={value}")),
                );
            }
            lines.extend(preserved.into_iter().map(str::to_owned));
            lines.push(format!("NumberOfEntries={}", playlist.entries.len()));
            lines.push("Version=2".into());
        }
    }
    (lines.join("\n") + "\n").into_bytes()
}

/// Canonically replace a playlist through [`Vfs::write_atomic`].
pub fn write_playlist(vfs: &dyn Vfs, playlist: &mut Playlist) -> Result<(), StoreError> {
    let bytes = canonical_bytes(playlist);
    vfs.write_atomic(&playlist.path, &bytes)?;
    playlist.content_token = content_token(&bytes);
    refresh_entry_ids(playlist);
    Ok(())
}

/// SHA-256 token used for deterministic stale-edit refusal.
#[must_use]
pub fn content_token(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Empty canonical playlist.
#[must_use]
pub fn empty_playlist(path: String, format: PlaylistFormat) -> Playlist {
    let mut playlist = Playlist {
        id: localcore_id::derive(&path).to_string(),
        name: file_stem(&path),
        path,
        format,
        entries: Vec::new(),
        preserved_lines: Vec::new(),
        content_token: String::new(),
    };
    refresh_entry_ids(&mut playlist);
    playlist
}
