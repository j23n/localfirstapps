//! Scan the tree from `apps/gallery/scripts/generate_test_library.py`.
//!
//! `#[ignore]`d: not a PR gate. Needs `LOCALGALLERY_E2E_LIBRARY` pointing at
//! a generated folder (`--count` stills, default 20_000). Run via
//! `apps/gallery/scripts/e2e_20k.sh` or the `E2E 20k` workflow
//! (`workflow_dispatch` only).
//!
//! ```sh
//! LOCALGALLERY_E2E_LIBRARY=/tmp/test-library \
//!   cargo test -p gallery-scan --release --test e2e_generated_library -- --ignored --nocapture
//! ```

use std::env;
use std::path::Path;
use std::time::Instant;

use gallery_scan::{scan, ScanInput, ScanOutcome};
use gallery_vfs::StdVfs;

fn expected_stills() -> usize {
    env::var("LOCALGALLERY_E2E_COUNT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20_000)
}

fn cache_of(outcome: &ScanOutcome) -> ScanInput {
    ScanInput {
        reuse_cached: true,
        cached_photos: outcome
            .flat_photos
            .iter()
            .map(|p| (p.path().to_string(), p.clone()))
            .collect(),
        cached_sidecar_manifest: outcome
            .sidecar_manifest
            .iter()
            .map(|r| (r.photo_id, r.clone()))
            .collect(),
    }
}

#[test]
#[ignore = "e2e: needs LOCALGALLERY_E2E_LIBRARY; not a PR gate"]
fn generated_library_scan_cold_and_light() {
    let root = env::var("LOCALGALLERY_E2E_LIBRARY").unwrap_or_else(|_| {
        panic!(
            "LOCALGALLERY_E2E_LIBRARY is unset. Generate a tree first:\n  \
             uv run apps/gallery/scripts/generate_test_library.py --out DIR --count N --today 2026-06-11"
        )
    });
    assert!(
        Path::new(&root).is_dir(),
        "LOCALGALLERY_E2E_LIBRARY is not a directory: {root}"
    );
    let want = expected_stills();

    let vfs = StdVfs::new();
    let t0 = Instant::now();
    let cold = scan(&vfs, &root, &ScanInput::default());
    let cold_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let stills = cold.flat_photos.iter().filter(|p| !p.is_video).count();
    let videos = cold.flat_photos.len() - stills;
    assert_eq!(
        stills, want,
        "stills: got {stills}, want {want} (LOCALGALLERY_E2E_COUNT / --count)"
    );
    assert!(
        cold.flat_photos
            .iter()
            .all(|p| !p.path().contains("sync-conflict")),
        "conflict copies must not be photos"
    );
    assert!(
        cold.sidecar_manifest
            .iter()
            .all(|r| !r.sidecar_url.path().contains("sync-conflict")),
        "conflict copies must not own sidecar rows"
    );
    assert!(
        cold.flat_photos.iter().any(|p| p.path().contains("Café")),
        "unicode edge-case stills must remain photos"
    );
    assert!(
        cold.failed_directory_paths.is_empty(),
        "generated tree must list cleanly: {:?}",
        cold.failed_directory_paths
    );
    assert!(!cold.sidecar_manifest.is_empty(), "most stills have .xmp");

    let t1 = Instant::now();
    let light = scan(&vfs, &root, &cache_of(&cold));
    let light_ms = t1.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(light.flat_photos.len(), cold.flat_photos.len());
    assert!(
        light.stats.cache_hits > 0,
        "light pass must reuse the cache"
    );

    let s = cold.stats;
    println!(
        "\nScan totals: {} files in {} folders, list={}ms hits={} slow={} probe={}\n\
         stills={stills} videos={videos} sidecars={}\n\
         cold={cold_ms:.1}ms light={light_ms:.1}ms (hits={})\n",
        cold.flat_photos.len(),
        s.folders,
        s.list_micros / 1000,
        s.cache_hits,
        s.slow_path,
        s.probe_micros / 1000,
        cold.sidecar_manifest.len(),
        light.stats.cache_hits,
    );

    // Catastrophe bound, not a flake-prone machine gate. Provider-backed
    // iOS was ≤ 60 s; local StdVfs on 20k JPEGs should be well under this.
    assert!(
        cold_ms < 180_000.0,
        "cold scan took {cold_ms:.0} ms (> 180 s) — walk or classify has regressed"
    );
}
