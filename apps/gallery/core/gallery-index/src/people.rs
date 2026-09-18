//! People-rail ordering: hidden filter, featured-first, optional recency gate.
//!
//! Ports `PeopleStore.orderedVisiblePeople`. Path compares are NFC-folded so
//! they agree with Swift `Set`/`==` canonical equivalence.

use gallery_model::date::{AppleDate, CivilDateTime, APPLE_EPOCH_OFFSET};

use crate::tags::TagSuggestion;
use crate::text;

/// Collections-rail cap (`visiblePeopleForRail`).
pub const PEOPLE_RAIL_CAP: usize = 20;

/// Hidden filtered out, featured floated to the front in feature order.
///
/// `recency_gated`: non-featured need a photo dated within two calendar years
/// of `now` (Apple reference seconds). Featured bypass the gate. `cap` is
/// applied after ordering.
pub fn visible_people(
    people: &[TagSuggestion],
    hidden: &[String],
    featured: &[String],
    now: f64,
    recency_gated: bool,
    cap: Option<usize>,
) -> Vec<TagSuggestion> {
    let hidden: std::collections::HashSet<String> = hidden.iter().map(|p| text::nfc(p)).collect();
    let visible: Vec<&TagSuggestion> = people
        .iter()
        .filter(|person| !hidden.contains(&text::nfc(&person.full_path)))
        .collect();
    let featured_keys: Vec<String> = featured.iter().map(|p| text::nfc(p)).collect();
    let featured_set: std::collections::HashSet<String> = featured_keys.iter().cloned().collect();
    let featured_first: Vec<TagSuggestion> = featured_keys
        .iter()
        .filter_map(|key| {
            visible
                .iter()
                .find(|person| text::nfc(&person.full_path) == *key)
                .map(|person| (*person).clone())
        })
        .collect();
    let mut rest: Vec<TagSuggestion> = visible
        .into_iter()
        .filter(|person| !featured_set.contains(&text::nfc(&person.full_path)))
        .cloned()
        .collect();
    if recency_gated {
        let cutoff = two_years_ago(now);
        rest.retain(|person| person.latest_photo_date.unwrap_or(f64::NEG_INFINITY) > cutoff);
    }
    let mut ordered = featured_first;
    ordered.append(&mut rest);
    match cap {
        Some(n) => ordered.into_iter().take(n).collect(),
        None => ordered,
    }
}

fn two_years_ago(now: f64) -> f64 {
    let unix = AppleDate(now).unix_secs_f64();
    let mut civil = CivilDateTime::from_unix_secs_f64(unix);
    civil.year -= 2;
    if civil.month == 2 && civil.day == 29 && !is_leap(civil.year) {
        civil.day = 28;
    }
    civil.as_naive_unix_secs() as f64 - APPLE_EPOCH_OFFSET
}

fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(name: &str, count: usize, latest: Option<f64>) -> TagSuggestion {
        TagSuggestion {
            id: format!("people/{}", name.to_lowercase()),
            display_name: name.to_string(),
            full_path: format!("People/{name}"),
            namespace: Some("People".into()),
            count,
            latest_photo_date: latest,
        }
    }

    #[test]
    fn hidden_are_dropped_and_featured_keep_user_order() {
        let people = vec![
            person("Cy", 9, Some(400.0)),
            person("Ada", 4, Some(400.0)),
            person("Bob", 2, Some(400.0)),
        ];
        let out = visible_people(
            &people,
            &["People/Cy".into()],
            &["People/Bob".into(), "People/Ada".into()],
            500.0,
            false,
            None,
        );
        assert_eq!(
            out.iter().map(|p| p.full_path.as_str()).collect::<Vec<_>>(),
            vec!["People/Bob", "People/Ada"]
        );
    }

    #[test]
    fn rail_recency_skips_stale_non_featured_and_caps() {
        // now ≈ 2024-06-11; two years ago ≈ 2022-06-11.
        let now = CivilDateTime::new(2024, 6, 11, 12, 0, 0).as_naive_unix_secs() as f64
            - APPLE_EPOCH_OFFSET;
        let recent = CivilDateTime::new(2023, 1, 1, 0, 0, 0).as_naive_unix_secs() as f64
            - APPLE_EPOCH_OFFSET;
        let stale = CivilDateTime::new(2020, 1, 1, 0, 0, 0).as_naive_unix_secs() as f64
            - APPLE_EPOCH_OFFSET;
        let people = vec![
            person("Old", 50, Some(stale)),
            person("New", 3, Some(recent)),
            person("PinnedOld", 1, Some(stale)),
        ];
        let out = visible_people(
            &people,
            &[],
            &["People/PinnedOld".into()],
            now,
            true,
            Some(20),
        );
        assert_eq!(
            out.iter().map(|p| p.full_path.as_str()).collect::<Vec<_>>(),
            vec!["People/PinnedOld", "People/New"]
        );
    }

    #[test]
    fn nfc_hidden_matches_decomposed_tag() {
        let people = vec![TagSuggestion {
            id: "people/jose ruiz".into(),
            display_name: "Jose Ruiz".into(),
            full_path: "People/Jose\u{0301} Ruiz".into(),
            namespace: Some("People".into()),
            count: 1,
            latest_photo_date: Some(400.0),
        }];
        let out = visible_people(
            &people,
            &["People/Jos\u{00E9} Ruiz".into()],
            &[],
            500.0,
            false,
            None,
        );
        assert!(out.is_empty());
    }
}
