//! Measurable public-binding reuse between GTK products.

use std::collections::BTreeSet;

/// Public, domain-neutral shell-kit behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BindingId {
    AboutDialog,
    ActionRow,
    AdaptiveShell,
    Banner,
    ChromeProgress,
    ChoiceDropdown,
    ChipBar,
    ConfirmDialog,
    Diagnostics,
    EmptyState,
    FieldRow,
    FormSheet,
    ListPage,
    ListScreen,
    MediaItem,
    NavRow,
    NavigationView,
    Page,
    PreferencesDialog,
    ProgressRow,
    PrimaryAction,
    PrimaryMenu,
    SearchBar,
    SearchEntry,
    SelectionBar,
    SettingsPage,
    SettingsScreen,
    Sheet,
    SplitListDetail,
    StatusRow,
    TextRow,
    TokenCss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReuseMetrics {
    pub first_unique: usize,
    pub second_unique: usize,
    pub shared: usize,
}

/// Count distinct binding ids and their intersection.
///
/// Product crates publish the ids they actually call. Keeping this function
/// in the kit gives tests one stable, non-domain metric without teaching the
/// kit which products exist.
#[must_use]
pub fn measure_reuse(first: &[BindingId], second: &[BindingId]) -> ReuseMetrics {
    let first = first.iter().copied().collect::<BTreeSet<_>>();
    let second = second.iter().copied().collect::<BTreeSet<_>>();
    ReuseMetrics {
        first_unique: first.len(),
        second_unique: second.len(),
        shared: first.intersection(&second).count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuse_metric_is_distinct_and_symmetric() {
        let left = [BindingId::TextRow, BindingId::TextRow, BindingId::ListPage];
        let right = [BindingId::ListPage, BindingId::MediaItem];
        assert_eq!(
            measure_reuse(&left, &right),
            ReuseMetrics {
                first_unique: 2,
                second_unique: 2,
                shared: 1,
            }
        );
        assert_eq!(
            measure_reuse(&left, &right).shared,
            measure_reuse(&right, &left).shared
        );
    }
}
