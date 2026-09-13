//! One native binding per ADR 0004 R4 item kind.

use adw::prelude::*;
use localcore_ui::{ActionRole, ItemKind};

use crate::data::{
    ActionRowData, FieldRowData, MediaItemData, NavRowData, ProgressRowData, StatusRowData,
    TextRowData, ToggleRowData,
};

/// Proves every item kind has a binding. A new kind without an arm
/// fails the build (ADR 0004 R7).
pub fn bind_item(kind: ItemKind) -> ItemKind {
    match kind {
        ItemKind::TextRow
        | ItemKind::MediaItem
        | ItemKind::FieldRow
        | ItemKind::ToggleRow
        | ItemKind::ActionRow
        | ItemKind::NavRow
        | ItemKind::ProgressRow
        | ItemKind::StatusRow => kind,
    }
}

pub fn text_row(data: &TextRowData) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(&data.title).build();
    if let Some(sub) = &data.subtitle {
        row.set_subtitle(sub);
    }
    if let Some(trailing) = &data.trailing {
        row.add_suffix(&gtk::Label::new(Some(trailing)));
    }
    row
}

pub fn media_item(data: &MediaItemData) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
    column.append(&gtk::Picture::new());
    if let Some(label) = &data.label {
        column.append(&gtk::Label::new(Some(label)));
    }
    if let Some(badge) = &data.badge {
        column.append(&gtk::Label::new(Some(badge)));
    }
    column
}

pub fn field_row(data: &FieldRowData) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(&data.label)
        .subtitle(&data.value)
        .build()
}

pub fn field_row_widget(data: &FieldRowData) -> gtk::Widget {
    if data.editable {
        let row = adw::EntryRow::builder().title(&data.label).build();
        row.set_text(&data.value);
        return row.upcast();
    }
    field_row(data).upcast()
}

pub fn toggle_row(data: &ToggleRowData) -> adw::SwitchRow {
    adw::SwitchRow::builder()
        .title(&data.label)
        .active(data.on)
        .build()
}

pub fn action_row(data: &ActionRowData) -> gtk::Button {
    let button = gtk::Button::builder()
        .label(&data.label)
        .sensitive(data.enabled)
        .build();
    if data.role == ActionRole::Destructive {
        button.add_css_class("destructive-action");
    }
    button
}

pub fn nav_row(data: &NavRowData) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&data.label)
        .activatable(true)
        .build();
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    if let Some(trailing) = &data.trailing {
        row.set_subtitle(trailing);
    }
    row
}

pub fn progress_row(data: &ProgressRowData) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
    column.append(&gtk::Label::new(Some(&data.label)));
    let bar = gtk::ProgressBar::new();
    match data.fraction {
        Some(fraction) => bar.set_fraction(fraction.clamp(0.0, 1.0)),
        None => bar.pulse(),
    }
    column.append(&bar);
    column
}

pub fn status_row(data: &StatusRowData) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(&data.message)
        .subtitle(data.severity.as_str())
        .build()
}

pub fn item_widget(kind: ItemKind) -> gtk::Widget {
    match kind {
        ItemKind::TextRow => text_row(&TextRowData {
            title: String::new(),
            subtitle: None,
            trailing: None,
        })
        .upcast(),
        ItemKind::MediaItem => media_item(&MediaItemData {
            label: None,
            badge: None,
        })
        .upcast(),
        ItemKind::FieldRow => field_row_widget(&FieldRowData {
            label: String::new(),
            value: String::new(),
            editable: false,
        }),
        ItemKind::ToggleRow => toggle_row(&ToggleRowData {
            label: String::new(),
            on: false,
        })
        .upcast(),
        ItemKind::ActionRow => action_row(&ActionRowData {
            label: String::new(),
            role: ActionRole::Normal,
            enabled: true,
        })
        .upcast(),
        ItemKind::NavRow => nav_row(&NavRowData {
            label: String::new(),
            trailing: None,
        })
        .upcast(),
        ItemKind::ProgressRow => progress_row(&ProgressRowData {
            label: String::new(),
            fraction: None,
        })
        .upcast(),
        ItemKind::StatusRow => status_row(&StatusRowData {
            message: String::new(),
            severity: localcore_ui::StatusSeverity::Info,
        })
        .upcast(),
    }
}
