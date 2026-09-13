//! Display values a binding accepts. Ready to show; no domain record.

use localcore_ui::{ActionRole, StatusSeverity};

#[derive(Debug, Clone)]
pub struct TextRowData {
    pub title: String,
    pub subtitle: Option<String>,
    pub trailing: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MediaItemData {
    pub label: Option<String>,
    pub badge: Option<String>,
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
