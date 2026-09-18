//! Display-free Gallery shell catalog timings over `LOCALGALLERY_E2E_LIBRARY`.
//!
//! `#[ignore]`d: not a `cargo test` / CI gate. Needs the generated 20k tree.
//! Run via `scripts/gtk-perf.sh 20k`. This is the Session + `ViewList`
//! path (scan, apply, flatten, first window) — no leftover `open_library`
//! and no `gio::ListStore`. GTK-thread leftover + refill are `--bench`.

use std::env;
use std::path::Path;
use std::time::Instant;

use gallery_gtk::{years_from_structure, Session, ViewList};

const DEFAULT_COUNT: usize = 20_000;

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn scaled(for_20k_ms: f64, count: usize, floor_ms: f64) -> f64 {
    (for_20k_ms * count as f64 / DEFAULT_COUNT as f64).max(floor_ms)
}

#[test]
#[ignore = "e2e: needs LOCALGALLERY_E2E_LIBRARY; not a cargo-test gate"]
fn generated_library_shell_catalog_is_bounded() {
    let root = env::var("LOCALGALLERY_E2E_LIBRARY").unwrap_or_else(|_| {
        panic!(
            "LOCALGALLERY_E2E_LIBRARY is unset. Generate a tree first:\n  \
             apps/gallery/scripts/e2e_20k.sh"
        )
    });
    assert!(
        Path::new(&root).is_dir(),
        "LOCALGALLERY_E2E_LIBRARY is not a directory: {root}"
    );
    let count = env_usize("LOCALGALLERY_E2E_COUNT", DEFAULT_COUNT);

    let mut session = Session::new(32);
    let scan_started = Instant::now();
    let catalog = session.scan_catalog(Path::new(&root)).expect("scan");
    let scan_ms = scan_started.elapsed().as_secs_f64() * 1000.0;
    assert!(
        catalog.flat_photos.len() >= count,
        "scan returned {} rows, expected at least {count}",
        catalog.flat_photos.len()
    );

    session
        .apply_catalog(catalog, Some(Path::new(&root)))
        .expect("apply");
    let apply_ms = session.last_apply_ms();
    assert_eq!(
        session.last_leftover_ms(),
        0.0,
        "this test keeps persist_host off"
    );
    assert!(session.photo_count() >= count);

    let structure = session.apply_photos_tab();
    let flatten_started = Instant::now();
    let list = ViewList::photos_flat(&structure);
    let flatten_ms = flatten_started.elapsed().as_secs_f64() * 1000.0;
    assert!(
        list.n_items() as usize >= count,
        "photos_flat {} ids, expected at least {count}",
        list.n_items()
    );
    let years = years_from_structure(&structure);
    assert!(
        years.len() > 1,
        "generated tree spans more than one year, got {}",
        years.len()
    );

    let folders_started = Instant::now();
    let folders = session.folder_listing(None).expect("folder listing");
    let folders_ms = folders_started.elapsed().as_secs_f64() * 1000.0;
    assert!(
        folders.rows.len() <= 256,
        "folder_listing must stay inside the 256-row window"
    );

    let window_started = Instant::now();
    let page = session
        .photo_window("photos", 0, 256)
        .expect("photo window");
    let window_ms = window_started.elapsed().as_secs_f64() * 1000.0;
    assert!(page.len() <= 256);
    if session.photo_count() >= 256 {
        assert_eq!(page.len(), 256);
    }

    let scan_ceiling = scaled(60_000.0, count, 5_000.0);
    let apply_ceiling = scaled(10_000.0, count, 1_000.0);
    let flatten_ceiling = scaled(2_000.0, count, 250.0);
    let folders_ceiling = scaled(200.0, count, 50.0);
    let window_ceiling = scaled(100.0, count, 25.0);

    println!(
        "[gallery-gtk-catalog] metric_photos={}",
        session.photo_count()
    );
    println!("[gallery-gtk-catalog] metric_scan_ms={scan_ms:.1} ceiling={scan_ceiling:.0}");
    println!("[gallery-gtk-catalog] metric_apply_ms={apply_ms:.1} ceiling={apply_ceiling:.0}");
    println!(
        "[gallery-gtk-catalog] metric_flatten_ms={flatten_ms:.1} ceiling={flatten_ceiling:.0}"
    );
    println!("[gallery-gtk-catalog] metric_folder_listing_ms={folders_ms:.1} ceiling={folders_ceiling:.0}");
    println!(
        "[gallery-gtk-catalog] metric_photo_window_ms={window_ms:.1} ceiling={window_ceiling:.0}"
    );

    assert!(scan_ms < scan_ceiling, "scan took {scan_ms:.0} ms");
    assert!(apply_ms < apply_ceiling, "apply took {apply_ms:.0} ms");
    assert!(
        flatten_ms < flatten_ceiling,
        "photos_flat took {flatten_ms:.0} ms"
    );
    assert!(
        folders_ms < folders_ceiling,
        "folder_listing took {folders_ms:.0} ms"
    );
    assert!(
        window_ms < window_ceiling,
        "photo_window took {window_ms:.0} ms"
    );
}
