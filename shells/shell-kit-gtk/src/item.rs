//! One native binding per ADR 0004 R4 item kind.

use adw::prelude::*;
use localcore_ui::{ActionRole, ItemKind, StatusSeverity};

use crate::data::{
    ActionRowData, ChartRowData, FieldRowData, Leading, MediaItemData, NavRowData, ProgressRowData,
    StatusRowData, TextRowData, ToggleRowData,
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
        | ItemKind::StatusRow
        | ItemKind::ChartRow => kind,
    }
}

fn dim_suffix(text: &str, numeric: bool) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("dim-label");
    if numeric {
        label.add_css_class("numeric");
    }
    label.set_valign(gtk::Align::Center);
    label.set_xalign(1.0);
    label
}

/// Build an `AdwActionRow` with markup off *before* the title is set.
/// `use-markup` defaults to true, so setting the title first parses `&` as an entity.
fn plain_action_row(title: &str) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_use_markup(false);
    row.set_title(title);
    row
}

fn apply_leading(row: &adw::ActionRow, leading: &Leading) {
    match leading {
        Leading::Avatar { text, texture } => {
            let avatar = adw::Avatar::new(40, Some(text.as_str()), true);
            if let Some(texture) = texture {
                avatar.set_custom_image(Some(texture));
            }
            avatar.set_valign(gtk::Align::Center);
            row.add_prefix(&avatar);
        }
        Leading::Symbol(icon) => {
            let image = gtk::Image::from_icon_name(icon);
            image.set_icon_size(gtk::IconSize::Normal);
            image.set_valign(gtk::Align::Center);
            row.add_prefix(&image);
        }
    }
}

pub fn text_row(data: &TextRowData) -> adw::ActionRow {
    let row = plain_action_row(&data.title);
    if let Some(sub) = &data.subtitle {
        row.set_subtitle(sub);
    }
    if let Some(leading) = &data.leading {
        apply_leading(&row, leading);
    }
    if let Some(trailing) = &data.trailing {
        row.add_suffix(&dim_suffix(trailing, true));
    }
    row
}

/// Native media row with a semantic symbol/file thumbnail fallback.
///
/// App-owned references such as `artwork:<id>` intentionally remain opaque
/// to the kit; the shell can replace the prefix image after host resolution.
pub fn media_item(data: &MediaItemData) -> adw::ActionRow {
    let row = plain_action_row(data.label.as_deref().unwrap_or_default());
    if let Some(badge) = &data.badge {
        row.set_subtitle(badge);
    }
    let image = if let Some(texture) = &data.texture {
        let image = gtk::Image::from_paintable(Some(texture));
        image.set_pixel_size(48);
        image
    } else if let Some(path) = data.thumbnail_ref.strip_prefix("file:") {
        gtk::Image::from_file(path)
    } else {
        let requested = data
            .thumbnail_ref
            .strip_prefix("symbol:")
            .unwrap_or("audio-x-generic");
        let symbol = match requested {
            "music-note" => "audio-x-generic",
            other => other,
        };
        let icon = if symbol.ends_with("-symbolic") {
            symbol.to_owned()
        } else {
            format!("{symbol}-symbolic")
        };
        gtk::Image::from_icon_name(&icon)
    };
    image.set_pixel_size(48);
    row.add_prefix(&image);
    row
}

/// Search result: symbolic type icon, title, Pango-markup subtitle.
pub fn search_hit_row(title: &str, subtitle_markup: &str, icon: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_activatable(true);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(12);
    content.set_margin_end(12);
    let image = gtk::Image::from_icon_name(icon);
    image.set_icon_size(gtk::IconSize::Normal);
    image.set_valign(gtk::Align::Start);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let title_label = gtk::Label::new(Some(title));
    title_label.set_xalign(0.0);
    title_label.set_wrap(true);
    title_label.add_css_class("heading");
    let subtitle = gtk::Label::new(None);
    subtitle.set_markup(subtitle_markup);
    subtitle.set_xalign(0.0);
    subtitle.set_wrap(true);
    subtitle.add_css_class("dim-label");
    text.append(&title_label);
    text.append(&subtitle);
    content.append(&image);
    content.append(&text);
    row.set_child(Some(&content));
    row
}

/// Bold the first case-insensitive occurrence of `query` inside `value`.
#[must_use]
pub fn highlight_markup(value: &str, query: &str) -> String {
    let query = query.trim();
    if query.is_empty() {
        return gtk::glib::markup_escape_text(value).to_string();
    }
    let hay = value.to_lowercase();
    let needle = query.to_lowercase();
    if hay.len() != value.len() || needle.len() != query.len() {
        return gtk::glib::markup_escape_text(value).to_string();
    }
    match hay.find(&needle) {
        Some(start) => {
            let end = start + needle.len();
            format!(
                "{}<b>{}</b>{}",
                gtk::glib::markup_escape_text(&value[..start]),
                gtk::glib::markup_escape_text(&value[start..end]),
                gtk::glib::markup_escape_text(&value[end..])
            )
        }
        None => gtk::glib::markup_escape_text(value).to_string(),
    }
}

pub fn field_row(data: &FieldRowData) -> adw::ActionRow {
    let row = plain_action_row(&data.label);
    row.set_subtitle(&data.value);
    row.set_title_lines(1);
    row
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

/// Toolbar / header action. List and settings use [`action_button_row`].
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

/// `AdwButtonRow` for list and settings actions (L7).
pub fn action_button_row(data: &ActionRowData) -> adw::ButtonRow {
    let row = adw::ButtonRow::builder()
        .title(&data.label)
        .sensitive(data.enabled)
        .build();
    if data.role == ActionRole::Destructive {
        row.add_css_class("destructive-action");
    }
    row
}

pub fn nav_row(data: &NavRowData) -> adw::ActionRow {
    let row = plain_action_row(&data.label);
    row.set_activatable(true);
    if let Some(trailing) = &data.trailing {
        row.add_suffix(&dim_suffix(trailing, false));
    }
    let chevron = gtk::Image::from_icon_name("go-next-symbolic");
    chevron.set_valign(gtk::Align::Center);
    row.add_suffix(&chevron);
    row
}

pub fn progress_row(data: &ProgressRowData) -> adw::ActionRow {
    let row = plain_action_row(&data.label);
    let bar = gtk::ProgressBar::new();
    bar.set_hexpand(true);
    bar.set_valign(gtk::Align::Center);
    match data.fraction {
        Some(fraction) => bar.set_fraction(fraction.clamp(0.0, 1.0)),
        None => bar.pulse(),
    }
    row.add_suffix(&bar);
    if data.cancel {
        let cancel = gtk::Button::from_icon_name("window-close-symbolic");
        cancel.set_valign(gtk::Align::Center);
        cancel.set_tooltip_text(Some("Cancel"));
        row.add_suffix(&cancel);
    }
    row
}

fn status_icon(severity: StatusSeverity) -> &'static str {
    match severity {
        StatusSeverity::Info => "dialog-information-symbolic",
        StatusSeverity::Warning => "dialog-warning-symbolic",
        StatusSeverity::Error => "dialog-error-symbolic",
    }
}

fn status_class(severity: StatusSeverity) -> Option<&'static str> {
    match severity {
        StatusSeverity::Info => None,
        StatusSeverity::Warning => Some("warning"),
        StatusSeverity::Error => Some("error"),
    }
}

pub fn status_row(data: &StatusRowData) -> adw::ActionRow {
    let row = plain_action_row(&data.message);
    let icon = gtk::Image::from_icon_name(status_icon(data.severity));
    icon.set_valign(gtk::Align::Center);
    row.add_prefix(&icon);
    if let Some(class) = status_class(data.severity) {
        row.add_css_class(class);
    }
    row
}

pub fn chart_row(data: &ChartRowData) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let header = plain_action_row(&data.title);
    if let Some(sub) = &data.subtitle {
        header.set_subtitle(sub);
    }
    if let Some(latest) = &data.latest {
        let trailing = match &data.unit {
            Some(unit) => format!("{latest} {unit}"),
            None => latest.clone(),
        };
        header.add_suffix(&dim_suffix(&trailing, true));
    }
    column.append(&header);

    let values = data.values.clone();
    let spark = gtk::DrawingArea::new();
    spark.set_content_height(48);
    spark.set_hexpand(true);
    spark.set_draw_func(move |area, cr, width, height| {
        let fg = area.color();
        cr.set_source_rgba(
            f64::from(fg.red()),
            f64::from(fg.green()),
            f64::from(fg.blue()),
            f64::from(fg.alpha()),
        );
        draw_sparkline(cr, width, height, &values);
    });
    column.append(&spark);
    column
}

fn draw_sparkline(cr: &gtk::cairo::Context, width: i32, height: i32, values: &[f64]) {
    if width <= 0 || height <= 0 {
        return;
    }
    if values.is_empty() {
        return;
    }
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let span = if (max - min).abs() < f64::EPSILON {
        1.0
    } else {
        max - min
    };
    let w = f64::from(width);
    let h = f64::from(height);
    let step = if values.len() == 1 {
        0.0
    } else {
        w / (values.len() - 1) as f64
    };
    cr.set_line_width(2.0);
    for (i, value) in values.iter().enumerate() {
        let x = step * i as f64;
        let y = h - ((value - min) / span) * (h - 4.0) - 2.0;
        if i == 0 {
            cr.move_to(x, y);
        } else {
            cr.line_to(x, y);
        }
    }
    let _ = cr.stroke();
}

pub fn item_widget(kind: ItemKind) -> gtk::Widget {
    match kind {
        ItemKind::TextRow => text_row(&TextRowData {
            title: String::new(),
            subtitle: None,
            trailing: None,
            leading: None,
        })
        .upcast(),
        ItemKind::MediaItem => media_item(&MediaItemData {
            thumbnail_ref: "symbol:audio-x-generic".into(),
            label: None,
            badge: None,
            texture: None,
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
            cancel: false,
        })
        .upcast(),
        ItemKind::StatusRow => status_row(&StatusRowData {
            message: String::new(),
            severity: StatusSeverity::Info,
        })
        .upcast(),
        ItemKind::ChartRow => chart_row(&ChartRowData {
            title: String::new(),
            subtitle: None,
            unit: None,
            latest: None,
            values: Vec::new(),
        })
        .upcast(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::prelude::{Cast, IsA, WidgetExt};

    fn find_widget<T: IsA<gtk::Widget>>(root: &gtk::Widget) -> Option<T> {
        if let Ok(hit) = root.clone().downcast::<T>() {
            return Some(hit);
        }
        let mut child = root.first_child();
        while let Some(node) = child {
            if let Some(hit) = find_widget::<T>(&node) {
                return Some(hit);
            }
            child = node.next_sibling();
        }
        None
    }

    fn find_label_with_class(root: &gtk::Widget, class: &str) -> Option<gtk::Label> {
        if let Ok(label) = root.clone().downcast::<gtk::Label>() {
            if label.has_css_class(class) {
                return Some(label);
            }
        }
        let mut child = root.first_child();
        while let Some(node) = child {
            if let Some(hit) = find_label_with_class(&node, class) {
                return Some(hit);
            }
            child = node.next_sibling();
        }
        None
    }

    fn find_image_with_icon(root: &gtk::Widget, icon: &str) -> Option<gtk::Image> {
        if let Ok(image) = root.clone().downcast::<gtk::Image>() {
            if image.icon_name().as_deref() == Some(icon) {
                return Some(image);
            }
        }
        let mut child = root.first_child();
        while let Some(node) = child {
            if let Some(hit) = find_image_with_icon(&node, icon) {
                return Some(hit);
            }
            child = node.next_sibling();
        }
        None
    }

    #[test]
    fn action_rows_keep_ampersands_as_plain_text() {
        crate::with_adw(|| {
            let title = "Straight Up Drum & Bass! Vol. 4";
            let row = text_row(&TextRowData {
                title: title.into(),
                subtitle: Some("A & B".into()),
                trailing: None,
                leading: None,
            });
            assert!(
                !row.uses_markup(),
                "title must not be parsed as Pango markup"
            );
            assert_eq!(row.title().as_str(), title);
            assert_eq!(row.subtitle().as_deref(), Some("A & B"));

            let media = media_item(&MediaItemData {
                thumbnail_ref: "symbol:audio-x-generic".into(),
                label: Some(title.into()),
                badge: Some("Artist & Friends".into()),
                texture: None,
            });
            assert!(!media.uses_markup());
            assert_eq!(media.title().as_str(), title);
        });
    }

    #[test]
    fn text_row_leading_and_trailing_are_present() {
        crate::with_adw(|| {
            let row = text_row(&TextRowData {
                title: "Ada Lovelace".into(),
                subtitle: Some("Mathematician".into()),
                trailing: Some("2".into()),
                leading: Some(Leading::Avatar {
                    text: "Ada Lovelace".into(),
                    texture: None,
                }),
            });
            let avatar = find_widget::<adw::Avatar>(row.upcast_ref()).expect("leading avatar");
            assert_eq!(avatar.text().as_deref(), Some("Ada Lovelace"));
            assert_eq!(avatar.size(), 40);
            assert!(avatar.shows_initials());
            let trailing = find_label_with_class(row.upcast_ref(), "numeric").expect("trailing");
            assert!(trailing.has_css_class("dim-label"));
            assert_eq!(trailing.valign(), gtk::Align::Center);
            assert_eq!(trailing.label().as_str(), "2");

            let symbol = text_row(&TextRowData {
                title: "Logs".into(),
                subtitle: None,
                trailing: None,
                leading: Some(Leading::Symbol("utilities-system-monitor-symbolic".into())),
            });
            assert!(
                find_image_with_icon(symbol.upcast_ref(), "utilities-system-monitor-symbolic")
                    .is_some()
            );
        });
    }

    #[test]
    fn status_row_icon_matches_severity() {
        crate::with_adw(|| {
            let cases = [
                (StatusSeverity::Info, "dialog-information-symbolic", None),
                (
                    StatusSeverity::Warning,
                    "dialog-warning-symbolic",
                    Some("warning"),
                ),
                (
                    StatusSeverity::Error,
                    "dialog-error-symbolic",
                    Some("error"),
                ),
            ];
            for (severity, icon, class) in cases {
                let row = status_row(&StatusRowData {
                    message: "state".into(),
                    severity,
                });
                assert!(
                    find_image_with_icon(row.upcast_ref(), icon).is_some(),
                    "missing {icon} for {severity:?}"
                );
                assert!(
                    row.subtitle().as_deref().unwrap_or_default().is_empty(),
                    "severity must not be a subtitle"
                );
                if let Some(class) = class {
                    assert!(row.has_css_class(class), "missing {class} on {severity:?}");
                } else {
                    assert!(!row.has_css_class("success"));
                    assert!(!row.has_css_class("warning"));
                    assert!(!row.has_css_class("error"));
                }
            }
        });
    }

    #[test]
    fn chart_row_trailing_is_dim_numeric() {
        crate::with_adw(|| {
            let chart = chart_row(&ChartRowData {
                title: "Steps".into(),
                subtitle: None,
                unit: Some("steps".into()),
                latest: Some("8,432".into()),
                values: vec![1.0, 2.0, 3.0],
            });
            let trailing =
                find_label_with_class(chart.upcast_ref(), "numeric").expect("latest suffix");
            assert!(trailing.has_css_class("dim-label"));
            assert_eq!(trailing.valign(), gtk::Align::Center);
            assert_eq!(trailing.label().as_str(), "8,432 steps");
        });
    }

    #[test]
    fn action_button_row_marks_destructive() {
        crate::with_adw(|| {
            let row = action_button_row(&ActionRowData {
                label: "Delete".into(),
                role: ActionRole::Destructive,
                enabled: true,
            });
            assert!(row.has_css_class("destructive-action"));
            assert_eq!(row.title().as_str(), "Delete");
        });
    }

    #[test]
    fn highlight_markup_wraps_the_first_match() {
        assert_eq!(
            highlight_markup("650-555-2718", "650"),
            "<b>650</b>-555-2718"
        );
        assert_eq!(highlight_markup("Ada", ""), "Ada");
    }
}
