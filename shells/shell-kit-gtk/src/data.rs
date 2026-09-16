//! Display values a binding accepts. Ready to show; no domain record.

use gtk::gdk;
use localcore_ui::{ActionRole, StatusSeverity};

/// Leading visual for [`TextRowData`]. Avatars are 40px; symbols are symbolic icons.
#[derive(Debug, Clone)]
pub enum Leading {
    Avatar {
        text: String,
        texture: Option<gdk::Texture>,
    },
    Symbol(String),
}

#[derive(Debug, Clone, Default)]
pub struct TextRowData {
    pub title: String,
    pub subtitle: Option<String>,
    pub trailing: Option<String>,
    pub leading: Option<Leading>,
}

#[derive(Debug, Clone)]
pub struct MediaItemData {
    pub thumbnail_ref: String,
    pub label: Option<String>,
    pub badge: Option<String>,
    pub texture: Option<gdk::Texture>,
}

#[derive(Debug, Clone)]
pub struct ChoiceData {
    pub labels: Vec<String>,
    pub selected: u32,
}

#[derive(Debug, Clone)]
pub struct FieldRowData {
    pub label: String,
    pub value: String,
    pub editable: bool,
}

#[derive(Debug, Clone)]
pub struct ToggleRowData {
    pub label: String,
    pub on: bool,
}

#[derive(Debug, Clone)]
pub struct ActionRowData {
    pub label: String,
    pub role: ActionRole,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct NavRowData {
    pub label: String,
    pub trailing: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProgressRowData {
    pub label: String,
    pub fraction: Option<f64>,
    /// When true, the row shows a cancel suffix (R5).
    pub cancel: bool,
}

#[derive(Debug, Clone)]
pub struct StatusRowData {
    pub message: String,
    pub severity: StatusSeverity,
}

#[derive(Debug, Clone)]
pub struct ConfirmData {
    pub question: String,
    pub destructive_label: String,
}

/// Display-ready series. Values are already in the unit the title names;
/// the shell only sparks them.
#[derive(Debug, Clone)]
pub struct ChartRowData {
    pub title: String,
    pub subtitle: Option<String>,
    pub unit: Option<String>,
    pub latest: Option<String>,
    pub values: Vec<f64>,
}

/// L6 filter control. Scope is a small fixed set (`scope_toggle`);
/// Choice is open-ended (`choice_dropdown`).
#[derive(Debug, Clone)]
pub enum Filter {
    Scope(Vec<String>),
    Choice(ChoiceData),
}

/// One inset L5 section: optional heading plus a boxed list.
#[derive(Debug, Clone, Default)]
pub struct ListSection {
    pub heading: Option<String>,
}

/// ADR 0007 R3 empty-state cases, plus loading and error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyKind {
    NoFolder,
    EmptyFolder,
    NoMatches,
    Loading,
    Error,
}

/// Copy a shell passes to [`crate::empty_state`]. No colours.
#[derive(Debug, Clone)]
pub struct EmptyCopy {
    pub title: String,
    pub description: Option<String>,
    pub action: Option<String>,
}

/// Kind plus copy for [`ListScreen::empty`].
#[derive(Debug, Clone)]
pub struct EmptyState {
    pub kind: EmptyKind,
    pub copy: EmptyCopy,
}

/// Typed list-screen input. The kit lays this out; the app refills rows.
#[derive(Debug, Clone)]
pub struct ListScreen {
    pub search: bool,
    pub filter: Option<Filter>,
    pub sections: Vec<ListSection>,
    pub primary: Option<String>,
    pub selection: Option<Vec<ActionRowData>>,
    pub banner: Option<String>,
    pub empty: Option<EmptyState>,
}

impl Default for ListScreen {
    fn default() -> Self {
        Self {
            search: false,
            filter: None,
            sections: Vec::new(),
            primary: None,
            selection: None,
            banner: None,
            empty: None,
        }
    }
}
