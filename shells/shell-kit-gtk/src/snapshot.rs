//! Debug-only window capture for the GTK design-pass harness.
//!
//! After the first mapped/rendered frame the window is drawn through
//! [`gtk::WidgetPaintable`] → [`gtk::Snapshot`] →
//! [`gtk::gsk::GskRendererExt::render_texture`] →
//! [`gtk::gdk::TextureExt::save_to_png`], then the application quits.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gtk::gdk::prelude::*;
use gtk::gsk::prelude::*;
use gtk::prelude::*;

/// Why a capture could not be written.
#[derive(Debug)]
pub enum SnapshotError {
    /// The widget has no allocated or intrinsic size yet.
    NoSize,
    /// No GSK renderer is attached to the native surface.
    NoRenderer,
    /// [`gtk::WidgetPaintable`] produced no render node.
    EmptyNode,
    /// Creating the destination directory or writing the PNG failed.
    Save(String),
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSize => formatter.write_str("window has no mapped size"),
            Self::NoRenderer => formatter.write_str("no GSK renderer for the native surface"),
            Self::EmptyNode => {
                formatter.write_str("widget paintable produced an empty render node")
            }
            Self::Save(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for SnapshotError {}

/// Render `widget` to `path` as a PNG.
pub fn capture_widget(
    widget: &impl IsA<gtk::Widget>,
    path: impl AsRef<Path>,
) -> Result<(), SnapshotError> {
    let widget = widget.as_ref();
    let paintable = gtk::WidgetPaintable::new(Some(widget));
    let width = first_positive(widget.width(), paintable.intrinsic_width());
    let height = first_positive(widget.height(), paintable.intrinsic_height());
    let (width, height) = match (width, height) {
        (Some(width), Some(height)) => (width, height),
        _ => return Err(SnapshotError::NoSize),
    };

    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, f64::from(width), f64::from(height));
    let node = snapshot.to_node().ok_or(SnapshotError::EmptyNode)?;

    let native = widget.native().ok_or(SnapshotError::NoRenderer)?;
    let renderer = native
        .renderer()
        .or_else(|| {
            native
                .surface()
                .and_then(|surface| gtk::gsk::Renderer::for_surface(&surface))
        })
        .ok_or(SnapshotError::NoRenderer)?;

    let viewport = gtk::graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
    let texture = renderer.render_texture(&node, Some(&viewport));
    if let Some(parent) = path.as_ref().parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|error| SnapshotError::Save(error.to_string()))?;
        }
    }
    texture
        .save_to_png(path)
        .map_err(|error| SnapshotError::Save(error.to_string()))
}

/// Capture the first mapped/rendered frame of `window`, then quit `app`.
///
/// A failed write prints to stderr and exits with status 1 so the harness
/// script does not treat a missing PNG as success.
pub fn capture_after_first_frame(
    window: &impl IsA<gtk::Widget>,
    path: impl AsRef<Path>,
    app: &impl IsA<gtk::Application>,
) {
    let widget = window.as_ref().clone();
    let path = path.as_ref().to_path_buf();
    let app = app.as_ref().clone();
    let finished = Rc::new(Cell::new(false));

    let timeout = {
        let finished = finished.clone();
        let app = app.clone();
        move || {
            if finished.get() {
                return;
            }
            eprintln!("snapshot failed: timed out waiting for a frame");
            app.quit();
            std::process::exit(1);
        }
    };
    gtk::glib::timeout_add_local_once(Duration::from_secs(8), timeout);

    let start = {
        let widget = widget.clone();
        let finished = finished.clone();
        move || wait_after_paint(widget, path, app, finished)
    };

    if widget.is_mapped() {
        gtk::glib::idle_add_local_once(start);
        return;
    }

    let start = Rc::new(RefCell::new(Some(start)));
    widget.connect_map(move |_| {
        if let Some(start) = start.borrow_mut().take() {
            gtk::glib::idle_add_local_once(start);
        }
    });
}

fn wait_after_paint(
    widget: gtk::Widget,
    path: PathBuf,
    app: gtk::Application,
    finished: Rc<Cell<bool>>,
) {
    let capture = {
        let widget = widget.clone();
        let app = app.clone();
        let finished = finished.clone();
        move || finish_capture(&widget, &path, &app, &finished)
    };

    let Some(clock) = widget.frame_clock() else {
        gtk::glib::idle_add_local_once(capture);
        return;
    };

    let handler: Rc<RefCell<Option<gtk::glib::SignalHandlerId>>> = Rc::new(RefCell::new(None));
    let once = Rc::new(Cell::new(false));
    let id = clock.connect_after_paint({
        let handler = handler.clone();
        let clock = clock.clone();
        move |_| {
            if once.replace(true) {
                return;
            }
            if let Some(id) = handler.borrow_mut().take() {
                clock.disconnect(id);
            }
            capture();
        }
    });
    *handler.borrow_mut() = Some(id);
    widget.queue_draw();
    clock.request_phase(gtk::gdk::FrameClockPhase::PAINT);
}

fn finish_capture(
    widget: &gtk::Widget,
    path: &Path,
    app: &gtk::Application,
    finished: &Cell<bool>,
) {
    if finished.replace(true) {
        return;
    }
    match capture_widget(widget, path) {
        Ok(()) => app.quit(),
        Err(error) => {
            eprintln!("snapshot failed: {error}");
            app.quit();
            std::process::exit(1);
        }
    }
}

fn first_positive(widget: i32, intrinsic: i32) -> Option<i32> {
    if widget > 0 {
        Some(widget)
    } else if intrinsic > 0 {
        Some(intrinsic)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::SnapshotError;

    #[test]
    fn error_messages_name_the_failure() {
        assert_eq!(
            SnapshotError::NoSize.to_string(),
            "window has no mapped size"
        );
        assert_eq!(
            SnapshotError::NoRenderer.to_string(),
            "no GSK renderer for the native surface"
        );
        assert_eq!(
            SnapshotError::EmptyNode.to_string(),
            "widget paintable produced an empty render node"
        );
        assert_eq!(
            SnapshotError::Save("disk full".into()).to_string(),
            "disk full"
        );
    }
}
