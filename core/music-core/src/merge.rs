//! Preservation-first Syncthing M3U merge (ADR 0005 R8-R11).
//!
//! A plan never writes. Applying a plan is the explicit user decision that
//! permits canonical replacement followed by conflict-copy deletion.

use std::collections::{BTreeMap, BTreeSet};

use localcore_conflict::ConflictGroup;
use localcore_vfs::{write_then_remove_copies, Vfs};
use unicode_normalization::UnicodeNormalization;

use crate::model::{Playlist, PlaylistEntry, PlaylistFormat};
use crate::playlist::{canonical_bytes, parse_playlist, PLAYLIST_READ_CAP};
use crate::StoreError;

/// Typed conflict disposition used by both shells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictDisposition {
    /// Ordered changes occupy disjoint gaps and have a deterministic union.
    Auto,
    /// Two sides changed the same ordered gap or reordered common entries.
    Choice,
    /// The surviving file is absent; a copy keeps user data.
    DeletedVersusModified,
    /// Audio and non-M3U conflicts are visible but not rewritten here.
    ManualOnly,
}

/// Computed merge. It has no filesystem side effects.
#[derive(Debug, Clone)]
pub struct MergePlan {
    /// Automatic, choice, or deleted-versus-modified.
    pub disposition: ConflictDisposition,
    /// Absolute path written on explicit apply.
    pub surviving_path: String,
    /// Absolute conflict-copy paths deleted only after the write.
    pub copy_paths: Vec<String>,
    /// Deterministic disjoint union, or the first source when choice is needed.
    pub merged: Playlist,
    /// Whole-document choices keyed by `surviving` or conflict-copy basename.
    pub sources: BTreeMap<String, Playlist>,
}

/// Parse every source and determine whether ordered changes can be unioned.
pub fn plan_merge(
    vfs: &dyn Vfs,
    root: &str,
    group: &ConflictGroup,
) -> Result<MergePlan, StoreError> {
    let format = PlaylistFormat::from_extension(
        group
            .canonical_name
            .rsplit_once('.')
            .map_or("", |(_, extension)| extension),
    )
    .ok_or(StoreError::UnsupportedConflict)?;
    if format == PlaylistFormat::Pls {
        return Err(StoreError::UnsupportedConflict);
    }

    let surviving_path = group.surviving_path(root);
    let surviving_exists = vfs.try_exists(&surviving_path)?;
    let mut versions = Vec::<(String, Playlist)>::new();
    if surviving_exists {
        versions.push((
            "surviving".into(),
            parse_at(vfs, &surviving_path, &surviving_path)?,
        ));
    }
    let mut copy_paths = Vec::new();
    for copy in &group.copies {
        let path = if crate::path::is_absolute(&copy.path) {
            copy.path.clone()
        } else {
            crate::path::join(root, &copy.path)
        };
        copy_paths.push(path.clone());
        versions.push((copy.name.clone(), parse_at(vfs, &path, &surviving_path)?));
    }
    if versions.is_empty() {
        return Err(StoreError::NotFound);
    }

    let mut merged = versions[0].1.clone();
    let mut needs_choice = false;
    for (_, version) in versions.iter().skip(1) {
        match merge_two(&merged, version) {
            Ok(next) => merged = next,
            Err(_) => needs_choice = true,
        }
    }
    merged.path = surviving_path.clone();
    merged.id = localcore_id::derive(&surviving_path).to_string();
    merged.name = crate::path::file_stem(&surviving_path);
    let disposition = if needs_choice {
        ConflictDisposition::Choice
    } else if !surviving_exists {
        ConflictDisposition::DeletedVersusModified
    } else {
        ConflictDisposition::Auto
    };
    let sources = versions.into_iter().collect();
    Ok(MergePlan {
        disposition,
        surviving_path,
        copy_paths,
        merged,
        sources,
    })
}

fn parse_at(vfs: &dyn Vfs, read_path: &str, canonical_path: &str) -> Result<Playlist, StoreError> {
    let bytes = vfs.read_capped(read_path, PLAYLIST_READ_CAP)?;
    let mut playlist = parse_playlist(canonical_path, &bytes)?;
    playlist.path = canonical_path.to_owned();
    playlist.id = localcore_id::derive(canonical_path).to_string();
    Ok(playlist)
}

/// Canonically write the selected/merged playlist, then remove losing copies.
///
/// `selected_source` is required only for [`ConflictDisposition::Choice`].
pub fn apply_merge(
    vfs: &dyn Vfs,
    plan: &MergePlan,
    selected_source: Option<&str>,
) -> Result<Playlist, StoreError> {
    let mut selected = match plan.disposition {
        ConflictDisposition::Choice => plan
            .sources
            .get(selected_source.ok_or(StoreError::NeedsChoice)?)
            .cloned()
            .ok_or(StoreError::NeedsChoice)?,
        ConflictDisposition::Auto | ConflictDisposition::DeletedVersusModified => {
            plan.merged.clone()
        }
        ConflictDisposition::ManualOnly => return Err(StoreError::UnsupportedConflict),
    };
    selected.path = plan.surviving_path.clone();
    selected.id = localcore_id::derive(&selected.path).to_string();
    selected.name = crate::path::file_stem(&selected.path);
    let bytes = canonical_bytes(&selected);
    write_then_remove_copies(vfs, &plan.surviving_path, &bytes, &plan.copy_paths)?;
    selected.content_token = crate::playlist::content_token(&bytes);
    crate::playlist::refresh_entry_ids(&mut selected);
    Ok(selected)
}

/// Why two playlists cannot be unioned without a whole-document choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeIssue {
    Overlap,
    Reorder,
    DirectiveConflict,
}

/// Merge two ordered documents when additions occupy disjoint gaps between
/// common entries. Duplicate entries are distinguished by occurrence count.
fn merge_two(base: &Playlist, incoming: &Playlist) -> Result<Playlist, MergeIssue> {
    let base_tokens = tokens(&base.entries);
    let incoming_tokens = tokens(&incoming.entries);
    let base_set: BTreeSet<&str> = base_tokens.iter().map(String::as_str).collect();
    let incoming_set: BTreeSet<&str> = incoming_tokens.iter().map(String::as_str).collect();
    let common_base: Vec<&str> = base_tokens
        .iter()
        .map(String::as_str)
        .filter(|token| incoming_set.contains(token))
        .collect();
    let common_incoming: Vec<&str> = incoming_tokens
        .iter()
        .map(String::as_str)
        .filter(|token| base_set.contains(token))
        .collect();
    if common_base != common_incoming {
        return Err(MergeIssue::Reorder);
    }
    let common: BTreeSet<&str> = common_base.iter().copied().collect();
    let base_gaps = gaps(&base.entries, &base_tokens, &common);
    let incoming_gaps = gaps(&incoming.entries, &incoming_tokens, &common);
    let mut output = Vec::new();
    for gap_index in 0..base_gaps.len() {
        let left = &base_gaps[gap_index];
        let right = &incoming_gaps[gap_index];
        if !left.is_empty() && !right.is_empty() && tokens(left) != tokens(right) {
            return Err(MergeIssue::Overlap);
        }
        if left.is_empty() {
            output.extend(right.clone());
        } else if right.is_empty() {
            output.extend(left.clone());
        } else {
            output.extend(
                left.iter()
                    .zip(right)
                    .map(|(base, incoming)| merge_entry_metadata(base, incoming))
                    .collect::<Result<Vec<_>, _>>()?,
            );
        }
        if let Some(token) = common_base.get(gap_index) {
            let base_index = base_tokens
                .iter()
                .position(|candidate| candidate == token)
                .expect("common token came from base");
            let incoming_index = incoming_tokens
                .iter()
                .position(|candidate| candidate == token)
                .expect("common token came from incoming");
            output.push(merge_entry_metadata(
                &base.entries[base_index],
                &incoming.entries[incoming_index],
            )?);
        }
    }

    let mut merged = base.clone();
    merged.entries = output;
    merged.preserved_lines =
        merge_preserved_lines(&base.preserved_lines, &incoming.preserved_lines);
    crate::playlist::refresh_entry_ids(&mut merged);
    Ok(merged)
}

fn merge_preserved_lines(base: &[String], incoming: &[String]) -> Vec<String> {
    fn counts(lines: &[String]) -> BTreeMap<&str, usize> {
        let mut output = BTreeMap::new();
        for line in lines {
            *output.entry(line.as_str()).or_default() += 1;
        }
        output
    }

    let base_counts = counts(base);
    let incoming_counts = counts(incoming);
    let keys: BTreeSet<&str> = base_counts
        .keys()
        .chain(incoming_counts.keys())
        .copied()
        .collect();
    keys.into_iter()
        .flat_map(|line| {
            let count = base_counts
                .get(line)
                .copied()
                .unwrap_or_default()
                .max(incoming_counts.get(line).copied().unwrap_or_default());
            std::iter::repeat_n(line.to_owned(), count)
        })
        .collect()
}

fn merge_entry_metadata(
    base: &PlaylistEntry,
    incoming: &PlaylistEntry,
) -> Result<PlaylistEntry, MergeIssue> {
    let mut merged = base.clone();
    if base.directives.is_empty() {
        merged.directives = incoming.directives.clone();
    } else if !incoming.directives.is_empty() && base.directives != incoming.directives {
        return Err(MergeIssue::DirectiveConflict);
    }
    if base.pls_fields.is_empty() {
        merged.pls_fields = incoming.pls_fields.clone();
    } else if !incoming.pls_fields.is_empty() && base.pls_fields != incoming.pls_fields {
        return Err(MergeIssue::DirectiveConflict);
    }
    Ok(merged)
}

fn tokens(entries: &[PlaylistEntry]) -> Vec<String> {
    let mut counts = BTreeMap::<String, usize>::new();
    entries
        .iter()
        .map(|entry| {
            let key = entry
                .resolved_path
                .clone()
                .unwrap_or_else(|| entry.raw_path.nfc().collect());
            let occurrence = counts.entry(key.clone()).or_default();
            let token = format!("{key}\0{occurrence}");
            *occurrence += 1;
            token
        })
        .collect()
}

fn gaps(
    entries: &[PlaylistEntry],
    entry_tokens: &[String],
    common: &BTreeSet<&str>,
) -> Vec<Vec<PlaylistEntry>> {
    let mut output = vec![Vec::new()];
    for (entry, token) in entries.iter().zip(entry_tokens) {
        if common.contains(token.as_str()) {
            output.push(Vec::new());
        } else {
            output
                .last_mut()
                .expect("one initial gap")
                .push(entry.clone());
        }
    }
    output
}
