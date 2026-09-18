//! Select mode, share (save), delete, and move. GTK paints; disk runs on workers.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;

use adw::prelude::*;
use gtk::gio;
use gtk::glib;
use localgallery::mutate::{
    create_subfolder, delete_photos, dest_is_under_root, export_photos, move_photos, DeleteResult,
    ExportResult, MoveResult, MutationTarget, ShareQuality,
};
use shell_kit_gtk::{
    action_row, confirm_dialog, form_sheet, nav_row, sheet, ActionRole, ActionRowData, ConfirmData,
    FormSheet, NavRowData, SheetSize,
};

use super::{find_named_widget, ViewerHost, Window};

pub(super) enum MutateOutcome {
    Deleted {
        result: DeleteResult,
        after_viewer: bool,
    },
    Moved {
        result: Result<MoveResult, String>,
        after_viewer: bool,
    },
    Exported {
        result: Result<ExportResult, String>,
    },
    FolderCreated {
        result: Result<PathBuf, String>,
    },
}

pub(super) struct MoveUi {
    dialog: adw::Dialog,
    list: gtk::ListBox,
    dest: Rc<RefCell<PathBuf>>,
}

#[derive(Clone, Copy)]
pub struct DeletePromptItem {
    pub is_video: bool,
}

#[must_use]
pub fn delete_prompt_title(photos: &[DeletePromptItem]) -> String {
    if photos.len() == 1 {
        return if photos[0].is_video {
            "Delete Video?".into()
        } else {
            "Delete Photo?".into()
        };
    }
    let videos = photos.iter().filter(|photo| photo.is_video).count();
    if videos == photos.len() {
        format!("Delete {} Videos?", photos.len())
    } else if videos == 0 {
        format!("Delete {} Photos?", photos.len())
    } else {
        format!("Delete {} Items?", photos.len())
    }
}

#[must_use]
pub fn delete_prompt_message(photos: &[DeletePromptItem]) -> String {
    if photos.len() == 1 {
        if photos[0].is_video {
            "This permanently deletes the video and its sidecar from disk.".into()
        } else {
            "This permanently deletes the photo and its sidecar from disk.".into()
        }
    } else {
        "This permanently deletes the files and their sidecars from disk.".into()
    }
}

fn prompt_items(targets: &[MutationTarget]) -> Vec<DeletePromptItem> {
    targets
        .iter()
        .map(|target| DeletePromptItem {
            is_video: target.is_video,
        })
        .collect()
}

pub(super) fn selection_actions() -> [ActionRowData; 3] {
    [
        ActionRowData {
            label: "Share".into(),
            role: ActionRole::Normal,
            enabled: false,
        },
        ActionRowData {
            label: "Move".into(),
            role: ActionRole::Normal,
            enabled: false,
        },
        ActionRowData {
            label: "Delete".into(),
            role: ActionRole::Destructive,
            enabled: false,
        },
    ]
}

pub(super) fn select_scope_actions() -> [ActionRowData; 3] {
    [
        ActionRowData {
            label: "Cancel".into(),
            role: ActionRole::Normal,
            enabled: true,
        },
        ActionRowData {
            label: "Select All".into(),
            role: ActionRole::Normal,
            enabled: true,
        },
        ActionRowData {
            label: "Deselect All".into(),
            role: ActionRole::Normal,
            enabled: false,
        },
    ]
}

#[cfg(test)]
pub(super) fn photo_context_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(Some("Open"), Some("photo.open"));
    menu.append(Some("Share"), Some("photo.share"));
    menu.append(Some("Move"), Some("photo.move"));
    menu.append(Some("Delete"), Some("photo.delete"));
    menu.append(Some("Select"), Some("photo.select"));
    menu
}

pub(super) fn set_tile_select_badge(item: &gtk::ListItem, on: bool) {
    if let Some(child) = item.child() {
        if let Some(badge) = find_named_widget::<gtk::Box>(&child, "select-badge") {
            badge.set_visible(on);
        }
    }
}

fn photo_id_name(id: &str) -> String {
    format!("photo:{id}")
}

pub(super) fn tile_photo_id(root: &gtk::Widget) -> Option<String> {
    let name = root.widget_name();
    if let Some(id) = name.strip_prefix("photo:") {
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }
    let mut child = root.first_child();
    while let Some(node) = child {
        if let Some(id) = tile_photo_id(&node) {
            return Some(id);
        }
        child = node.next_sibling();
    }
    None
}

fn photo_id_from_ancestors(start: &gtk::Widget) -> Option<String> {
    let mut current = Some(start.clone());
    while let Some(widget) = current {
        if let Some(id) = widget.widget_name().strip_prefix("photo:") {
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
        current = widget.parent();
    }
    None
}

fn photo_id_at(grid: &gtk::GridView, x: f64, y: f64) -> Option<String> {
    if let Some(picked) = grid.pick(x, y, gtk::PickFlags::DEFAULT) {
        if let Some(id) = photo_id_from_ancestors(&picked).or_else(|| tile_photo_id(&picked)) {
            return Some(id);
        }
    }
    let mut child = grid.first_child();
    while let Some(widget) = child {
        if let Some(bounds) = widget.compute_bounds(grid) {
            let left = f64::from(bounds.x());
            let top = f64::from(bounds.y());
            if x >= left
                && y >= top
                && x < left + f64::from(bounds.width())
                && y < top + f64::from(bounds.height())
            {
                if let Some(id) =
                    tile_photo_id(&widget).or_else(|| photo_id_from_ancestors(&widget))
                {
                    return Some(id);
                }
            }
        }
        child = widget.next_sibling();
    }
    None
}

fn apply_tile_checks(
    root: &gtk::Widget,
    selecting: bool,
    selected: &std::collections::BTreeSet<String>,
) {
    if let Some(badge) = find_named_widget::<gtk::Box>(root, "select-badge") {
        let on = selecting && tile_photo_id(root).is_some_and(|id| selected.contains(&id));
        badge.set_visible(on);
        return;
    }
    let mut child = root.first_child();
    while let Some(node) = child {
        apply_tile_checks(&node, selecting, selected);
        child = node.next_sibling();
    }
}

fn walk_photo_grids(root: &gtk::Widget, visit: &mut impl FnMut(&gtk::GridView)) {
    if let Ok(grid) = root.clone().downcast::<gtk::GridView>() {
        if grid.has_css_class("gallery-photos") || grid.has_css_class("gallery-folders") {
            visit(&grid);
        }
    }
    let mut child = root.first_child();
    while let Some(node) = child {
        walk_photo_grids(&node, visit);
        child = node.next_sibling();
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => a == b,
    }
}

fn folder_children<'a>(
    folders: &'a [gallery_ffi::ScannedFolderHost],
    dest: &Path,
    root: &Path,
) -> Vec<&'a gallery_ffi::ScannedFolderHost> {
    if let Some(index) = folders
        .iter()
        .position(|folder| Path::new(&folder.path) == dest)
    {
        return folders
            .iter()
            .filter(|folder| folder.parent_index == Some(index as u32))
            .collect();
    }
    if same_dir(dest, root) {
        return folders
            .iter()
            .filter(|folder| folder.parent_index.is_none())
            .filter(|folder| Path::new(&folder.path) != dest)
            .collect();
    }
    // Just-created dest is not in last_folders yet.
    Vec::new()
}

impl Window {
    pub(super) fn set_selecting(&self, on: bool) {
        self.inner.session.borrow_mut().set_selecting(on);
        self.inner.selection_bar.revealer.set_reveal_child(on);
        self.inner.select_scope_bar.revealer.set_reveal_child(on);
        // Take the Comet tab bar out of the toolbar so it cannot sit on
        // top of Share / Move / Delete and eat the clicks.
        if on {
            if self.inner.shell.switcher_bar.parent().is_some() {
                self.inner
                    .shell
                    .toolbar
                    .remove(&self.inner.shell.switcher_bar);
            }
        } else if self.inner.shell.switcher_bar.parent().is_none() {
            self.inner
                .shell
                .toolbar
                .add_bottom_bar(&self.inner.shell.switcher_bar);
        }
        self.sync_selection_bar();
        self.refresh_selection_tiles();
    }

    pub(super) fn select_visible_photos(&self) {
        if !self.inner.session.borrow().is_selecting() {
            return;
        }
        self.inner.session.borrow_mut().select_visible();
        self.sync_selection_bar();
        self.refresh_selection_tiles();
    }

    pub(super) fn deselect_all_photos(&self) {
        if !self.inner.session.borrow().is_selecting() {
            return;
        }
        self.inner.session.borrow_mut().clear_selection();
        self.sync_selection_bar();
        self.refresh_selection_tiles();
    }

    pub(super) fn toggle_photo_selected(&self, id: &str) {
        self.inner.session.borrow_mut().toggle_selected(id);
        self.sync_selection_bar();
        self.refresh_selection_tiles();
    }

    pub(super) fn activate_photo(&self, id: &str, host: ViewerHost) {
        if self.inner.session.borrow().is_selecting() {
            self.toggle_photo_selected(id);
        } else {
            self.push_viewer(id, host);
        }
    }

    pub(super) fn bind_tile_selection(&self, item: &gtk::ListItem, id: &str) {
        if let Some(child) = item.child() {
            child.set_widget_name(&photo_id_name(id));
        }
        let session = self.inner.session.borrow();
        set_tile_select_badge(item, session.is_selecting() && session.is_selected(id));
    }

    fn sync_selection_bar(&self) {
        let session = self.inner.session.borrow();
        let n = session.selected_ids().len();
        let selecting = session.is_selecting();
        let visible = session.visible_photo_ids();
        let all_visible_selected =
            !visible.is_empty() && visible.iter().all(|id| session.is_selected(id));
        drop(session);
        let label = format!("{n} selected");
        self.inner.selection_bar.count.set_label(&label);
        self.inner.select_scope_bar.count.set_label(&label);
        let enable = selecting && n > 0;
        for button in &self.inner.selection_bar.buttons {
            button.set_sensitive(enable);
        }
        if let Some(cancel) = self.inner.select_scope_bar.buttons.first() {
            cancel.set_sensitive(selecting);
        }
        if let Some(select_all) = self.inner.select_scope_bar.buttons.get(1) {
            select_all.set_sensitive(selecting && !visible.is_empty() && !all_visible_selected);
        }
        if let Some(deselect) = self.inner.select_scope_bar.buttons.get(2) {
            deselect.set_sensitive(enable);
        }
    }

    pub(super) fn cancel_select_mode(&self) {
        if self.inner.session.borrow().is_selecting() {
            self.set_selecting(false);
        }
    }

    pub(super) fn select_one_photo(&self, id: &str) {
        self.set_selecting(true);
        self.inner.session.borrow_mut().clear_selection();
        self.inner.session.borrow_mut().toggle_selected(id);
        self.sync_selection_bar();
        self.refresh_selection_tiles();
    }

    pub(super) fn refresh_selection_tiles(&self) {
        let session = self.inner.session.borrow();
        let selecting = session.is_selecting();
        let selected = session.selected_ids().clone();
        drop(session);
        let mut visit = |grid: &gtk::GridView| {
            let mut child = grid.first_child();
            while let Some(node) = child {
                apply_tile_checks(&node, selecting, &selected);
                child = node.next_sibling();
            }
        };
        visit(&self.inner.photos_grid);
        walk_photo_grids(&self.inner.folders_nav.clone().upcast(), &mut visit);
        walk_photo_grids(&self.inner.collections_nav.clone().upcast(), &mut visit);
        walk_photo_grids(&self.inner.photos_nav.clone().upcast(), &mut visit);
    }

    pub(super) fn share_selected(&self) {
        let targets = self.inner.session.borrow().selected_targets();
        if targets.is_empty() {
            self.toast("Select photos first");
            return;
        }
        self.present_share(targets, false);
    }

    pub(super) fn move_selected(&self) {
        let targets = self.inner.session.borrow().selected_targets();
        if targets.is_empty() {
            self.toast("Select photos first");
            return;
        }
        self.present_move(targets, false);
    }

    pub(super) fn delete_selected(&self) {
        let targets = self.inner.session.borrow().selected_targets();
        if targets.is_empty() {
            self.toast("Select photos first");
            return;
        }
        self.present_delete(targets, false);
    }

    pub(super) fn share_viewer_item(&self, id: &str) {
        let Some(target) = self.inner.session.borrow().target_for_id(id) else {
            self.toast("Photo is not in the library");
            return;
        };
        self.present_share(vec![target], true);
    }

    pub(super) fn move_viewer_item(&self, id: &str) {
        let Some(target) = self.inner.session.borrow().target_for_id(id) else {
            self.toast("Photo is not in the library");
            return;
        };
        self.present_move(vec![target], true);
    }

    pub(super) fn attach_photo_grid_menu(&self, grid: &gtk::GridView, host: ViewerHost) {
        // Claim on press so GtkListItemWidget cannot steal the sequence;
        // show on release so the matching button-up does not autohide the
        // popover (which looks like the menu does nothing).
        let pending = Rc::new(RefCell::new(None::<(String, f64, f64)>));
        let click = gtk::GestureClick::new();
        click.set_button(3);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = self.clone();
        let grid_click = grid.clone();
        let pending_press = pending.clone();
        click.connect_pressed(move |gesture, n_press, x, y| {
            if n_press != 1 || this.inner.session.borrow().is_selecting() {
                return;
            }
            let Some(id) = photo_id_at(&grid_click, x, y) else {
                return;
            };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            pending_press.replace(Some((id, x, y)));
        });
        let this = self.clone();
        let grid_click = grid.clone();
        click.connect_released(move |gesture, _n_press, _x, _y| {
            let Some((id, x, y)) = pending.take() else {
                return;
            };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            this.popup_photo_menu(&grid_click, x, y, host, id);
        });
        grid.add_controller(click);

        let alt_pending = Rc::new(RefCell::new(None::<(String, f64, f64)>));
        let alt = gtk::GestureClick::new();
        alt.set_button(1);
        alt.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = self.clone();
        let grid_alt = grid.clone();
        let alt_press = alt_pending.clone();
        alt.connect_pressed(move |gesture, n_press, x, y| {
            if n_press != 1 {
                return;
            }
            let context = gesture
                .current_event()
                .is_some_and(|event| event.triggers_context_menu());
            if !context || this.inner.session.borrow().is_selecting() {
                return;
            }
            let Some(id) = photo_id_at(&grid_alt, x, y) else {
                return;
            };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            alt_press.replace(Some((id, x, y)));
        });
        let this = self.clone();
        let grid_alt = grid.clone();
        alt.connect_released(move |gesture, _n_press, _x, _y| {
            let Some((id, x, y)) = alt_pending.take() else {
                return;
            };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            this.popup_photo_menu(&grid_alt, x, y, host, id);
        });
        grid.add_controller(alt);

        let long = gtk::GestureLongPress::new();
        long.set_touch_only(true);
        long.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = self.clone();
        let grid_long = grid.clone();
        long.connect_pressed(move |gesture, x, y| {
            if this.inner.session.borrow().is_selecting() {
                return;
            }
            let Some(id) = photo_id_at(&grid_long, x, y) else {
                return;
            };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            this.popup_photo_menu(&grid_long, x, y, host, id);
        });
        grid.add_controller(long);
    }

    fn popup_photo_menu(&self, grid: &gtk::GridView, x: f64, y: f64, host: ViewerHost, id: String) {
        if self.inner.session.borrow().is_selecting() {
            return;
        }
        let surface = self.inner.toast.clone();
        let point = grid.compute_point(&surface, &gtk::graphene::Point::new(x as f32, y as f32));
        let (px, py) = point
            .map(|point| (point.x().round() as i32, point.y().round() as i32))
            .unwrap_or((x.round() as i32, y.round() as i32));

        let popover = gtk::Popover::new();
        popover.set_autohide(true);
        popover.set_has_arrow(false);
        popover.set_halign(gtk::Align::Start);
        let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let add_item = |label: &str, destructive: bool, activate: Box<dyn Fn()>| {
            let button = gtk::Button::with_label(label);
            button.add_css_class("flat");
            button.set_hexpand(true);
            if destructive {
                button.add_css_class("destructive-action");
            }
            let pop = popover.clone();
            button.connect_clicked(move |_| {
                pop.popdown();
                activate();
            });
            list.append(&button);
        };
        let this = self.clone();
        let open_id = id.clone();
        add_item(
            "Open",
            false,
            Box::new(move || this.push_viewer(&open_id, host)),
        );
        let this = self.clone();
        let share_id = id.clone();
        add_item(
            "Share",
            false,
            Box::new(move || this.share_grid_item(&share_id)),
        );
        let this = self.clone();
        let move_id = id.clone();
        add_item(
            "Move",
            false,
            Box::new(move || this.move_grid_item(&move_id)),
        );
        let this = self.clone();
        let delete_id = id.clone();
        add_item(
            "Delete",
            true,
            Box::new(move || this.delete_grid_item(&delete_id)),
        );
        let this = self.clone();
        let select_id = id;
        add_item(
            "Select",
            false,
            Box::new(move || this.select_one_photo(&select_id)),
        );
        popover.set_child(Some(&list));
        popover.set_parent(&surface);
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(px, py, 1, 1)));
        popover.connect_closed(|popover| popover.unparent());
        // After the click sequence so autohide does not eat the same event.
        glib::idle_add_local_once(move || {
            popover.popup();
        });
    }

    fn share_grid_item(&self, id: &str) {
        let Some(target) = self.inner.session.borrow().target_for_id(id) else {
            self.toast("Photo is not in the library");
            return;
        };
        self.present_share(vec![target], false);
    }

    fn move_grid_item(&self, id: &str) {
        let Some(target) = self.inner.session.borrow().target_for_id(id) else {
            self.toast("Photo is not in the library");
            return;
        };
        self.present_move(vec![target], false);
    }

    fn delete_grid_item(&self, id: &str) {
        let Some(target) = self.inner.session.borrow().target_for_id(id) else {
            self.toast("Photo is not in the library");
            return;
        };
        self.present_delete(vec![target], false);
    }

    pub(super) fn delete_viewer_item(&self, id: &str) {
        let Some(target) = self.inner.session.borrow().target_for_id(id) else {
            self.toast("Photo is not in the library");
            return;
        };
        self.present_delete(vec![target], true);
    }

    fn present_delete(&self, targets: Vec<MutationTarget>, after_viewer: bool) {
        if targets.is_empty() {
            return;
        }
        let items = prompt_items(&targets);
        let dialog = confirm_dialog(&ConfirmData {
            question: delete_prompt_title(&items),
            destructive_label: "Delete".into(),
        });
        dialog.set_body(&delete_prompt_message(&items));
        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response != "confirm" {
                return;
            }
            this.start_delete(targets.clone(), after_viewer);
        });
        dialog.present(Some(&self.inner.window));
    }

    fn start_delete(&self, targets: Vec<MutationTarget>, after_viewer: bool) {
        let Some(tx) = self.begin_mutate_job("Deleting") else {
            return;
        };
        self.mute_watch();
        let work = self.inner.session.borrow().scan_work_arc();
        thread::spawn(move || {
            crate::session::Session::touch_shared_work(&work, "Deleting", None, None);
            let result = delete_photos(&targets);
            let _ = tx.send(MutateOutcome::Deleted {
                result,
                after_viewer,
            });
        });
    }

    fn present_share(&self, targets: Vec<MutationTarget>, after_viewer: bool) {
        if targets.is_empty() {
            return;
        }
        if targets.iter().any(|target| target.is_video) {
            self.pick_export_folder(targets, ShareQuality::Original, after_viewer);
            return;
        }
        let dialog = adw::AlertDialog::new(Some("Save"), Some("Choose a size"));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("original", "Original");
        dialog.add_response("high", "High (4096px)");
        dialog.add_response("medium", "Medium (2048px)");
        dialog.add_response("small", "Small (1024px)");
        dialog.set_default_response(Some("original"));
        dialog.set_close_response("cancel");
        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            let quality = match response {
                "original" => ShareQuality::Original,
                "high" => ShareQuality::High,
                "medium" => ShareQuality::Medium,
                "small" => ShareQuality::Small,
                _ => return,
            };
            this.pick_export_folder(targets.clone(), quality, after_viewer);
        });
        dialog.present(Some(&self.inner.window));
    }

    fn pick_export_folder(
        &self,
        targets: Vec<MutationTarget>,
        quality: ShareQuality,
        _after_viewer: bool,
    ) {
        let dialog = gtk::FileDialog::builder()
            .title("Save Items")
            .modal(true)
            .build();
        let window = self.inner.window.clone();
        let this = self.clone();
        dialog.select_folder(
            Some(&window),
            gio::Cancellable::NONE,
            move |result| match result {
                Ok(file) => match file.path() {
                    Some(path) => this.start_export(targets, path, quality),
                    None => this.toast("Folder path is not UTF-8"),
                },
                Err(err) => {
                    let msg = err.to_string();
                    if !msg.contains("Dismissed") && !msg.contains("dismissed") {
                        this.toast(&msg);
                    }
                }
            },
        );
    }

    fn start_export(&self, targets: Vec<MutationTarget>, dest: PathBuf, quality: ShareQuality) {
        let Some(tx) = self.begin_mutate_job("Exporting") else {
            return;
        };
        let work = self.inner.session.borrow().scan_work_arc();
        thread::spawn(move || {
            crate::session::Session::touch_shared_work(&work, "Exporting", None, None);
            let result = export_photos(&targets, &dest, quality).map_err(|err| err.to_string());
            let _ = tx.send(MutateOutcome::Exported { result });
        });
    }

    fn present_move(&self, targets: Vec<MutationTarget>, after_viewer: bool) {
        if targets.is_empty() {
            return;
        }
        let Some(root) = self.inner.session.borrow().folder().map(Path::to_path_buf) else {
            self.toast("Choose a Folder first");
            return;
        };
        let dest = Rc::new(RefCell::new(root.clone()));
        let list = gtk::ListBox::new();
        list.add_css_class("boxed-list");
        list.set_selection_mode(gtk::SelectionMode::None);
        let new_folder = action_row(&ActionRowData {
            label: "New Folder".into(),
            role: ActionRole::Normal,
            enabled: true,
        });
        let move_here = action_row(&ActionRowData {
            label: "Move Here".into(),
            role: ActionRole::Normal,
            enabled: true,
        });
        let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
        body.set_margin_top(12);
        body.set_margin_bottom(12);
        body.set_margin_start(12);
        body.set_margin_end(12);
        body.append(&list);
        body.append(&new_folder);
        body.append(&move_here);
        let dialog = sheet("Move", &body, SheetSize::Picker);
        self.refill_move_list(&list, &dest, &root);
        let this = self.clone();
        let dest_new = dest.clone();
        new_folder.connect_clicked(move |_| this.present_new_folder(dest_new.borrow().clone()));
        let this = self.clone();
        let dest_move = dest.clone();
        let batch = targets.clone();
        move_here.connect_clicked(move |_| {
            this.start_move(batch.clone(), dest_move.borrow().clone(), after_viewer);
        });
        self.inner.move_ui.replace(Some(MoveUi {
            dialog: dialog.clone(),
            list: list.clone(),
            dest,
        }));
        dialog.present(Some(&self.inner.window));
    }

    fn refill_move_list(&self, list: &gtk::ListBox, dest: &Rc<RefCell<PathBuf>>, root: &Path) {
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }
        let current = dest.borrow().clone();
        if !same_dir(&current, root) {
            if let Some(parent) = current.parent() {
                if dest_is_under_root(parent, root) {
                    let row = nav_row(&NavRowData {
                        label: "Up".into(),
                        trailing: None,
                    });
                    let this = self.clone();
                    let dest = dest.clone();
                    let list_act = list.clone();
                    let parent = parent.to_path_buf();
                    let root = root.to_path_buf();
                    row.connect_activated(move |_| {
                        dest.replace(parent.clone());
                        this.refill_move_list(&list_act, &dest, &root);
                    });
                    list.append(&row);
                }
            }
        }
        let folders = self.inner.session.borrow().last_folders().to_vec();
        let children = folder_children(&folders, &current, root);
        for folder in children {
            let row = nav_row(&NavRowData {
                label: folder.name.clone(),
                trailing: None,
            });
            let this = self.clone();
            let dest = dest.clone();
            let list_act = list.clone();
            let path = PathBuf::from(&folder.path);
            let root = root.to_path_buf();
            row.connect_activated(move |_| {
                dest.replace(path.clone());
                this.refill_move_list(&list_act, &dest, &root);
            });
            list.append(&row);
        }
    }

    fn present_new_folder(&self, parent: PathBuf) {
        let entry = gtk::Entry::new();
        entry.set_placeholder_text(Some("Folder name"));
        let built = form_sheet(FormSheet {
            title: "New Folder".into(),
            cancel: "Cancel".into(),
            confirm: "Create".into(),
            body: entry.clone().upcast(),
        });
        let this = self.clone();
        let dialog = built.dialog.clone();
        built.confirm.connect_clicked(move |_| {
            let name = entry.text().to_string();
            dialog.close();
            this.start_create_folder(parent.clone(), name);
        });
        built.dialog.present(Some(&self.inner.window));
    }

    fn start_create_folder(&self, parent: PathBuf, name: String) {
        let Some(root) = self.inner.session.borrow().folder().map(Path::to_path_buf) else {
            self.toast("Choose a Folder first");
            return;
        };
        let Some(tx) = self.begin_mutate_job("Moving") else {
            return;
        };
        self.mute_watch();
        let work = self.inner.session.borrow().scan_work_arc();
        thread::spawn(move || {
            crate::session::Session::touch_shared_work(&work, "Moving", None, None);
            let result = create_subfolder(&parent, &name, &root).map_err(|err| err.to_string());
            let _ = tx.send(MutateOutcome::FolderCreated { result });
        });
    }

    fn start_move(&self, targets: Vec<MutationTarget>, dest: PathBuf, after_viewer: bool) {
        let Some(root) = self.inner.session.borrow().folder().map(Path::to_path_buf) else {
            self.toast("Choose a Folder first");
            return;
        };
        if !dest_is_under_root(&dest, &root) {
            self.toast("Destination is outside the photo folder");
            return;
        }
        if !dest.is_dir() {
            self.toast("Destination is not a folder");
            return;
        }
        if let Some(ui) = self.inner.move_ui.borrow().as_ref() {
            ui.dialog.close();
        }
        let Some(tx) = self.begin_mutate_job("Moving") else {
            return;
        };
        self.inner.move_ui.replace(None);
        self.mute_watch();
        let work = self.inner.session.borrow().scan_work_arc();
        thread::spawn(move || {
            crate::session::Session::touch_shared_work(&work, "Moving", None, None);
            let result = move_photos(&targets, &dest, &root).map_err(|err| err.to_string());
            let _ = tx.send(MutateOutcome::Moved {
                result,
                after_viewer,
            });
        });
    }

    fn begin_mutate_job(&self, label: &str) -> Option<mpsc::Sender<MutateOutcome>> {
        if self.inner.mutate_rx.borrow().is_some() {
            self.toast("Wait for the current operation to finish");
            return None;
        }
        self.inner.session.borrow().begin_work(label);
        self.sync_chrome_progress();
        let (tx, rx) = mpsc::channel();
        self.inner.mutate_rx.replace(Some(rx));
        Some(tx)
    }

    pub(super) fn mute_watch(&self) {
        if let Some(gate) = self.inner.watch_mute.borrow().as_ref() {
            let _ = gate.set_muted(true);
        }
    }

    pub(super) fn unmute_watch(&self) {
        if let Some(gate) = self.inner.watch_mute.borrow().as_ref() {
            let _ = gate.set_muted(false);
        }
    }

    pub(super) fn poll_mutate(&self) {
        let Some(rx) = self.inner.mutate_rx.borrow_mut().take() else {
            return;
        };
        let outcome = match rx.try_recv() {
            Ok(outcome) => outcome,
            Err(mpsc::TryRecvError::Empty) => {
                self.inner.mutate_rx.replace(Some(rx));
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.unmute_watch();
                self.inner.session.borrow().clear_work();
                self.sync_chrome_progress();
                self.toast("Could not finish the file operation");
                return;
            }
        };
        match outcome {
            MutateOutcome::Deleted {
                result,
                after_viewer,
            } => self.finish_delete(result, after_viewer),
            MutateOutcome::Moved {
                result,
                after_viewer,
            } => self.finish_move(result, after_viewer),
            MutateOutcome::Exported { result } => self.finish_export(result),
            MutateOutcome::FolderCreated { result } => self.finish_create_folder(result),
        }
    }

    fn finish_delete(&self, result: DeleteResult, after_viewer: bool) {
        if !result.deleted_ids.is_empty() {
            self.apply_deleted_photos(&result.deleted_ids);
        }
        let deleted = result.deleted_ids.len();
        let failed = result.failed.len();
        if deleted > 0 && failed == 0 && result.partial_ids.is_empty() {
            self.toast(&format!(
                "Deleted {deleted} {}",
                if deleted == 1 { "item" } else { "items" }
            ));
        } else if failed > 0 && deleted == 0 {
            self.toast(&format!("Could not delete {failed} items"));
        } else {
            self.toast(&format!("Deleted {deleted}, failed {failed}"));
        }
        if after_viewer {
            let host = self.inner.last_viewer_host.get();
            let _ = self.viewer_nav(host).pop();
        }
        self.set_selecting(false);
        self.drain_watch();
        self.unmute_watch();
        self.drain_watch();
        self.inner.watch_ignore_ticks.set(4);
        self.inner.session.borrow().clear_work();
        self.sync_chrome_progress();
    }

    fn finish_move(&self, result: Result<MoveResult, String>, after_viewer: bool) {
        self.unmute_watch();
        match result {
            Ok(result) => {
                let moved = result.moved.len();
                let failed = result.failed.len();
                let skipped = result.skipped.len();
                if matches!((moved, failed, skipped), (0, 0, _)) {
                    self.toast("Nothing to move");
                } else if failed == 0 {
                    self.toast(&format!(
                        "Moved {moved} {}",
                        if moved == 1 { "item" } else { "items" }
                    ));
                } else {
                    self.toast(&format!("Moved {moved}, failed {failed}"));
                }
                self.after_library_mutation(after_viewer);
            }
            Err(error) => {
                self.toast(&error);
                self.inner.session.borrow().clear_work();
                self.sync_chrome_progress();
            }
        }
    }

    fn finish_export(&self, result: Result<ExportResult, String>) {
        match result {
            Ok(result) => {
                let saved = result.saved.len();
                let failed = result.failed.len();
                if failed == 0 {
                    self.toast(&format!(
                        "Saved {saved} {}",
                        if saved == 1 { "item" } else { "items" }
                    ));
                } else {
                    self.toast(&format!("Saved {saved}, failed {failed}"));
                }
            }
            Err(error) => self.toast(&error),
        }
        self.inner.session.borrow().clear_work();
        self.sync_chrome_progress();
    }

    fn finish_create_folder(&self, result: Result<PathBuf, String>) {
        self.unmute_watch();
        self.inner.session.borrow().clear_work();
        self.sync_chrome_progress();
        match result {
            Ok(path) => {
                if let Some(ui) = self.inner.move_ui.borrow().as_ref() {
                    ui.dest.replace(path.clone());
                    if let Some(root) = self.inner.session.borrow().folder().map(Path::to_path_buf)
                    {
                        self.refill_move_list(&ui.list, &ui.dest, &root);
                    }
                }
                self.toast("Folder created");
            }
            Err(error) => self.toast(&error),
        }
    }

    fn after_library_mutation(&self, after_viewer: bool) {
        if after_viewer {
            let host = self.inner.last_viewer_host.get();
            let _ = self.viewer_nav(host).pop();
        }
        self.set_selecting(false);
        self.reload_folder();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        delete_prompt_message, delete_prompt_title, photo_context_menu, select_scope_actions,
        selection_actions, DeletePromptItem,
    };

    #[test]
    fn selection_bar_is_share_move_delete() {
        let actions = selection_actions();
        assert_eq!(actions[0].label, "Share");
        assert_eq!(actions[1].label, "Move");
        assert_eq!(actions[2].label, "Delete");
    }

    #[test]
    fn select_scope_bar_leads_with_cancel() {
        let actions = select_scope_actions();
        assert_eq!(actions[0].label, "Cancel");
        assert!(actions[0].enabled);
        assert_eq!(actions[1].label, "Select All");
        assert_eq!(actions[2].label, "Deselect All");
        assert!(!actions[2].enabled);
    }

    #[test]
    fn photo_context_menu_lists_share_move_delete() {
        use gtk::prelude::MenuModelExt;
        let menu = photo_context_menu();
        assert_eq!(menu.n_items(), 5);
        let labels: Vec<String> = (0..menu.n_items())
            .filter_map(|i| {
                menu.item_attribute_value(i, "label", None)
                    .and_then(|value| value.get::<String>())
            })
            .collect();
        assert_eq!(labels, ["Open", "Share", "Move", "Delete", "Select"]);
    }

    #[test]
    fn photo_id_walks_tile_name() {
        use gtk::prelude::{BoxExt, Cast, WidgetExt};
        if !(gtk::is_initialized() || gtk::init().is_ok()) {
            return;
        }
        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.set_widget_name("photo:abc");
        let child = gtk::Picture::new();
        frame.append(&child);
        assert_eq!(
            super::photo_id_from_ancestors(child.upcast_ref()),
            Some("abc".into())
        );
        assert_eq!(super::tile_photo_id(frame.upcast_ref()), Some("abc".into()));
    }

    #[test]
    fn delete_prompt_matches_ios_copy() {
        let photo = [DeletePromptItem { is_video: false }];
        assert_eq!(delete_prompt_title(&photo), "Delete Photo?");
        assert!(delete_prompt_message(&photo).contains("sidecar"));
        let photos = [
            DeletePromptItem { is_video: false },
            DeletePromptItem { is_video: false },
        ];
        assert_eq!(delete_prompt_title(&photos), "Delete 2 Photos?");
        let videos = [
            DeletePromptItem { is_video: true },
            DeletePromptItem { is_video: true },
        ];
        assert_eq!(delete_prompt_title(&videos), "Delete 2 Videos?");
        let mixed = [
            DeletePromptItem { is_video: true },
            DeletePromptItem { is_video: false },
        ];
        assert_eq!(delete_prompt_title(&mixed), "Delete 2 Items?");
        assert_eq!(
            delete_prompt_title(&[DeletePromptItem { is_video: true }]),
            "Delete Video?"
        );
    }
}
