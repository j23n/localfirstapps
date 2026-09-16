//! Shared, generation-checked view-model vocabulary.
//!
//! A structure read is intentionally separate from a content read. Structure
//! contains only state, section/id/action metadata, and a generation. Content
//! is returned as one bounded window of display-ready rows. Holding the
//! generation at both ends prevents a window from being joined to ids from a
//! newer projection.

/// Largest content window accepted at the FFI boundary.
///
/// 256 is larger than every currently visible Gallery viewport, including
/// prefetch, while keeping one accidental "load everything" request bounded.
pub const MAX_VIEW_WINDOW: usize = 256;

/// Whole-screen content state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ViewContentState {
    Loading,
    Empty,
    Content,
    Error,
}

/// ADR 0004 slot kind used by one structure section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ViewSlotKind {
    TextRow,
    MediaItem,
    ActionRow,
}

/// Metadata for one section. Item content is deliberately absent.
///
/// R6 role: structure DTO.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ViewSection {
    pub id: String,
    pub title: String,
    pub slot_kind: ViewSlotKind,
    pub item_ids: Vec<String>,
}

/// One action advertised by a structure read.
///
/// R6 role: structure DTO.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ViewAction {
    pub id: String,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
}

/// Cheap whole-screen structure. No formatted collection content appears
/// here; callers fetch that through a bounded window method.
///
/// R6 role: structure DTO.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ViewStructure {
    pub state: ViewContentState,
    pub sections: Vec<ViewSection>,
    pub actions: Vec<ViewAction>,
    pub generation: u64,
}

/// Display-ready ADR 0004 media item.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct GalleryMediaItem {
    pub id: String,
    pub thumbnail_ref: String,
    /// Pre-formatted UTC capture time, used by the visible-range chrome.
    pub label: Option<String>,
    /// Spoken filename; separate from the visual/date label.
    pub accessibility_label: Option<String>,
    pub badge: Option<String>,
}

/// Display-ready ADR 0004 text row.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct GalleryTextRow {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub trailing: Option<String>,
}

/// Typed refusal from a generation-checked window read.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum ViewError {
    StaleGeneration {
        requested: u64,
        current: u64,
        message: String,
        user_actionable: bool,
    },
    WindowTooLarge {
        requested: u64,
        maximum: u64,
        message: String,
        user_actionable: bool,
    },
    SectionNotFound {
        section_id: String,
        message: String,
        user_actionable: bool,
    },
}

impl std::fmt::Display for ViewError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StaleGeneration { message, .. }
            | Self::WindowTooLarge { message, .. }
            | Self::SectionNotFound { message, .. } => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ViewError {}

pub(crate) fn checked_window(
    requested_generation: u64,
    current_generation: u64,
    offset: u64,
    limit: u64,
    len: usize,
) -> Result<std::ops::Range<usize>, ViewError> {
    if requested_generation != current_generation {
        return Err(ViewError::StaleGeneration {
            requested: requested_generation,
            current: current_generation,
            message: "This Gallery view changed. Reload its structure and retry.".into(),
            user_actionable: true,
        });
    }
    let requested = usize::try_from(limit).unwrap_or(usize::MAX);
    if requested > MAX_VIEW_WINDOW {
        return Err(ViewError::WindowTooLarge {
            requested: limit,
            maximum: MAX_VIEW_WINDOW as u64,
            message: format!("A Gallery content window cannot exceed {MAX_VIEW_WINDOW} items."),
            user_actionable: false,
        });
    }
    let start = usize::try_from(offset).unwrap_or(usize::MAX).min(len);
    let end = start.saturating_add(requested).min(len);
    Ok(start..end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_generation_is_a_typed_refusal() {
        assert!(matches!(
            checked_window(4, 5, 0, 20, 100),
            Err(ViewError::StaleGeneration {
                requested: 4,
                current: 5,
                ..
            })
        ));
    }

    #[test]
    fn windows_are_bounded_before_allocation() {
        assert!(matches!(
            checked_window(7, 7, 0, MAX_VIEW_WINDOW as u64 + 1, 20_000),
            Err(ViewError::WindowTooLarge { .. })
        ));
        assert_eq!(
            checked_window(7, 7, 19_990, 40, 20_000).unwrap(),
            19_990..20_000
        );
    }
}
