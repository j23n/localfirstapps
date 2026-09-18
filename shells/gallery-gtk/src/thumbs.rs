//! GTK bind layer over leftover **non-`ui`** decode / XDG APIs.
//!
//! Do not import leftover `src/ui`. Thumbnails resolve `thumbnail_ref` in
//! factory `bind` and cancel only on factory `unbind` (the cell was recycled).
//! Layout `unmap` pauses waiters without cancelling the pool job — GridView
//! remaps tiles while filling the model. Viewer jobs still resume on `map`.
//! Pool completions wake a LOW-priority idle drain so scroll/input at
//! DEFAULT keep the frame. Each idle uploads a few textures, then yields.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;

use localgallery::decode::RgbFrame;
use localgallery::display::{grid_thumb_size, viewer_long_side};
use localgallery::thumbs::{
    complete_action, frame_cost, job_key, result_channel, CompleteAction, DecodePool, FlightBook,
    PoolJob, PoolKind, PoolResult, ReadyCache, DEFAULT_WORKERS,
};
use localgallery::xdg_thumb::ThumbSize;

const VIEWPORT_HOOK_CLASS: &str = "gallery-thumb-hooks";

thread_local! {
    static DRAIN_TARGET: RefCell<Option<Weak<ThumbInner>>> = const { RefCell::new(None) };
}

static DRAIN_PENDING: AtomicBool = AtomicBool::new(false);

/// GPU uploads + `set_paintable` stay off the scroll frame. Two matches
/// the decode pool; more than that steals the next input cycle.
const DRAIN_TEXTURES_PER_IDLE: usize = 2;

fn attach_drain_target(inner: &Rc<ThumbInner>) {
    DRAIN_TARGET.with(|slot| *slot.borrow_mut() = Some(Rc::downgrade(inner)));
}

/// Wake the GTK thread after a pool result. Coalesces bursts into one idle.
/// `LOW` runs after GTK has handled the scroll/frame clock.
fn wake_drain() {
    if DRAIN_PENDING.swap(true, Ordering::AcqRel) {
        return;
    }
    glib::idle_add_full(glib::Priority::LOW, || {
        DRAIN_PENDING.store(false, Ordering::Release);
        DRAIN_TARGET.with(|slot| {
            if let Some(inner) = slot.borrow().as_ref().and_then(Weak::upgrade) {
                ThumbCache { inner }.drain();
            }
        });
        glib::ControlFlow::Break
    });
}

/// Shared decode queue. Poll [`Self::drain`] on the GTK thread.
#[derive(Clone)]
pub struct ThumbCache {
    inner: Rc<ThumbInner>,
}

struct ThumbInner {
    pool: DecodePool,
    rx: RefCell<Receiver<PoolResult>>,
    flight: RefCell<FlightBook>,
    waiting: RefCell<HashMap<String, Vec<ThumbSink>>>,
    ready: RefCell<ReadyCache<gdk::Texture>>,
    requests: RefCell<Vec<StoredBinding>>,
}

impl ThumbCache {
    #[must_use]
    pub fn new() -> Self {
        Self::with_workers(DEFAULT_WORKERS)
    }

    fn with_workers(workers: usize) -> Self {
        let (tx, rx) = result_channel();
        Self::from_pool(
            DecodePool::spawn_notified(workers, tx, wake_drain),
            rx,
            workers,
        )
    }

    fn from_pool(pool: DecodePool, rx: Receiver<PoolResult>, workers: usize) -> Self {
        let cache = Self {
            inner: Rc::new(ThumbInner {
                pool,
                rx: RefCell::new(rx),
                flight: RefCell::new(FlightBook::new(workers)),
                waiting: RefCell::new(HashMap::new()),
                ready: RefCell::new(ReadyCache::product()),
                requests: RefCell::new(Vec::new()),
            }),
        };
        attach_drain_target(&cache.inner);
        cache
    }

    #[cfg(test)]
    fn with_work<F>(workers: usize, work: F) -> Self
    where
        F: Fn(&PoolJob) -> Option<RgbFrame> + Send + Sync + 'static,
    {
        let (tx, rx) = result_channel();
        Self::from_pool(
            DecodePool::spawn_work_notified(workers, tx, work, wake_drain),
            rx,
            workers,
        )
    }

    /// Call from the GTK thread. Uploads at most
    /// [`DRAIN_TEXTURES_PER_IDLE`] textures, then reschedules so a fling
    /// can take the next cycle.
    pub fn drain(&self) -> bool {
        let t0 = std::time::Instant::now();
        let mut progressed = false;
        let mut delivered = 0u32;
        let mut missed = 0u32;
        let mut uploaded = 0usize;
        while uploaded < DRAIN_TEXTURES_PER_IDLE {
            let Ok(msg) = self.inner.rx.borrow().try_recv() else {
                break;
            };
            progressed = true;
            self.inner.flight.borrow_mut().finish(&msg.key);
            let waiters = self
                .inner
                .waiting
                .borrow_mut()
                .remove(&msg.key)
                .unwrap_or_default();
            let Some(frame) = msg.frame else {
                missed += 1;
                localcore_trace::event("thumbs", format!("drain miss key={}", msg.key));
                continue;
            };
            let Some(tex) = texture_from_rgb(&frame) else {
                missed += 1;
                localcore_trace::event(
                    "thumbs",
                    format!(
                        "texture fail key={} {}x{}",
                        msg.key, frame.width, frame.height
                    ),
                );
                continue;
            };
            uploaded += 1;
            self.inner
                .ready
                .borrow_mut()
                .insert(msg.key.clone(), tex.clone(), frame_cost(&frame));
            if complete_action(waiters.len()) == CompleteAction::Deliver {
                for sink in waiters {
                    sink.deliver(&tex);
                }
                delivered += 1;
            }
        }
        if uploaded >= DRAIN_TEXTURES_PER_IDLE {
            wake_drain();
        }
        if progressed && localcore_trace::enabled() {
            localcore_trace::event(
                "thumbs",
                format!(
                    "drain delivered={delivered} missed={missed} {}",
                    localcore_trace::fmt_ms(t0.elapsed())
                ),
            );
        }
        progressed
    }

    #[must_use]
    pub fn debug_snapshot(&self) -> ThumbSnapshot {
        ThumbSnapshot {
            ready: self.inner.ready.borrow().len(),
            inflight: self.inner.flight.borrow().inflight_len(),
            waiting: self.inner.waiting.borrow().len(),
        }
    }

    /// Gallery tile: XDG `large` (1×) or `x-large` (2×).
    pub fn bind_grid(&self, picture: &gtk::Picture, thumbnail_ref: &str, id: &str, scale: u32) {
        let size = grid_thumb_size(scale);
        self.enqueue(
            ThumbSink::Picture(picture.clone()),
            thumbnail_ref,
            id,
            PoolKind::Grid { size },
        );
    }

    /// Kit `media_item` prefix `gtk::Image` named `thumb`.
    #[allow(dead_code)]
    pub fn bind_prefix(&self, image: &gtk::Image, thumbnail_ref: &str, id: &str, scale: u32) {
        let size = grid_thumb_size(scale);
        self.enqueue(
            ThumbSink::Image(image.clone()),
            thumbnail_ref,
            id,
            PoolKind::Grid { size },
        );
    }

    /// Viewer: original file, long side capped by [`viewer_long_side`].
    ///
    /// Paints a grid / XDG thumb immediately so opening a photo is not gated
    /// on the full-size JPEG.
    pub fn bind_viewer(&self, picture: &gtk::Picture, path: &str, id: &str, max_side: u32) {
        let _ = self.drain();
        if let Some(tex) = self.peek_placeholder(id, path) {
            picture.set_paintable(Some(&tex));
        }
        self.enqueue(
            ThumbSink::Picture(picture.clone()),
            path,
            id,
            PoolKind::Viewer { max_side },
        );
    }

    fn peek_placeholder(&self, id: &str, path: &str) -> Option<gdk::Texture> {
        for size in [
            ThumbSize::XXLarge,
            ThumbSize::XLarge,
            ThumbSize::Large,
            ThumbSize::Normal,
        ] {
            let key = job_key(id, &PoolKind::Grid { size });
            if let Some(tex) = self.inner.ready.borrow_mut().get_cloned(&key) {
                return Some(tex);
            }
        }
        let png =
            localgallery::xdg_thumb::lookup_path(&localgallery::xdg_thumb::cache_root(), path)?;
        gdk::Texture::from_filename(&png).ok()
    }

    /// Drop waiters and blank the picture. Tests and explicit teardown;
    /// factory recycle during scroll must not use this.
    #[allow(dead_code)]
    pub fn unbind(&self, picture: &gtk::Picture) {
        self.recycle(picture);
        picture.set_paintable(Option::<&gdk::Paintable>::None);
    }

    /// Factory recycle during scroll: detach waiters, keep the last
    /// paintable. Clearing here blanks the tile and queues a draw on the
    /// input frame. The next `bind` paints the new id when it is ready.
    pub fn recycle(&self, picture: &gtk::Picture) {
        let sink = ThumbSink::Picture(picture.clone());
        self.detach(&sink);
        self.forget_request(&sink);
    }

    /// Drop waiters for a kit prefix image.
    #[allow(dead_code)]
    pub fn unbind_prefix(&self, image: &gtk::Image) {
        let sink = ThumbSink::Image(image.clone());
        self.detach(&sink);
        self.forget_request(&sink);
        image.set_paintable(Option::<&gdk::Paintable>::None);
    }

    pub fn viewer_long_side(&self, logical_width: u32, logical_height: u32, scale: u32) -> u32 {
        viewer_long_side(logical_width, logical_height, scale)
    }
}

impl Default for ThumbCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ThumbCache {
    fn enqueue(&self, sink: ThumbSink, path: &str, id: &str, kind: PoolKind) {
        if matches!(kind, PoolKind::Viewer { .. }) {
            self.ensure_viewport_hooks(&sink);
        }
        self.store_request(&sink, path, id, &kind);
        let key = job_key(id, &kind);
        if self.is_waiting(&sink, &key) {
            return;
        }
        self.detach(&sink);
        // Grid bind runs inside GridView's scroll/layout. Do not drain
        // (texture upload) or clear (blank + draw) on that stack.
        if matches!(kind, PoolKind::Viewer { .. }) {
            let _ = self.drain();
        }
        if let Some(tex) = self.inner.ready.borrow_mut().get_cloned(&key) {
            localcore_trace::detail("thumbs", format!("cache hit {key}"));
            sink.deliver(&tex);
            return;
        }
        self.inner
            .waiting
            .borrow_mut()
            .entry(key.clone())
            .or_default()
            .push(sink);
        if !self.inner.flight.borrow_mut().try_start(&key) {
            localcore_trace::detail("thumbs", format!("join in-flight {key}"));
            return;
        }
        let kind_label = match &kind {
            PoolKind::Grid { size } => format!("grid:{}", size.dir_name()),
            PoolKind::Viewer { max_side } => format!("viewer:{max_side}"),
            PoolKind::Face { size, .. } => format!("face:{}", size.dir_name()),
        };
        if matches!(kind, PoolKind::Viewer { .. }) {
            localcore_trace::event("thumbs", format!("submit {kind_label} path={path}"));
        } else {
            localcore_trace::detail("thumbs", format!("submit {kind_label} path={path}"));
        }
        self.inner.pool.submit(PoolJob {
            key,
            path: path.to_string(),
            kind,
        });
    }

    fn detach(&self, sink: &ThumbSink) {
        self.drop_waiters(sink, true);
    }

    fn pause(&self, sink: &ThumbSink) {
        self.drop_waiters(sink, false);
    }

    fn drop_waiters(&self, sink: &ThumbSink, cancel: bool) {
        let mut emptied = Vec::new();
        self.inner.waiting.borrow_mut().retain(|key, waiters| {
            waiters.retain(|waiting| waiting != sink);
            if waiters.is_empty() {
                emptied.push(key.clone());
                false
            } else {
                true
            }
        });
        if !cancel {
            return;
        }
        for key in emptied {
            if self.inner.pool.cancel(&key) {
                localcore_trace::detail("thumbs", format!("cancel queued {key}"));
                self.inner.flight.borrow_mut().finish(&key);
            }
        }
    }

    fn store_request(&self, sink: &ThumbSink, path: &str, id: &str, kind: &PoolKind) {
        let request = StoredRequest {
            path: path.to_string(),
            id: id.to_string(),
            kind: kind.clone(),
        };
        let mut requests = self.inner.requests.borrow_mut();
        requests.retain(|binding| binding.sink.is_alive() && !binding.sink.matches(sink));
        requests.push(StoredBinding {
            sink: WeakSink::from_sink(sink),
            request,
        });
    }

    fn forget_request(&self, sink: &ThumbSink) {
        self.inner
            .requests
            .borrow_mut()
            .retain(|binding| !binding.sink.matches(sink));
    }

    fn request_for(&self, sink: &ThumbSink) -> Option<StoredRequest> {
        self.inner
            .requests
            .borrow()
            .iter()
            .find_map(|binding| binding.sink.matches(sink).then(|| binding.request.clone()))
    }

    fn is_waiting(&self, sink: &ThumbSink, key: &str) -> bool {
        self.inner
            .waiting
            .borrow()
            .get(key)
            .is_some_and(|waiters| waiters.iter().any(|waiting| waiting == sink))
    }

    fn on_unmap(&self, sink: &ThumbSink) {
        self.pause(sink);
    }

    fn on_map(&self, sink: &ThumbSink) {
        let Some(req) = self.request_for(sink) else {
            return;
        };
        let key = job_key(&req.id, &req.kind);
        if self.is_waiting(sink, &key) {
            return;
        }
        // Re-enqueue even when a placeholder is already painted. `bind_viewer`
        // shows a grid/XDG thumb first; skipping that case would drop the
        // full-size job after a tab hide or nav transition unmap/map.
        self.enqueue(sink.clone(), &req.path, &req.id, req.kind);
    }

    fn ensure_viewport_hooks(&self, sink: &ThumbSink) {
        let weak = Rc::downgrade(&self.inner);
        match sink {
            ThumbSink::Picture(picture) => {
                if picture.has_css_class(VIEWPORT_HOOK_CLASS) {
                    return;
                }
                picture.add_css_class(VIEWPORT_HOOK_CLASS);
                let inner = weak.clone();
                picture.connect_unmap(move |picture| {
                    let Some(inner) = inner.upgrade() else { return };
                    ThumbCache { inner }.on_unmap(&ThumbSink::Picture(picture.clone()));
                });
                picture.connect_map(move |picture| {
                    let Some(inner) = weak.upgrade() else { return };
                    ThumbCache { inner }.on_map(&ThumbSink::Picture(picture.clone()));
                });
            }
            ThumbSink::Image(image) => {
                if image.has_css_class(VIEWPORT_HOOK_CLASS) {
                    return;
                }
                image.add_css_class(VIEWPORT_HOOK_CLASS);
                let inner = weak.clone();
                image.connect_unmap(move |image| {
                    let Some(inner) = inner.upgrade() else { return };
                    ThumbCache { inner }.on_unmap(&ThumbSink::Image(image.clone()));
                });
                image.connect_map(move |image| {
                    let Some(inner) = weak.upgrade() else { return };
                    ThumbCache { inner }.on_map(&ThumbSink::Image(image.clone()));
                });
            }
        }
    }
}

#[derive(Clone)]
enum ThumbSink {
    Picture(gtk::Picture),
    #[allow(dead_code)]
    Image(gtk::Image),
}

impl PartialEq for ThumbSink {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Picture(left), Self::Picture(right)) => left == right,
            (Self::Image(left), Self::Image(right)) => left == right,
            _ => false,
        }
    }
}

impl ThumbSink {
    fn deliver(&self, tex: &gdk::Texture) {
        match self {
            Self::Picture(picture) => {
                picture.set_paintable(Some(tex));
                picture.queue_draw();
            }
            Self::Image(image) => {
                image.set_paintable(Some(tex));
                image.queue_draw();
            }
        }
    }

}

#[derive(Clone)]
struct StoredRequest {
    path: String,
    id: String,
    kind: PoolKind,
}

struct StoredBinding {
    sink: WeakSink,
    request: StoredRequest,
}

enum WeakSink {
    Picture(glib::object::WeakRef<gtk::Picture>),
    Image(glib::object::WeakRef<gtk::Image>),
}

impl WeakSink {
    fn from_sink(sink: &ThumbSink) -> Self {
        match sink {
            ThumbSink::Picture(picture) => Self::Picture(picture.downgrade()),
            ThumbSink::Image(image) => Self::Image(image.downgrade()),
        }
    }

    fn matches(&self, sink: &ThumbSink) -> bool {
        match (self, sink) {
            (Self::Picture(weak), ThumbSink::Picture(picture)) => {
                weak.upgrade().as_ref() == Some(picture)
            }
            (Self::Image(weak), ThumbSink::Image(image)) => weak.upgrade().as_ref() == Some(image),
            _ => false,
        }
    }

    fn is_alive(&self) -> bool {
        match self {
            Self::Picture(weak) => weak.upgrade().is_some(),
            Self::Image(weak) => weak.upgrade().is_some(),
        }
    }
}

/// Decode-pool counters for the idle heartbeat.
#[derive(Debug, Clone, Copy)]
pub struct ThumbSnapshot {
    pub ready: usize,
    pub inflight: usize,
    pub waiting: usize,
}

fn texture_from_rgb(frame: &RgbFrame) -> Option<gdk::Texture> {
    let expect = frame.width as usize * frame.height as usize * 3;
    if frame.rgb.len() != expect || frame.width == 0 || frame.height == 0 {
        return None;
    }
    let bytes = glib::Bytes::from(&frame.rgb);
    Some(
        gdk::MemoryTexture::new(
            frame.width as i32,
            frame.height as i32,
            gdk::MemoryFormat::R8g8b8,
            &bytes,
            frame.width as usize * 3,
        )
        .upcast(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pump_main_until(deadline: std::time::Instant, mut ready: impl FnMut() -> bool) {
        let ctx = glib::MainContext::default();
        while !ready() {
            assert!(
                std::time::Instant::now() < deadline,
                "GTK idle did not drain the completed thumb"
            );
            while ctx.iteration(false) {}
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[test]
    fn job_keys_are_host_keys() {
        let grid = job_key(
            "abc",
            &PoolKind::Grid {
                size: localgallery::xdg_thumb::ThumbSize::Large,
            },
        );
        assert!(grid.contains("abc"));
        assert!(grid.contains("large"));
        let view = job_key("abc", &PoolKind::Viewer { max_side: 800 });
        assert!(view.contains("800"));
    }

    fn ensure_gtk() -> bool {
        gtk::is_initialized() || gtk::init().is_ok()
    }

    /// GTK widgets must be created on the thread that called `gtk::init`.
    /// The harness uses a new thread per test, so keep these cases together.
    #[test]
    fn unbind_drops_waiters() {
        if !ensure_gtk() {
            return;
        }
        assert_unbind_clears_waiters_and_paintable();
        assert_last_waiter_unbind_cancels_queued_job_and_clears_flight();
        assert_last_waiter_inflight_leaves_flight();
        assert_unmap_keeps_request_and_map_reenqueues_queued_job();
        assert_unbind_clears_request_so_map_does_not_requeue();
        assert_map_reenqueues_viewer_when_placeholder_is_set();
        assert_rebind_same_photo_does_not_resubmit();
        assert_pool_result_paints_on_idle_without_poll();
        assert_recycle_keeps_paintable();
    }

    fn assert_pool_result_paints_on_idle_without_poll() {
        let cache = ThumbCache::with_work(1, |_| {
            Some(RgbFrame {
                width: 1,
                height: 1,
                rgb: vec![1, 2, 3],
            })
        });
        let picture = gtk::Picture::new();
        cache.bind_grid(&picture, "/tmp/idle.jpg", "idle", 1);
        pump_main_until(
            std::time::Instant::now() + std::time::Duration::from_secs(2),
            || picture.paintable().is_some(),
        );
    }

    fn assert_recycle_keeps_paintable() {
        let cache = ThumbCache::new();
        let picture = gtk::Picture::new();
        let tex = texture_from_rgb(&RgbFrame {
            width: 1,
            height: 1,
            rgb: vec![0, 0, 0],
        })
        .expect("tex");
        picture.set_paintable(Some(&tex));
        cache
            .inner
            .waiting
            .borrow_mut()
            .insert("k".into(), vec![ThumbSink::Picture(picture.clone())]);
        cache.recycle(&picture);
        assert!(cache
            .inner
            .waiting
            .borrow()
            .get("k")
            .is_none_or(|waiters| waiters.is_empty()));
        assert!(picture.paintable().is_some());
    }

    fn assert_unbind_clears_waiters_and_paintable() {
        let cache = ThumbCache::new();
        let picture = gtk::Picture::new();
        cache
            .inner
            .waiting
            .borrow_mut()
            .insert("k".into(), vec![ThumbSink::Picture(picture.clone())]);
        cache.unbind(&picture);
        assert!(cache
            .inner
            .waiting
            .borrow()
            .get("k")
            .is_none_or(|waiters| waiters.is_empty()));
        assert!(picture.paintable().is_none());
    }

    fn grid_key(id: &str) -> String {
        job_key(
            id,
            &PoolKind::Grid {
                size: grid_thumb_size(1),
            },
        )
    }

    fn hold_pair() -> std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)> {
        std::sync::Arc::new((std::sync::Mutex::new(true), std::sync::Condvar::new()))
    }

    fn wait_held(hold: &std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>) {
        let (lock, cv) = hold.as_ref();
        let mut guard = lock.lock().unwrap();
        while *guard {
            guard = cv.wait(guard).unwrap_or_else(|p| p.into_inner());
        }
    }

    fn release_held(hold: &std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>) {
        let (lock, cv) = hold.as_ref();
        *lock.lock().unwrap() = false;
        cv.notify_all();
    }

    fn release_and_drain(
        cache: &ThumbCache,
        hold: &std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    ) {
        release_held(hold);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !cache.drain() {
            assert!(
                std::time::Instant::now() < deadline,
                "pool did not finish after release"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    fn blocking_cache(
        started: std::sync::Arc<std::sync::Mutex<usize>>,
        hold: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    ) -> ThumbCache {
        ThumbCache::with_work(1, move |_job| {
            *started.lock().unwrap() += 1;
            wait_held(&hold);
            None
        })
    }

    fn wait_started(started: &std::sync::Mutex<usize>, n: usize) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if *started.lock().unwrap() >= n {
                return;
            }
            assert!(std::time::Instant::now() < deadline, "worker did not start");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    fn assert_last_waiter_unbind_cancels_queued_job_and_clears_flight() {
        let started = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let hold = hold_pair();
        let cache = blocking_cache(started.clone(), hold.clone());
        let first = gtk::Picture::new();
        let second = gtk::Picture::new();
        cache.bind_grid(&first, "/tmp/a.jpg", "a", 1);
        wait_started(&started, 1);
        cache.bind_grid(&second, "/tmp/b.jpg", "b", 1);
        let key_b = grid_key("b");
        assert!(cache.inner.flight.borrow().is_inflight(&key_b));
        assert!(cache.inner.waiting.borrow().contains_key(&key_b));

        cache.unbind(&second);
        assert!(!cache.inner.flight.borrow().is_inflight(&key_b));
        assert!(!cache.inner.waiting.borrow().contains_key(&key_b));
        assert!(cache
            .request_for(&ThumbSink::Picture(second.clone()))
            .is_none());
        assert!(second.paintable().is_none());

        cache.bind_grid(&second, "/tmp/b.jpg", "b", 1);
        assert!(cache.inner.flight.borrow().is_inflight(&key_b));
        assert!(cache.inner.waiting.borrow().contains_key(&key_b));
        cache.unbind(&first);
        cache.unbind(&second);
        release_and_drain(&cache, &hold);
    }

    fn assert_last_waiter_inflight_leaves_flight() {
        let started = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let hold = hold_pair();
        let cache = blocking_cache(started.clone(), hold.clone());
        let picture = gtk::Picture::new();
        cache.bind_grid(&picture, "/tmp/a.jpg", "a", 1);
        wait_started(&started, 1);
        let key = grid_key("a");
        assert!(cache.inner.flight.borrow().is_inflight(&key));

        cache.unbind(&picture);
        assert!(cache.inner.flight.borrow().is_inflight(&key));
        assert!(!cache.inner.waiting.borrow().contains_key(&key));
        assert!(picture.paintable().is_none());
        release_and_drain(&cache, &hold);
    }

    fn assert_unmap_keeps_request_and_map_reenqueues_queued_job() {
        let started = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let hold = hold_pair();
        let cache = blocking_cache(started.clone(), hold.clone());
        let first = gtk::Picture::new();
        let second = gtk::Picture::new();
        cache.bind_grid(&first, "/tmp/a.jpg", "a", 1);
        wait_started(&started, 1);
        cache.bind_grid(&second, "/tmp/b.jpg", "b", 1);
        let key_b = grid_key("b");
        let sink = ThumbSink::Picture(second.clone());

        cache.on_unmap(&sink);
        assert!(cache.request_for(&sink).is_some());
        assert!(
            cache.inner.flight.borrow().is_inflight(&key_b),
            "unmap must not cancel a still-bound tile"
        );
        assert!(!cache.inner.waiting.borrow().contains_key(&key_b));
        assert!(second.paintable().is_none());

        cache.on_map(&sink);
        assert!(cache.inner.flight.borrow().is_inflight(&key_b));
        assert!(cache.inner.waiting.borrow().contains_key(&key_b));
        cache.unbind(&first);
        cache.unbind(&second);
        release_and_drain(&cache, &hold);
    }

    fn assert_map_reenqueues_viewer_when_placeholder_is_set() {
        let started = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let hold = hold_pair();
        let cache = blocking_cache(started.clone(), hold.clone());
        let grid = gtk::Picture::new();
        let viewer = gtk::Picture::new();
        cache.bind_grid(&grid, "/tmp/a.jpg", "a", 1);
        wait_started(&started, 1);

        let placeholder = texture_from_rgb(&RgbFrame {
            width: 1,
            height: 1,
            rgb: vec![0, 0, 0],
        })
        .expect("placeholder");
        cache
            .inner
            .ready
            .borrow_mut()
            .insert(grid_key("v"), placeholder, 3);
        cache.bind_viewer(&viewer, "/tmp/v.jpg", "v", 800);
        assert!(viewer.paintable().is_some());
        let view_key = job_key("v", &PoolKind::Viewer { max_side: 800 });
        let sink = ThumbSink::Picture(viewer.clone());
        assert!(cache.inner.waiting.borrow().contains_key(&view_key));

        cache.on_unmap(&sink);
        assert!(cache.request_for(&sink).is_some());
        assert!(viewer.paintable().is_some());
        assert!(!cache.inner.waiting.borrow().contains_key(&view_key));
        assert!(
            cache.inner.flight.borrow().is_inflight(&view_key),
            "unmap must not cancel the viewer decode"
        );

        cache.on_map(&sink);
        assert!(cache.inner.flight.borrow().is_inflight(&view_key));
        assert!(cache.inner.waiting.borrow().contains_key(&view_key));
        cache.unbind(&grid);
        cache.unbind(&viewer);
        release_and_drain(&cache, &hold);
    }

    fn assert_rebind_same_photo_does_not_resubmit() {
        let started = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let hold = hold_pair();
        let cache = blocking_cache(started.clone(), hold.clone());
        let first = gtk::Picture::new();
        let second = gtk::Picture::new();
        cache.bind_grid(&first, "/tmp/a.jpg", "a", 1);
        wait_started(&started, 1);
        cache.bind_grid(&second, "/tmp/b.jpg", "b", 1);
        let key_b = grid_key("b");
        assert!(cache.inner.flight.borrow().is_inflight(&key_b));
        cache.bind_grid(&second, "/tmp/b.jpg", "b", 1);
        assert!(cache.inner.flight.borrow().is_inflight(&key_b));
        assert_eq!(
            cache
                .inner
                .waiting
                .borrow()
                .get(&key_b)
                .map(Vec::len)
                .unwrap_or(0),
            1,
            "rebind of the same tile must not cancel and resubmit"
        );
        cache.unbind(&first);
        cache.unbind(&second);
        release_and_drain(&cache, &hold);
    }

    fn assert_unbind_clears_request_so_map_does_not_requeue() {
        let started = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let hold = hold_pair();
        let cache = blocking_cache(started.clone(), hold.clone());
        let first = gtk::Picture::new();
        let second = gtk::Picture::new();
        cache.bind_grid(&first, "/tmp/a.jpg", "a", 1);
        wait_started(&started, 1);
        cache.bind_grid(&second, "/tmp/b.jpg", "b", 1);
        let key_b = grid_key("b");
        cache.unbind(&second);
        cache.on_map(&ThumbSink::Picture(second.clone()));
        assert!(!cache.inner.flight.borrow().is_inflight(&key_b));
        assert!(!cache.inner.waiting.borrow().contains_key(&key_b));
        cache.unbind(&first);
        release_and_drain(&cache, &hold);
    }
}
