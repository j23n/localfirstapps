//! Preservation-first merge planning for Syncthing `.xmp` conflict copies.
//!
//! [`merge_sidecar_conflicts`] is pure: it returns candidate bytes and never
//! writes or deletes a file. [`apply_sidecar_conflict`] is the explicit R10
//! commit (`write_atomic` of those bytes, then `remove` of losing copies).
//! Callers must not invent a second merge.

use std::collections::{BTreeMap, BTreeSet};

use gallery_vfs::{Vfs, VfsError};

use crate::edit;
use crate::error::{MetaError, MetaResult};
use crate::read::view_of;
use crate::schema::*;
use crate::tags::{nfc, nfc_lower};
use crate::xml::{parse, serialize, Document, Element, Node, NsScope};

/// One sidecar participating in a conflict group.
#[derive(Debug, Clone, Copy)]
pub struct SidecarVersion<'a> {
    /// Path displayed to the user and used as a deterministic tie-breaker.
    pub path: &'a str,
    /// Filesystem modification time in nanoseconds since the Unix epoch.
    ///
    /// The newest version supplies LocalGallery-owned fields. Equal times are
    /// broken by `path` and then by bytes, so directory enumeration order can
    /// never affect the output.
    pub modified_ns: i128,
    /// Complete UTF-8 XMP packet.
    pub bytes: &'a [u8],
}

/// A merge candidate that has not touched the filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarConflictMerge {
    /// Canonical, deterministic candidate bytes.
    pub bytes: Vec<u8>,
    /// Version whose LocalGallery-owned values won.
    pub newest_path: String,
    /// Every input path, sorted and deduplicated for the confirmation UI.
    pub input_paths: Vec<String>,
}

/// Merge a canonical sidecar and its Syncthing copies without writing either.
///
/// Human keywords are a case-aware union across every version. Values listed
/// in a version's `Core*` ownership sentinels are not human and therefore do
/// not leak back from an older version. LocalGallery-owned scalar, sentinel,
/// face-region, and person-projection fields come from the newest version.
///
/// Properties in namespaces this crate does not own are copied rather than
/// overwritten. Divergent unknown scalar values therefore both survive in
/// separate `rdf:Description` blocks; a future tool that understands the
/// namespace can resolve them without this app having destroyed either edit.
pub fn merge_sidecar_conflicts(
    versions: &[SidecarVersion<'_>],
) -> MetaResult<SidecarConflictMerge> {
    if versions.is_empty() {
        return Err(MetaError::NotAnXmpPacket {
            detail: "a sidecar conflict merge needs at least one version".into(),
        });
    }

    let mut ordered = versions.to_vec();
    ordered.sort_by(|a, b| {
        a.modified_ns
            .cmp(&b.modified_ns)
            .then_with(|| a.path.cmp(b.path))
            .then_with(|| a.bytes.cmp(b.bytes))
    });

    let mut parsed = Vec::with_capacity(ordered.len());
    for version in &ordered {
        let doc = parse(version.bytes)?;
        if edit::find_rdf_root(&doc).is_none() {
            return Err(MetaError::NotAnXmpPacket {
                detail: format!("{} has no rdf:RDF element", version.path),
            });
        }
        parsed.push(doc);
    }

    let newest = ordered.last().expect("checked non-empty");
    let mut merged = parsed.last().expect("parallel vectors").clone();
    let root = edit::find_rdf_root(&merged).expect("validated above");

    merge_human_keywords(&mut merged, &root, &parsed);
    merge_unknown_properties(&mut merged, &root, &parsed);

    let mut input_paths: Vec<String> = ordered.iter().map(|v| v.path.to_string()).collect();
    input_paths.sort();
    input_paths.dedup();
    Ok(SidecarConflictMerge {
        bytes: serialize(&merged),
        newest_path: newest.path.to_string(),
        input_paths,
    })
}

/// Write merged sidecar bytes to the surviving path, then delete copies.
///
/// This is the commit step, not a merge. `bytes` must already come from
/// [`merge_sidecar_conflicts`]. A copy that is already gone is ignored.
pub fn apply_sidecar_conflict(
    vfs: &dyn Vfs,
    surviving_path: &str,
    copy_paths: &[String],
    bytes: &[u8],
) -> MetaResult<()> {
    vfs.write_atomic(surviving_path, bytes)?;
    for path in copy_paths {
        match vfs.remove(path) {
            Ok(()) => {}
            Err(VfsError::NotFound { .. }) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum KeywordField {
    Subject,
    Tags,
    Hierarchical,
}

fn merge_human_keywords(doc: &mut Document, root: &[usize], sources: &[Document]) {
    for (field, uri, prefix, local, kind, ignore_case) in [
        (
            KeywordField::Subject,
            NS_DC,
            PREFIX_DC,
            PROP_SUBJECT,
            "Bag",
            true,
        ),
        (
            KeywordField::Tags,
            NS_DIGIKAM,
            PREFIX_DIGIKAM,
            PROP_TAGS_LIST,
            "Seq",
            false,
        ),
        (
            KeywordField::Hierarchical,
            NS_LR,
            PREFIX_LR,
            PROP_HIERARCHICAL_SUBJECT,
            "Bag",
            false,
        ),
    ] {
        let mut wanted: BTreeMap<String, String> = BTreeMap::new();
        for source in sources {
            let view = view_of(source);
            let (values, owned): (&[String], Vec<String>) = match field {
                KeywordField::Subject => (
                    &view.subject,
                    view.core
                        .subjects
                        .iter()
                        .chain(&view.core.people_subjects)
                        .cloned()
                        .collect(),
                ),
                KeywordField::Tags => (
                    &view.tags_list,
                    view.core
                        .tags
                        .iter()
                        .chain(&view.core.people)
                        .cloned()
                        .collect(),
                ),
                KeywordField::Hierarchical => (
                    &view.hierarchical_subject,
                    view.core
                        .hierarchical
                        .iter()
                        .chain(&view.core.people_hierarchical)
                        .cloned()
                        .collect(),
                ),
            };
            let key = |value: &str| {
                if ignore_case {
                    nfc_lower(value)
                } else {
                    nfc(value)
                }
            };
            let owned: BTreeSet<String> = owned.iter().map(|v| key(v)).collect();
            for value in values {
                let normalized = key(value);
                if owned.contains(&normalized) {
                    continue;
                }
                let value = nfc(value);
                wanted
                    .entry(normalized)
                    .and_modify(|present| {
                        if value < *present {
                            *present = value.clone();
                        }
                    })
                    .or_insert(value);
            }
        }

        let present: BTreeSet<String> = edit::list_values(doc, root, uri, local)
            .iter()
            .map(|v| if ignore_case { nfc_lower(v) } else { nfc(v) })
            .collect();
        let add: Vec<String> = wanted
            .into_iter()
            .filter_map(|(key, value)| (!present.contains(&key)).then_some(value))
            .collect();
        if ignore_case {
            crate::write::edit_list_ignore_case(
                doc,
                &root.to_vec(),
                uri,
                prefix,
                local,
                kind,
                &[],
                &add,
            );
        } else {
            crate::write::edit_list_exact(doc, &root.to_vec(), uri, prefix, local, kind, &[], &add);
        }
    }
}

fn merge_unknown_properties(doc: &mut Document, root: &[usize], sources: &[Document]) {
    let mut seen = BTreeSet::new();
    for desc in unknown_descriptions(sources.last().expect("non-empty")) {
        seen.insert(element_bytes(&desc));
    }

    let mut additions: BTreeMap<Vec<u8>, Element> = BTreeMap::new();
    for source in sources.iter().take(sources.len().saturating_sub(1)) {
        for desc in unknown_descriptions(source) {
            let bytes = element_bytes(&desc);
            if !seen.contains(&bytes) {
                additions.entry(bytes).or_insert(desc);
            }
        }
    }

    for (bytes, desc) in additions {
        edit::append_child_element(doc, root, desc);
        seen.insert(bytes);
    }
}

fn unknown_descriptions(doc: &Document) -> Vec<Element> {
    let Some(root) = edit::find_rdf_root(doc) else {
        return Vec::new();
    };
    let Some(rdf) = edit::element_at(doc, &root) else {
        return Vec::new();
    };
    let root_scope = edit::scope_at(doc, &root);
    let mut out = Vec::new();

    for child in rdf.child_elements() {
        let scope = root_scope.extended(child);
        if !scope.matches(child, NS_RDF, "Description") {
            continue;
        }
        let mut copy = child.clone();
        let mut payload = false;

        let remove_attrs: Vec<String> = copy
            .attrs()
            .iter()
            .filter(|attr| {
                if attr.name == "xmlns" || attr.name.starts_with("xmlns:") {
                    return false;
                }
                let (uri, local) = scope.resolve(&attr.name);
                if is_scaffolding(uri, local) {
                    return false;
                }
                if is_merge_controlled(uri, local) {
                    return true;
                }
                payload = true;
                false
            })
            .map(|attr| attr.name.clone())
            .collect();
        for name in remove_attrs {
            copy.remove_attr(&name);
        }

        copy.children.retain(|node| {
            let Some(el) = node.as_element() else {
                return true;
            };
            let inner = scope.extended(el);
            let (uri, local) = inner.resolve(&el.name);
            if is_merge_controlled(uri, local) {
                false
            } else {
                payload = true;
                true
            }
        });

        if !payload {
            continue;
        }
        bind_inherited_property_prefixes(&mut copy, &scope);
        out.push(copy);
    }
    out
}

fn bind_inherited_property_prefixes(desc: &mut Element, source_scope: &NsScope) {
    let prefixes: BTreeSet<String> = desc
        .children
        .iter()
        .filter_map(Node::as_element)
        .filter_map(Element::prefix)
        .filter(|prefix| *prefix != "rdf" && *prefix != "xml")
        .map(str::to_string)
        .collect();
    for prefix in prefixes {
        let attr = format!("xmlns:{prefix}");
        if desc.attr(&attr).is_none() {
            if let Some(uri) = source_scope.uri_for(&prefix) {
                desc.set_attr(attr, uri);
            }
        }
    }
}

fn is_scaffolding(uri: Option<&str>, local: &str) -> bool {
    uri == Some(NS_RDF) && local == "about"
}

fn is_merge_controlled(uri: Option<&str>, local: &str) -> bool {
    matches!(
        (uri, local),
        (Some(NS_DC), PROP_SUBJECT)
            | (Some(NS_DIGIKAM), PROP_TAGS_LIST)
            | (Some(NS_LR), PROP_HIERARCHICAL_SUBJECT)
            | (Some(NS_IPTC_EXT), PROP_PERSON_IN_IMAGE)
            | (Some(NS_MWG_RS), PROP_REGIONS)
    ) || (uri == Some(NS_PHOTO_TOOLS)
        && (local.starts_with("Core")
            || matches!(
                local,
                PROP_TAGGER_VERSION
                    | PROP_TAGGED_AT
                    | PROP_CLIP_EMBEDDING
                    | PROP_CLIP_MODEL
                    | PROP_CLIP_TIMESTAMP
            )))
}

fn element_bytes(element: &Element) -> Vec<u8> {
    serialize(&Document {
        nodes: vec![Node::Element(element.clone())],
    })
}
