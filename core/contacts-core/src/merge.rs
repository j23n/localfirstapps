//! Syncthing `.vcf` merge (ADR 0005 R8–R11).
//!
//! Grouping is [`localcore_conflict`]. Policy lives here. Copies are never
//! deleted until [`apply_merge`] — that call *is* the explicit choice (R10).

use localcore_conflict::ConflictGroup;
use localcore_vfs::Vfs;

use crate::card::{Card, Labeled, LabeledAddress};
use crate::store::{join_root, StoreError};
use crate::vcard::{parse, write};

/// How a group resolves before the user confirms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeKind {
    /// Every field is disjoint or identical — merged bytes are determined.
    Auto,
    /// At least one field differs on two sides.
    Choice,
    /// Surviving file is gone; a copy holds the data (R11).
    DeletedVersusModified,
}

/// One field that cannot merge automatically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldConflict {
    /// Stable field key (`fn`, `tel:cell`, `note`, …).
    pub field: String,
    /// `(source, formatted value)` pairs, source is `surviving` or a copy name.
    pub sides: Vec<(String, String)>,
}

/// Computed merge. Does not write. Apply is a separate call.
#[derive(Debug, Clone)]
pub struct MergePlan {
    /// Auto / choice / delete-vs-modify.
    pub kind: MergeKind,
    /// Surviving basename.
    pub canonical_name: String,
    /// Disjoint union; conflicted fields keep the surviving (or first) value.
    pub merged: Card,
    /// Fields that need a pick.
    pub conflicts: Vec<FieldConflict>,
    /// Absolute paths of copies to delete on apply.
    pub copy_paths: Vec<String>,
    /// Absolute path of the surviving file, if it exists.
    pub surviving_path: Option<String>,
}

/// Build a plan for one group. Does not write or delete.
pub fn plan_merge(
    vfs: &dyn Vfs,
    root: &str,
    group: &ConflictGroup,
) -> Result<MergePlan, StoreError> {
    let surviving_path = join_root(root, &group.canonical_name);
    let surviving_exists = vfs.exists(&surviving_path);
    let surviving = if surviving_exists {
        let bytes = vfs.read(&surviving_path)?;
        parse(&bytes, &group.canonical_name, false)
    } else {
        None
    };

    let mut versions: Vec<(String, Card)> = Vec::new();
    if let Some(card) = surviving {
        versions.push(("surviving".into(), card));
    }
    for copy in &group.copies {
        let path = if copy.path.contains('/') {
            copy.path.clone()
        } else {
            join_root(root, &copy.name)
        };
        let bytes = vfs.read(&path)?;
        if let Some(card) = parse(&bytes, &group.canonical_name, false) {
            versions.push((copy.name.clone(), card));
        }
    }
    if versions.is_empty() {
        return Err(StoreError::NotFound);
    }

    let (merged, conflicts) = merge_versions(&versions);
    let kind = if !surviving_exists {
        if conflicts.is_empty() {
            MergeKind::DeletedVersusModified
        } else {
            MergeKind::Choice
        }
    } else if conflicts.is_empty() {
        MergeKind::Auto
    } else {
        MergeKind::Choice
    };

    let copy_paths = group
        .copies
        .iter()
        .map(|c| {
            if c.path.contains('/') {
                c.path.clone()
            } else {
                join_root(root, &c.name)
            }
        })
        .collect();

    Ok(MergePlan {
        kind,
        canonical_name: group.canonical_name.clone(),
        merged,
        conflicts,
        copy_paths,
        surviving_path: surviving_exists.then_some(surviving_path),
    })
}

/// Write the merged card and delete copies. `choices` maps field → source name.
///
/// This is the explicit user action (R10). Automatic merge still goes through
/// here so two devices converge on the same bytes (R9).
pub fn apply_merge(
    vfs: &dyn Vfs,
    root: &str,
    plan: &MergePlan,
    choices: &[(String, String)],
) -> Result<Card, StoreError> {
    if plan.kind == MergeKind::Choice {
        for conflict in &plan.conflicts {
            if !choices.iter().any(|(f, _)| f == &conflict.field) {
                return Err(StoreError::IncompleteChoices);
            }
        }
    }
    let mut card = plan.merged.clone();
    for (field, source) in choices {
        if let Some(conflict) = plan.conflicts.iter().find(|c| &c.field == field) {
            if let Some((_, value)) = conflict.sides.iter().find(|(s, _)| s == source) {
                apply_choice(&mut card, field, value);
            }
        }
    }
    card.file_name = plan.canonical_name.clone();
    card.canonicalize();
    let path = join_root(root, &plan.canonical_name);
    vfs.write_atomic(&path, write(&card).as_bytes())?;
    for copy in &plan.copy_paths {
        if vfs.exists(copy) {
            vfs.remove(copy)?;
        }
    }
    Ok(card)
}

fn merge_versions(versions: &[(String, Card)]) -> (Card, Vec<FieldConflict>) {
    let mut merged = versions[0].1.clone();
    let mut conflicts = Vec::new();

    merge_scalar(
        &mut merged.full_name,
        versions,
        "fn",
        |c| c.full_name.clone(),
        &mut conflicts,
    );
    merge_scalar(
        &mut merged.family_name,
        versions,
        "family",
        |c| c.family_name.clone(),
        &mut conflicts,
    );
    merge_scalar(
        &mut merged.given_name,
        versions,
        "given",
        |c| c.given_name.clone(),
        &mut conflicts,
    );
    merge_scalar(
        &mut merged.organization,
        versions,
        "org",
        |c| c.organization.clone(),
        &mut conflicts,
    );
    merge_scalar(
        &mut merged.job_title,
        versions,
        "title",
        |c| c.job_title.clone(),
        &mut conflicts,
    );
    merge_scalar(
        &mut merged.nickname,
        versions,
        "nickname",
        |c| c.nickname.clone(),
        &mut conflicts,
    );
    merge_scalar(
        &mut merged.note,
        versions,
        "note",
        |c| c.note.clone(),
        &mut conflicts,
    );
    merge_id(&mut merged, versions, &mut conflicts);
    merge_birthday(&mut merged, versions, &mut conflicts);
    merge_photo(&mut merged, versions, &mut conflicts);
    merge_labeled(
        &mut merged.phones,
        versions,
        "tel",
        |c| &c.phones,
        &mut conflicts,
    );
    merge_labeled(
        &mut merged.emails,
        versions,
        "email",
        |c| &c.emails,
        &mut conflicts,
    );
    merge_labeled(
        &mut merged.urls,
        versions,
        "url",
        |c| &c.urls,
        &mut conflicts,
    );
    merge_addresses(&mut merged, versions, &mut conflicts);

    let mut cats: Vec<String> = versions
        .iter()
        .flat_map(|(_, c)| c.categories.clone())
        .collect();
    cats.sort();
    cats.dedup();
    merged.categories = cats;

    let mut unknown: Vec<String> = versions
        .iter()
        .flat_map(|(_, c)| c.unknown_fields.clone())
        .collect();
    unknown.sort();
    unknown.dedup();
    merged.unknown_fields = unknown;

    merged.canonicalize();
    conflicts.sort_by(|a, b| a.field.cmp(&b.field));
    (merged, conflicts)
}

fn merge_id(merged: &mut Card, versions: &[(String, Card)], conflicts: &mut Vec<FieldConflict>) {
    let filled: Vec<(String, String)> = versions
        .iter()
        .filter(|(_, c)| !c.local_id.is_empty())
        .map(|(s, c)| (s.clone(), c.local_id.clone()))
        .collect();
    let unique: std::collections::BTreeSet<&str> = filled.iter().map(|(_, v)| v.as_str()).collect();
    if unique.len() > 1 {
        conflicts.push(FieldConflict {
            field: "id".into(),
            sides: filled,
        });
    } else if let Some((_, id)) = filled.first() {
        merged.local_id = id.clone();
    }
}

fn merge_scalar(
    dest: &mut String,
    versions: &[(String, Card)],
    field: &str,
    get: impl Fn(&Card) -> String,
    conflicts: &mut Vec<FieldConflict>,
) {
    let filled: Vec<(String, String)> = versions
        .iter()
        .map(|(s, c)| (s.clone(), get(c)))
        .filter(|(_, v)| !v.is_empty())
        .collect();
    let unique: std::collections::BTreeSet<&str> = filled.iter().map(|(_, v)| v.as_str()).collect();
    match unique.len() {
        0 => {}
        1 => *dest = filled[0].1.clone(),
        _ => {
            *dest = filled[0].1.clone();
            conflicts.push(FieldConflict {
                field: field.into(),
                sides: filled,
            });
        }
    }
}

fn merge_birthday(
    merged: &mut Card,
    versions: &[(String, Card)],
    conflicts: &mut Vec<FieldConflict>,
) {
    let filled: Vec<(String, String)> = versions
        .iter()
        .filter_map(|(s, c)| c.birthday.as_ref().map(|b| (s.clone(), format_bday(b))))
        .collect();
    let unique: std::collections::BTreeSet<&str> = filled.iter().map(|(_, v)| v.as_str()).collect();
    if unique.len() > 1 {
        conflicts.push(FieldConflict {
            field: "bday".into(),
            sides: filled,
        });
    } else if let Some((_, first)) = versions.iter().find(|(_, c)| c.birthday.is_some()) {
        merged.birthday = first.birthday.clone();
    }
}

fn format_bday(b: &crate::card::Birthday) -> String {
    match b.year {
        Some(y) => format!("{y:04}-{:02}-{:02}", b.month, b.day),
        None => format!("--{:02}-{:02}", b.month, b.day),
    }
}

fn merge_photo(merged: &mut Card, versions: &[(String, Card)], conflicts: &mut Vec<FieldConflict>) {
    let filled: Vec<(String, &Vec<u8>)> = versions
        .iter()
        .filter_map(|(s, c)| c.photo.as_ref().map(|p| (s.clone(), p)))
        .collect();
    if filled.len() <= 1 {
        if let Some((_, p)) = filled.first() {
            merged.photo = Some((*p).clone());
        }
        return;
    }
    let first = filled[0].1;
    if filled.iter().all(|(_, p)| *p == first) {
        merged.photo = Some(first.clone());
        return;
    }
    merged.photo = Some(first.clone());
    conflicts.push(FieldConflict {
        field: "photo".into(),
        sides: filled
            .into_iter()
            .map(|(s, p)| (s, format!("{} bytes", p.len())))
            .collect(),
    });
}

fn merge_labeled(
    dest: &mut Vec<Labeled>,
    versions: &[(String, Card)],
    kind: &str,
    get: impl Fn(&Card) -> &Vec<Labeled>,
    conflicts: &mut Vec<FieldConflict>,
) {
    let mut by_label: std::collections::BTreeMap<String, Vec<(String, String)>> =
        std::collections::BTreeMap::new();
    for (source, card) in versions {
        for row in get(card) {
            by_label
                .entry(row.label.clone())
                .or_default()
                .push((source.clone(), row.value.clone()));
        }
    }
    let mut out = Vec::new();
    for (label, sides) in by_label {
        let unique: std::collections::BTreeSet<&str> =
            sides.iter().map(|(_, v)| v.as_str()).collect();
        if unique.len() > 1 {
            dest_keep_first(&mut out, &label, &sides);
            conflicts.push(FieldConflict {
                field: format!("{kind}:{label}"),
                sides,
            });
        } else if let Some((_, value)) = sides.first() {
            out.push(Labeled {
                label,
                value: value.clone(),
            });
        }
    }
    out.sort();
    *dest = out;
}

fn dest_keep_first(out: &mut Vec<Labeled>, label: &str, sides: &[(String, String)]) {
    if let Some((_, value)) = sides.first() {
        out.push(Labeled {
            label: label.to_owned(),
            value: value.clone(),
        });
    }
}

fn merge_addresses(
    merged: &mut Card,
    versions: &[(String, Card)],
    conflicts: &mut Vec<FieldConflict>,
) {
    let mut by_label: std::collections::BTreeMap<String, Vec<(String, LabeledAddress)>> =
        std::collections::BTreeMap::new();
    for (source, card) in versions {
        for row in &card.addresses {
            by_label
                .entry(row.label.clone())
                .or_default()
                .push((source.clone(), row.clone()));
        }
    }
    let mut out = Vec::new();
    for (label, sides) in by_label {
        let unique: std::collections::BTreeSet<String> =
            sides.iter().map(|(_, a)| a.value.formatted()).collect();
        if unique.len() > 1 {
            if let Some((_, first)) = sides.first() {
                out.push(first.clone());
            }
            conflicts.push(FieldConflict {
                field: format!("adr:{label}"),
                sides: sides
                    .into_iter()
                    .map(|(s, a)| (s, a.value.formatted()))
                    .collect(),
            });
        } else if let Some((_, addr)) = sides.into_iter().next() {
            out.push(addr);
        }
    }
    out.sort();
    merged.addresses = out;
}

fn apply_choice(card: &mut Card, field: &str, value: &str) {
    match field {
        "fn" => card.full_name = value.to_owned(),
        "family" => card.family_name = value.to_owned(),
        "given" => card.given_name = value.to_owned(),
        "org" => card.organization = value.to_owned(),
        "title" => card.job_title = value.to_owned(),
        "nickname" => card.nickname = value.to_owned(),
        "note" => card.note = value.to_owned(),
        "id" => card.local_id = value.to_owned(),
        other if other.starts_with("tel:") => {
            let label = &other[4..];
            if let Some(row) = card.phones.iter_mut().find(|p| p.label == label) {
                row.value = value.to_owned();
            }
        }
        other if other.starts_with("email:") => {
            let label = &other[6..];
            if let Some(row) = card.emails.iter_mut().find(|p| p.label == label) {
                row.value = value.to_owned();
            }
        }
        other if other.starts_with("url:") => {
            let label = &other[4..];
            if let Some(row) = card.urls.iter_mut().find(|p| p.label == label) {
                row.value = value.to_owned();
            }
        }
        _ => {}
    }
}
