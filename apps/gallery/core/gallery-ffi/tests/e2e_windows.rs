//! Generated-library regression for the windowed UniFFI projection.
//!
//! Run by `apps/gallery/scripts/e2e_20k.sh` after the scan/enrichment/index/
//! memories pipeline. This second pass exercises the actual FFI-facing
//! scanner records and generation-checked content windows.

use std::env;
use std::time::Instant;

use gallery_ffi::{LibraryIndex, ScanRequest, ScannerSession, ScheduledMemoryContext, ViewError};

#[test]
#[ignore = "e2e: needs LOCALGALLERY_E2E_LIBRARY"]
fn generated_library_ffi_windows_are_bounded_and_generation_checked() {
    let root = env::var("LOCALGALLERY_E2E_LIBRARY")
        .expect("LOCALGALLERY_E2E_LIBRARY must point at the generated tree");
    let expected = env::var("LOCALGALLERY_E2E_COUNT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20_000);

    let scanner = ScannerSession::new();
    let scanned = scanner
        .scan(
            root,
            ScanRequest {
                reuse_cached: false,
                cached_photos: Vec::new(),
                cached_sidecar_manifest: Vec::new(),
            },
            None,
        )
        .expect("generated tree must scan");
    assert!(
        scanned.flat_photos.len() >= expected,
        "generated scan returned {} rows, expected at least {expected}",
        scanned.flat_photos.len()
    );

    let index = LibraryIndex::new();
    index.build(scanned.flat_photos);
    let structure = index.photo_structure();
    let id_count: usize = structure
        .sections
        .iter()
        .map(|section| section.item_ids.len())
        .sum();
    assert!(id_count >= expected);

    let mut crossed = 0usize;
    let mut largest = 0usize;
    for section in &structure.sections {
        for offset in (0..section.item_ids.len()).step_by(128) {
            let rows = index
                .photo_window(section.id.clone(), offset as u64, 128, structure.generation)
                .expect("current generation");
            largest = largest.max(rows.len());
            crossed += rows.len();
        }
    }
    assert_eq!(crossed, id_count);
    assert_eq!(
        largest, 128,
        "no FFI content allocation may exceed its window"
    );

    let filtered = index.set_photo_view("anna".into(), Vec::new());
    assert!(filtered.generation > structure.generation);
    assert!(matches!(
        index.photo_window(
            structure.sections[0].id.clone(),
            0,
            128,
            structure.generation
        ),
        Err(ViewError::StaleGeneration { .. })
    ));
    let filtered_rows = if let Some(section) = filtered.sections.first() {
        index
            .photo_window(section.id.clone(), 0, 128, filtered.generation)
            .expect("fresh filtered generation")
    } else {
        Vec::new()
    };
    assert!(filtered_rows.len() <= 128);

    let horizon_ceiling_ms = env::var("LOCALGALLERY_E2E_HORIZON_MAX_MS")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(1_000.0);
    let started = Instant::now();
    let scheduled = index.compute_scheduled(
        ScheduledMemoryContext {
            leaf_folders: Vec::new(),
            contacts: Vec::new(),
            person_contact_links: Vec::new(),
            birthdays_enabled: true,
            me_person_path: String::new(),
            hidden_people: Vec::new(),
            now: 802_094_400.0, // 2026-06-01T00:00:00Z
            time_zone_offset_seconds: 0,
            horizon_offset_seconds: Vec::new(),
            seed: "e2e-20k".into(),
            seen_memory_ids: Vec::new(),
            surfaced_clusters: Vec::new(),
        },
        7,
        Vec::new(),
    );
    let horizon_ms = started.elapsed().as_secs_f64() * 1_000.0;
    println!(
        "[gallery-e2e-20k] metric_retained_horizon_ms={horizon_ms:.1} items={} ceiling={horizon_ceiling_ms:.0}",
        scheduled.len()
    );
    assert!(
        horizon_ms < horizon_ceiling_ms,
        "retained 20k horizon took {horizon_ms:.1}ms (ceiling {horizon_ceiling_ms:.0}ms)"
    );
}
