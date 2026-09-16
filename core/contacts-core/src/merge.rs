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
    /// Absolute path where the surviving file exists or will be written.
    pub surviving_path: String,
    sources: std::collections::BTreeMap<String, Card>,
}

/// Build a plan for one group. Does not write or delete.
pub fn plan_merge(
    vfs: &dyn Vfs,
    root: &str,
    group: &ConflictGroup,
) -> Result<MergePlan, StoreError> {
    let surviving_path = group.surviving_path(root);
    let surviving_exists = vfs.try_exists(&surviving_path)?;
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
        let path = if is_absolute_path(&copy.path) {
            copy.path.clone()
        } else {
            join_root(root, &copy.path)
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
    let sources = versions.iter().cloned().collect();
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
            if is_absolute_path(&c.path) {
                c.path.clone()
            } else {
                join_root(root, &c.path)
            }
        })
        .collect();

    Ok(MergePlan {
        kind,
        canonical_name: relative_to_root(root, &surviving_path),
        merged,
        conflicts,
        copy_paths,
        surviving_path,
        sources,
    })
}

/// Write the merged card and delete copies. `choices` maps field → source name.
///
/// This is the explicit user action (R10). Automatic merge still goes through
/// here so two devices converge on the same bytes (R9).
pub fn apply_merge(
    vfs: &dyn Vfs,
    _root: &str,
    plan: &MergePlan,
    choices: &[(String, String)],
) -> Result<Card, StoreError> {
    for conflict in &plan.conflicts {
        let Some((_, source)) = choices.iter().find(|(field, _)| field == &conflict.field) else {
            return Err(StoreError::IncompleteChoices);
        };
        if !conflict
            .sides
            .iter()
            .any(|(candidate, _)| candidate == source)
        {
            return Err(StoreError::IncompleteChoices);
        }
    }
    let mut card = plan.merged.clone();
    for (field, source) in choices {
        if plan
            .conflicts
            .iter()
            .any(|conflict| &conflict.field == field)
        {
            let source_card = plan
                .sources
                .get(source)
                .ok_or(StoreError::IncompleteChoices)?;
            apply_choice(&mut card, field, source_card);
        }
    }
    card.file_name = plan.canonical_name.clone();
    card.canonicalize();
    vfs.write_atomic(&plan.surviving_path, write(&card).as_bytes())?;
    for copy in &plan.copy_paths {
        if vfs.try_exists(copy)? {
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
        &mut merged.middle_name,
        versions,
        "middle",
        |c| c.middle_name.clone(),
        &mut conflicts,
    );
    merge_scalar(
        &mut merged.name_prefix,
        versions,
        "prefix",
        |c| c.name_prefix.clone(),
        &mut conflicts,
    );
    merge_scalar(
        &mut merged.name_suffix,
        versions,
        "suffix",
        |c| c.name_suffix.clone(),
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

    merged.unknown_fields =
        multiset_union_strings(versions.iter().map(|(_, card)| &card.unknown_fields));

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
    let filled: Vec<(String, &Vec<u8>, Option<&str>)> = versions
        .iter()
        .filter_map(|(source, card)| {
            card.photo
                .as_ref()
                .map(|photo| (source.clone(), photo, card.photo_media_type.as_deref()))
        })
        .collect();
    if filled.len() <= 1 {
        if let Some((_, p, media_type)) = filled.first() {
            merged.photo = Some((*p).clone());
            merged.photo_media_type = media_type.map(str::to_owned);
        }
        return;
    }
    let first = (filled[0].1, filled[0].2);
    if filled
        .iter()
        .all(|(_, photo, media_type)| (*photo, *media_type) == first)
    {
        merged.photo = Some(first.0.clone());
        merged.photo_media_type = filled[0].2.map(str::to_owned);
        return;
    }
    merged.photo = Some(first.0.clone());
    merged.photo_media_type = first.1.map(str::to_owned);
    conflicts.push(FieldConflict {
        field: "photo".into(),
        sides: filled
            .into_iter()
            .map(|(source, photo, media_type)| {
                (
                    source,
                    format!(
                        "{} bytes ({})",
                        photo.len(),
                        media_type.unwrap_or("unknown type")
                    ),
                )
            })
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
    let labels: std::collections::BTreeSet<String> = versions
        .iter()
        .flat_map(|(_, card)| get(card).iter().map(|row| row.label.clone()))
        .collect();
    let mut out = Vec::new();
    for label in labels {
        let occurrences = versions
            .iter()
            .map(|(_, card)| get(card).iter().filter(|row| row.label == label).count())
            .max()
            .unwrap_or(0);
        for occurrence in 0..occurrences {
            let sides: Vec<(String, String)> = versions
                .iter()
                .filter_map(|(source, card)| {
                    get(card)
                        .iter()
                        .filter(|row| row.label == label)
                        .nth(occurrence)
                        .map(|row| (source.clone(), row.value.clone()))
                })
                .collect();
            let unique: std::collections::BTreeSet<&str> =
                sides.iter().map(|(_, value)| value.as_str()).collect();
            if unique.len() > 1 {
                dest_keep_first(&mut out, &label, &sides);
                let suffix = if occurrence == 0 {
                    String::new()
                } else {
                    format!(":{occurrence}")
                };
                conflicts.push(FieldConflict {
                    field: format!("{kind}:{label}{suffix}"),
                    sides,
                });
            } else if let Some((_, value)) = sides.first() {
                out.push(Labeled {
                    label: label.clone(),
                    value: value.clone(),
                });
            }
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
    let labels: std::collections::BTreeSet<String> = versions
        .iter()
        .flat_map(|(_, card)| card.addresses.iter().map(|row| row.label.clone()))
        .collect();
    let mut out = Vec::new();
    for label in labels {
        let occurrences = versions
            .iter()
            .map(|(_, card)| {
                card.addresses
                    .iter()
                    .filter(|row| row.label == label)
                    .count()
            })
            .max()
            .unwrap_or(0);
        for occurrence in 0..occurrences {
            let sides: Vec<(String, LabeledAddress)> = versions
                .iter()
                .filter_map(|(source, card)| {
                    card.addresses
                        .iter()
                        .filter(|row| row.label == label)
                        .nth(occurrence)
                        .map(|row| (source.clone(), row.clone()))
                })
                .collect();
            let unique: std::collections::BTreeSet<String> =
                sides.iter().map(|(_, a)| a.value.formatted()).collect();
            if unique.len() > 1 {
                if let Some((_, first)) = sides.first() {
                    out.push(first.clone());
                }
                let suffix = if occurrence == 0 {
                    String::new()
                } else {
                    format!(":{occurrence}")
                };
                conflicts.push(FieldConflict {
                    field: format!("adr:{label}{suffix}"),
                    sides: sides
                        .into_iter()
                        .map(|(s, a)| (s, a.value.formatted()))
                        .collect(),
                });
            } else if let Some((_, addr)) = sides.into_iter().next() {
                out.push(addr);
            }
        }
    }
    out.sort();
    merged.addresses = out;
}

fn apply_choice(card: &mut Card, field: &str, source: &Card) {
    match field {
        "fn" => card.full_name = source.full_name.clone(),
        "family" => card.family_name = source.family_name.clone(),
        "given" => card.given_name = source.given_name.clone(),
        "middle" => card.middle_name = source.middle_name.clone(),
        "prefix" => card.name_prefix = source.name_prefix.clone(),
        "suffix" => card.name_suffix = source.name_suffix.clone(),
        "org" => card.organization = source.organization.clone(),
        "title" => card.job_title = source.job_title.clone(),
        "nickname" => card.nickname = source.nickname.clone(),
        "note" => card.note = source.note.clone(),
        "id" => card.local_id = source.local_id.clone(),
        "bday" => card.birthday = source.birthday.clone(),
        "photo" => {
            card.photo = source.photo.clone();
            card.photo_media_type = source.photo_media_type.clone();
        }
        other if other.starts_with("tel:") => {
            apply_labeled_choice(&mut card.phones, &source.phones, &other[4..]);
        }
        other if other.starts_with("email:") => {
            apply_labeled_choice(&mut card.emails, &source.emails, &other[6..]);
        }
        other if other.starts_with("url:") => {
            apply_labeled_choice(&mut card.urls, &source.urls, &other[4..]);
        }
        other if other.starts_with("adr:") => {
            let (label, occurrence) = occurrence_key(&other[4..]);
            if let (Some(dest), Some(selected)) = (
                card.addresses
                    .iter_mut()
                    .filter(|row| row.label == label)
                    .nth(occurrence),
                source
                    .addresses
                    .iter()
                    .filter(|row| row.label == label)
                    .nth(occurrence),
            ) {
                *dest = selected.clone();
            }
        }
        _ => {}
    }
}

fn apply_labeled_choice(rows: &mut [Labeled], source: &[Labeled], key: &str) {
    let (label, occurrence) = occurrence_key(key);
    let selected = source
        .iter()
        .filter(|row| row.label == label)
        .nth(occurrence);
    if let (Some(row), Some(selected)) = (
        rows.iter_mut()
            .filter(|row| row.label == label)
            .nth(occurrence),
        selected,
    ) {
        *row = selected.clone();
    }
}

fn occurrence_key(key: &str) -> (&str, usize) {
    key.rsplit_once(':')
        .and_then(|(label, suffix)| suffix.parse::<usize>().ok().map(|index| (label, index)))
        .unwrap_or((key, 0))
}

fn multiset_union_strings<'a>(values: impl Iterator<Item = &'a Vec<String>>) -> Vec<String> {
    let mut maxima = std::collections::BTreeMap::<String, usize>::new();
    for value_set in values {
        let mut counts = std::collections::BTreeMap::<&str, usize>::new();
        for value in value_set {
            *counts.entry(value).or_default() += 1;
        }
        for (value, count) in counts {
            let maximum = maxima.entry(value.to_owned()).or_default();
            *maximum = (*maximum).max(count);
        }
    }
    maxima
        .into_iter()
        .flat_map(|(value, count)| std::iter::repeat_n(value, count))
        .collect()
}

fn is_absolute_path(path: &str) -> bool {
    path.starts_with(['/', '\\'])
        || path
            .as_bytes()
            .get(1)
            .is_some_and(|separator| *separator == b':')
}

fn relative_to_root(root: &str, path: &str) -> String {
    let root = root.trim_end_matches(['/', '\\']);
    path.strip_prefix(root)
        .and_then(|rest| rest.strip_prefix(['/', '\\']))
        .unwrap_or(path)
        .to_owned()
}
