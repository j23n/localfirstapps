//! One native binding per ADR 0004 R4 affordance.

use adw::prelude::*;
use gtk::gio;
use localcore_ui::Affordance;

use crate::data::{Chip, ChipMode, ChoiceData, ConfirmData, ProgressRowData};

/// Proves every affordance has a binding.
pub fn bind_affordance(kind: Affordance) -> Affordance {
    match kind {
        Affordance::Search
        | Affordance::Filter
        | Affordance::Sort
        | Affordance::Selection
        | Affordance::PrimaryAction
        | Affordance::Overflow
        | Affordance::Banner
        | Affordance::Progress
        | Affordance::Confirm => kind,
    }
}

/// Search field. [`crate::list_screen`] places this; keep the helper
/// as a thin constructor for callers that still assemble by hand.
pub fn search_entry() -> gtk::SearchEntry {
    gtk::SearchEntry::new()
}

/// L6 small fixed set: libadwaita `ToggleGroup` (floor Adw 1.9).
pub fn scope_toggle(options: &[impl AsRef<str>]) -> adw::ToggleGroup {
    let group = adw::ToggleGroup::new();
    for (index, option) in options.iter().enumerate() {
        let label = option.as_ref();
        let toggle = adw::Toggle::builder()
            .name(format!("scope-{index}"))
            .label(label)
            .build();
        group.add(toggle);
    }
    if !options.is_empty() {
        group.set_active(0);
    }
    group
}

pub fn filter_button() -> gtk::MenuButton {
    gtk::MenuButton::builder()
        .icon_name("funnel-symbolic")
        .build()
}

pub fn sort_button() -> gtk::MenuButton {
    gtk::MenuButton::builder()
        .icon_name("view-sort-descending-symbolic")
        .build()
}

/// Native single-choice control used by sort and named-predicate pickers.
pub fn choice_dropdown(data: &ChoiceData) -> gtk::DropDown {
    let refs = data.labels.iter().map(String::as_str).collect::<Vec<_>>();
    let dropdown = gtk::DropDown::from_strings(&refs);
    let selected = if data.labels.is_empty() {
        gtk::INVALID_LIST_POSITION
    } else {
        data.selected
            .min(data.labels.len().saturating_sub(1) as u32)
    };
    dropdown.set_selected(selected);
    dropdown
}

pub fn primary_action(label: &str) -> gtk::Button {
    let button = gtk::Button::builder().label(label).build();
    button.add_css_class("suggested-action");
    button
}

/// Header-bar icon action (page-level create/add).
pub fn header_action(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .build()
}

/// In-content primary (`pill suggested-action`).
pub fn inline_primary(label: &str, icon: &str) -> gtk::Button {
    let button = gtk::Button::builder().label(label).icon_name(icon).build();
    button.add_css_class("pill");
    button.add_css_class("suggested-action");
    button
}

/// Overflow menu built from a `gio::Menu` the app passes.
pub fn overflow(menu: &gio::Menu) -> gtk::MenuButton {
    gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .menu_model(menu)
        .build()
}

pub fn overflow_button() -> gtk::MenuButton {
    gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .build()
}

pub fn banner(title: &str) -> adw::Banner {
    adw::Banner::builder().title(title).revealed(true).build()
}

/// Header-bar chrome for affordance `progress`. Leaves `AdwBanner` for folder status.
#[derive(Clone)]
pub struct ChromeProgress {
    pub root: gtk::Box,
    pub spinner: gtk::Spinner,
    pub label: gtk::Label,
    cancel: gtk::Button,
}

/// Spinner + phase + optional Cancel. Hidden until the shell applies a job.
#[must_use]
pub fn chrome_progress() -> ChromeProgress {
    let spinner = gtk::Spinner::new();
    spinner.set_valign(gtk::Align::Center);
    let label = gtk::Label::new(None);
    label.add_css_class("caption-heading");
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_max_width_chars(28);
    let cancel = gtk::Button::from_icon_name("window-close-symbolic");
    cancel.set_valign(gtk::Align::Center);
    cancel.set_tooltip_text(Some("Cancel"));
    cancel.add_css_class("flat");
    cancel.set_visible(false);
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    root.add_css_class("chrome-progress");
    root.set_valign(gtk::Align::Center);
    root.append(&spinner);
    root.append(&label);
    root.append(&cancel);
    root.set_visible(false);
    ChromeProgress {
        root,
        spinner,
        label,
        cancel,
    }
}

impl ChromeProgress {
    /// Show or hide from a revealed [`ProgressRowData`].
    pub fn apply(&self, progress: Option<&ProgressRowData>) {
        match progress {
            Some(data) => {
                self.root.set_visible(true);
                self.spinner.start();
                let text = match &data.detail {
                    Some(detail) if !detail.is_empty() => format!("{}  {detail}", data.label),
                    _ => data.label.clone(),
                };
                self.label.set_text(&text);
                self.cancel.set_visible(data.cancel);
            }
            None => {
                self.spinner.stop();
                self.root.set_visible(false);
                self.cancel.set_visible(false);
            }
        }
    }

    /// Cancel button for the shell to hook.
    #[must_use]
    pub fn cancel_button(&self) -> &gtk::Button {
        &self.cancel
    }
}

/// Pill chips in an `AdwWrapBox`. Removable chips add a close icon.
///
/// `GtkButton.icon_name` replaces the child, so removable chips must
/// compose label + close (and an optional leading icon) themselves.
pub fn chip_bar(chips: &[Chip]) -> adw::WrapBox {
    let bar = adw::WrapBox::new();
    bar.set_child_spacing(8);
    bar.set_line_spacing(8);
    for chip in chips {
        bar.append(&chip_button(chip));
    }
    bar
}

fn chip_button(chip: &Chip) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("pill");
    button.set_halign(gtk::Align::Start);
    button.set_hexpand(false);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    if let Some(icon) = chip.icon.as_deref() {
        let image = gtk::Image::from_icon_name(icon);
        image.set_icon_size(gtk::IconSize::Normal);
        row.append(&image);
    }
    let label = gtk::Label::new(Some(&chip.label));
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_max_width_chars(24);
    row.append(&label);
    if chip.mode == ChipMode::Removable {
        let close = gtk::Image::from_icon_name("window-close-symbolic");
        close.set_icon_size(gtk::IconSize::Normal);
        row.append(&close);
        button.set_tooltip_text(Some(&format!("Remove {}", chip.label)));
    }
    button.set_child(Some(&row));
    button
}

pub fn confirm_dialog(data: &ConfirmData) -> adw::AlertDialog {
    let dialog = adw::AlertDialog::new(Some(&data.question), None);
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("confirm", &data.destructive_label);
    dialog.set_response_appearance("confirm", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog
}

pub fn affordance_widget(kind: Affordance) -> gtk::Widget {
    match kind {
        Affordance::Search => search_entry().upcast(),
        Affordance::Filter => filter_button().upcast(),
        Affordance::Sort => sort_button().upcast(),
        Affordance::Selection => gtk::ToggleButton::new().upcast(),
        Affordance::PrimaryAction => primary_action("").upcast(),
        Affordance::Overflow => overflow_button().upcast(),
        Affordance::Banner => banner("").upcast(),
        Affordance::Progress => chrome_progress().root.upcast(),
        Affordance::Confirm => {
            // AlertDialog is not a Widget; the chrome that offers
            // confirm is a destructive button that presents it.
            primary_action("").upcast()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::prelude::{Cast, WidgetExt};

    #[test]
    fn scope_toggle_is_a_toggle_group() {
        crate::with_adw(|| {
            let group = scope_toggle(&["All".to_string(), "Info".to_string()]);
            assert_eq!(group.n_toggles(), 2);
            assert_eq!(group.active(), 0);
        });
    }

    #[test]
    fn header_action_and_inline_primary_use_libadwaita_classes() {
        crate::with_adw(|| {
            let header = header_action("list-add-symbolic", "Add");
            assert_eq!(header.icon_name().as_deref(), Some("list-add-symbolic"));
            assert_eq!(header.tooltip_text().as_deref(), Some("Add"));

            let primary = inline_primary("Play All", "media-playback-start-symbolic");
            assert!(primary.has_css_class("pill"));
            assert!(primary.has_css_class("suggested-action"));
        });
    }

    fn find_image_with_icon(root: &gtk::Widget, icon: &str) -> bool {
        if let Ok(image) = root.clone().downcast::<gtk::Image>() {
            if image.icon_name().as_deref() == Some(icon) {
                return true;
            }
        }
        let mut child = root.first_child();
        while let Some(node) = child {
            if find_image_with_icon(&node, icon) {
                return true;
            }
            child = node.next_sibling();
        }
        false
    }

    fn find_label(root: &gtk::Widget, text: &str) -> bool {
        if let Ok(label) = root.clone().downcast::<gtk::Label>() {
            if label.text() == text {
                return true;
            }
        }
        let mut child = root.first_child();
        while let Some(node) = child {
            if find_label(&node, text) {
                return true;
            }
            child = node.next_sibling();
        }
        false
    }

    #[test]
    fn removable_chip_bar_has_close_icon() {
        crate::with_adw(|| {
            let bar = chip_bar(&[Chip {
                label: "Vacation".into(),
                mode: ChipMode::Removable,
                icon: Some("tag-symbolic".into()),
            }]);
            let root = bar.upcast_ref();
            assert!(find_image_with_icon(root, "window-close-symbolic"));
            assert!(find_image_with_icon(root, "tag-symbolic"));
            assert!(find_label(root, "Vacation"));
        });
    }

    #[test]
    fn display_chip_bar_has_no_close_icon() {
        crate::with_adw(|| {
            let bar = chip_bar(&[Chip {
                label: "Friends".into(),
                mode: ChipMode::Display,
                icon: None,
            }]);
            let root = bar.upcast_ref();
            assert!(!find_image_with_icon(root, "window-close-symbolic"));
            assert!(find_label(root, "Friends"));
        });
    }

    #[test]
    fn overflow_uses_the_passed_menu() {
        crate::with_adw(|| {
            let menu = gio::Menu::new();
            menu.append(Some("Export"), Some("win.export"));
            let button = overflow(&menu);
            assert_eq!(button.icon_name().as_deref(), Some("view-more-symbolic"));
            assert!(button.menu_model().is_some());
        });
    }

    #[test]
    fn chrome_progress_stays_hidden_until_applied() {
        crate::with_adw(|| {
            let chrome = chrome_progress();
            assert!(!chrome.root.is_visible());
            chrome.apply(Some(&ProgressRowData {
                label: "Scanning".into(),
                detail: Some("12 found".into()),
                fraction: None,
                cancel: true,
            }));
            assert!(chrome.root.is_visible());
            assert!(chrome.cancel_button().is_visible());
            assert_eq!(chrome.label.text().as_str(), "Scanning  12 found");
            chrome.apply(None);
            assert!(!chrome.root.is_visible());
        });
    }
}
