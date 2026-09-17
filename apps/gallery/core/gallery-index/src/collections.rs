//! Collections-hub grouping over aggregated tag suggestions.
//!
//! Leftover GTK used to own this; `gallery-ffi` location windows are the
//! consumer. People stay out of these groups — they have their own window API.

use std::collections::HashMap;

use crate::tags::TagSuggestion;

/// A Collections hub section (Objects, Scenes, Places, …).
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionGroup {
    /// First path segment, or `"Other"` for flat tags.
    pub name: String,
    /// Photo count credited to the group (max of its buckets, not a unique union).
    pub count: usize,
    /// Buckets in this namespace, already sorted by the index.
    pub tags: Vec<TagSuggestion>,
}

/// Group aggregated tags for the Collections hub. People are excluded — they
/// have their own row.
pub fn collection_groups(tags: &[TagSuggestion]) -> Vec<CollectionGroup> {
    let mut order: Vec<String> = Vec::new();
    let mut buckets: HashMap<String, Vec<TagSuggestion>> = HashMap::new();
    for tag in tags {
        let ns = tag
            .namespace
            .as_deref()
            .filter(|n| !n.is_empty())
            .unwrap_or("Other");
        if ns.eq_ignore_ascii_case("people") {
            continue;
        }
        if !buckets.contains_key(ns) {
            order.push(ns.to_string());
        }
        buckets.entry(ns.to_string()).or_default().push(tag.clone());
    }
    order
        .into_iter()
        .filter_map(|name| {
            let tags = buckets.remove(&name)?;
            let count = tags.iter().map(|t| t.count).max().unwrap_or(0);
            Some(CollectionGroup { name, count, tags })
        })
        .collect()
}

/// Tags that are not a prefix of another tag in `tags`.
///
/// `Places/Italy` drops out when `Places/Italy/Lazio/Rome` exists, so a rail
/// shows cities rather than every ancestor.
pub fn leaf_tags(tags: &[TagSuggestion]) -> Vec<TagSuggestion> {
    tags.iter()
        .filter(|tag| {
            let prefix = format!("{}/", tag.full_path);
            !tags
                .iter()
                .any(|other| other.full_path.starts_with(&prefix))
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gallery_model::HierarchicalTag;

    fn tag(path: &str, count: usize) -> TagSuggestion {
        let tag = HierarchicalTag::new(path);
        TagSuggestion {
            id: path.to_ascii_lowercase(),
            display_name: tag.display_name,
            full_path: tag.full_path,
            namespace: tag.namespace,
            count,
            latest_photo_date: None,
        }
    }

    #[test]
    fn collection_groups_skip_people_and_keep_namespace_order() {
        let tags = vec![
            tag("Objects/Cat", 4),
            tag("People/Ada", 2),
            tag("Scenes/Beach", 9),
        ];
        let groups = collection_groups(&tags);
        assert_eq!(
            groups
                .iter()
                .map(|g| (g.name.as_str(), g.count))
                .collect::<Vec<_>>(),
            vec![("Objects", 4), ("Scenes", 9)]
        );
    }

    #[test]
    fn leaf_tags_drop_prefix_ancestors() {
        let tags = vec![
            tag("Places/Italy", 10),
            tag("Places/Italy/Lazio", 8),
            tag("Places/Italy/Lazio/Rome", 5),
            tag("Places/France/Paris", 3),
        ];
        let leaves = leaf_tags(&tags);
        assert_eq!(
            leaves
                .iter()
                .map(|t| t.full_path.as_str())
                .collect::<Vec<_>>(),
            vec!["Places/Italy/Lazio/Rome", "Places/France/Paris"]
        );
    }

    #[test]
    fn flat_tags_land_in_other() {
        let groups = collection_groups(&[tag("Vacation", 3)]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "Other");
        assert_eq!(groups[0].tags[0].full_path, "Vacation");
    }
}
