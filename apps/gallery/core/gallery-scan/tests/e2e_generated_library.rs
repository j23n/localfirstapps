//! Local performance-regression suite over `generate_test_library.py`.
//!
//! `#[ignore]`d: not a CI gate. Needs `LOCALGALLERY_E2E_LIBRARY`. Run via
//! `apps/gallery/scripts/e2e_20k.sh`.
//!
//! One ignored test walks the generated tree, enriches stills from EXIF +
//! `.xmp`, then feeds that same `PhotoFile` table into `gallery-index` and
//! `gallery-memories`. Structural counts are compared to a committed golden
//! when `--count/--seed/--today` match a file under `tests/e2e_baselines/`.
//! Timings are compared to `$LOCALGALLERY_E2E_LIBRARY/.e2e-timing.json`
//! (2× slack) so a machine records its own baseline. Rewrite goldens with
//! `LOCALGALLERY_E2E_RECORD=1`.
//!
//! ```sh
//! LOCALGALLERY_E2E_LIBRARY=/tmp/localgallery-e2e-library \
//!   cargo test -p gallery-scan --release --test e2e_generated_library -- --ignored --nocapture
//! ```

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use gallery_index::LibraryIndex;
use gallery_memories::{
    compute_scheduled, generate, GenerationInputs, LeafFolder, UtcOffset,
    SCHEDULED_MEMORY_HORIZON_DAYS,
};
use gallery_meta::read_image_metadata;
use gallery_model::date::{AppleDate, CivilDateTime};
use gallery_model::photo::{PhotoFile, PhotoFolder, SidecarStatus};
use gallery_scan::{scan, ScanInput, ScanOutcome};
use gallery_vfs::StdVfs;
use serde::{Deserialize, Serialize};

const DEFAULT_COUNT: usize = 20_000;
const DEFAULT_SEED: u64 = 42;
const DEFAULT_TODAY: &str = "2026-06-11";

/// Finding 3: 7-day horizon on a 20k library, now on the generated tree.
const HORIZON_BUDGET_MS: f64 = 100.0;
/// Same headroom as `gallery-index` `search_perf` (release is far faster).
const SEARCH_BUDGET_MS: f64 = 25.0;
/// Local timing file may grow by this factor before the run is a regression.
const TIMING_SLACK: f64 = 2.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Structure {
    version: u32,
    count: usize,
    seed: u64,
    today: String,
    stills: usize,
    videos: usize,
    sidecars: usize,
    folders: u64,
    files: usize,
    tagged_photos: usize,
    gps_photos: usize,
    dated_from_metadata: usize,
    search_anna: usize,
    search_rome: usize,
    memories_generated: usize,
    scheduled_horizon: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Timing {
    cold_ms: f64,
    light_ms: f64,
    enrich_ms: f64,
    index_build_ms: f64,
    search_anna_ms: f64,
    search_rome_ms: f64,
    memories_generate_ms: f64,
    horizon_ms: f64,
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn env_u64(name: &str, default: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn recording() -> bool {
    matches!(
        env::var("LOCALGALLERY_E2E_RECORD").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
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

fn millis(f: impl FnOnce()) -> f64 {
    let t = Instant::now();
    f();
    t.elapsed().as_secs_f64() * 1000.0
}

fn scaled(for_20k_ms: f64, count: usize, floor_ms: f64) -> f64 {
    (for_20k_ms * count as f64 / DEFAULT_COUNT as f64).max(floor_ms)
}

fn parse_today(today: &str) -> CivilDateTime {
    let parts: Vec<u32> = today
        .split('-')
        .map(|p| {
            p.parse()
                .expect("LOCALGALLERY_E2E_TODAY must be YYYY-MM-DD")
        })
        .collect();
    assert_eq!(parts.len(), 3, "LOCALGALLERY_E2E_TODAY must be YYYY-MM-DD");
    CivilDateTime::new(parts[0] as i32, parts[1], parts[2], 12, 0, 0)
}

fn apply_meta(photo: &mut PhotoFile) {
    if photo.is_video {
        return;
    }
    let meta = read_image_metadata(&StdVfs, photo.path());
    if let Some(clock) = meta.capture_wall_clock {
        photo.date_taken = Some(AppleDate::from_unix_secs_f64(
            clock.as_naive_unix_secs() as f64
        ));
        photo.date_from_metadata = true;
    }
    if !meta.hierarchical_tags.is_empty()
        || meta.country_code.is_some()
        || !meta.face_regions.is_empty()
    {
        photo.sidecar_status = SidecarStatus::Cached;
    }
    photo.hierarchical_tags = meta.hierarchical_tags;
    photo.country_code = meta.country_code;
    photo.gps_latitude = meta.gps_latitude;
    photo.gps_longitude = meta.gps_longitude;
    photo.face_regions = meta.face_regions;
    photo.enriched_file_date = photo.file_modification_date;
}

fn enrich_photos(photos: &mut [PhotoFile]) {
    let n = photos.len().max(1);
    let threads = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(4)
        .clamp(1, 8);
    let chunk = n.div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for slice in photos.chunks_mut(chunk) {
            scope.spawn(move || {
                for photo in slice {
                    apply_meta(photo);
                }
            });
        }
    });
}

fn leaf_folders(root: &PhotoFolder) -> Vec<LeafFolder> {
    let mut out = Vec::new();
    collect_leaves(root, &mut out);
    out
}

fn collect_leaves(folder: &PhotoFolder, out: &mut Vec<LeafFolder>) {
    if !folder.photos.is_empty() {
        out.push(LeafFolder {
            id: folder.id,
            name: folder.name.clone(),
            photo_ids: folder.photos.iter().map(|p| p.id).collect(),
        });
    }
    for child in &folder.subfolders {
        collect_leaves(child, out);
    }
}

fn baseline_path(count: usize, seed: u64, today: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/e2e_baselines")
        .join(format!("count-{count}-seed-{seed}-today-{today}.json"))
}

fn timing_path(root: &str) -> PathBuf {
    Path::new(root).join(".e2e-timing.json")
}

fn write_json(path: &Path, value: &impl Serialize) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, serde_json::to_string_pretty(value).unwrap() + "\n").unwrap();
}

fn assert_structure(got: &Structure, want: &Structure) {
    assert_eq!(got.version, want.version);
    assert_eq!(got.count, want.count);
    assert_eq!(got.seed, want.seed);
    assert_eq!(got.today, want.today);
    assert_eq!(got.stills, want.stills, "stills");
    assert_eq!(got.sidecars, want.sidecars, "sidecars");
    assert_eq!(got.folders, want.folders, "folders");
    assert_eq!(got.tagged_photos, want.tagged_photos, "tagged_photos");
    assert_eq!(got.gps_photos, want.gps_photos, "gps_photos");
    assert_eq!(
        got.dated_from_metadata, want.dated_from_metadata,
        "dated_from_metadata"
    );
    assert_eq!(got.search_anna, want.search_anna, "search_anna");
    assert_eq!(got.search_rome, want.search_rome, "search_rome");
    if got.videos != want.videos {
        eprintln!(
            "note: videos {} vs golden {} (ffmpeg optional) — not comparing files/memories",
            got.videos, want.videos
        );
        return;
    }
    assert_eq!(got.files, want.files, "files");
    assert_eq!(
        got.memories_generated, want.memories_generated,
        "memories_generated"
    );
    assert_eq!(
        got.scheduled_horizon, want.scheduled_horizon,
        "scheduled_horizon"
    );
}

fn assert_timing_vs_recorded(got: &Timing, recorded: &Timing) {
    let checks = [
        ("cold_ms", got.cold_ms, recorded.cold_ms),
        ("light_ms", got.light_ms, recorded.light_ms),
        ("enrich_ms", got.enrich_ms, recorded.enrich_ms),
        (
            "index_build_ms",
            got.index_build_ms,
            recorded.index_build_ms,
        ),
        (
            "search_anna_ms",
            got.search_anna_ms,
            recorded.search_anna_ms,
        ),
        (
            "search_rome_ms",
            got.search_rome_ms,
            recorded.search_rome_ms,
        ),
        (
            "memories_generate_ms",
            got.memories_generate_ms,
            recorded.memories_generate_ms,
        ),
        ("horizon_ms", got.horizon_ms, recorded.horizon_ms),
    ];
    let mut failed = Vec::new();
    for (name, g, r) in checks {
        if r <= 0.0 {
            continue;
        }
        let cap = r * TIMING_SLACK;
        if g > cap {
            failed.push(format!(
                "{name}: {g:.1} ms > {cap:.1} ms (recorded {r:.1} × {TIMING_SLACK})"
            ));
        }
    }
    assert!(
        failed.is_empty(),
        "timing regression vs {}:\n  {}",
        timing_path_hint(),
        failed.join("\n  ")
    );
}

fn timing_path_hint() -> String {
    env::var("LOCALGALLERY_E2E_LIBRARY").unwrap_or_else(|_| "<library>".into())
}

#[test]
#[ignore = "e2e: needs LOCALGALLERY_E2E_LIBRARY; local-only, not a CI gate"]
fn generated_library_regression() {
    let root = env::var("LOCALGALLERY_E2E_LIBRARY").unwrap_or_else(|_| {
        panic!(
            "LOCALGALLERY_E2E_LIBRARY is unset. Generate a tree first:\n  \
             python3 apps/gallery/scripts/generate_test_library.py --out DIR --count N --today {DEFAULT_TODAY}"
        )
    });
    assert!(
        Path::new(&root).is_dir(),
        "LOCALGALLERY_E2E_LIBRARY is not a directory: {root}"
    );
    let count = env_usize("LOCALGALLERY_E2E_COUNT", DEFAULT_COUNT);
    let seed = env_u64("LOCALGALLERY_E2E_SEED", DEFAULT_SEED);
    let today = env::var("LOCALGALLERY_E2E_TODAY").unwrap_or_else(|_| DEFAULT_TODAY.to_string());

    let vfs = StdVfs::new();
    let mut cold = None;
    let cold_ms = millis(|| cold = Some(scan(&vfs, &root, &ScanInput::default())));
    let cold = cold.unwrap();

    let stills = cold.flat_photos.iter().filter(|p| !p.is_video).count();
    let videos = cold.flat_photos.len() - stills;
    assert_eq!(
        stills, count,
        "stills: got {stills}, want {count} (LOCALGALLERY_E2E_COUNT / --count)"
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
    assert_eq!(cold.stats.cache_hits, 0, "cold scan must not hit the cache");
    assert_eq!(
        cold.stats.slow_path as usize,
        cold.flat_photos.len(),
        "cold scan rebuilds every photo"
    );
    let mut light = None;
    let light_ms = millis(|| light = Some(scan(&vfs, &root, &cache_of(&cold))));
    let light = light.unwrap();
    assert_eq!(light.flat_photos.len(), cold.flat_photos.len());
    assert_eq!(
        light.stats.cache_hits as usize,
        cold.flat_photos.len(),
        "light scan must reuse every cached photo"
    );
    assert!(
        light.modified_paths.is_empty() && light.removed_paths.is_empty(),
        "unchanged generated tree: modified={:?} removed={:?}",
        light.modified_paths,
        light.removed_paths
    );
    // Wall-clock light < cold is not a gate: both walks are a few hundred
    // milliseconds on a warm disk, and the second pass can lose. The cache
    // counters above are the regression signal.

    let mut photos = cold.flat_photos.clone();
    let enrich_ms = millis(|| enrich_photos(&mut photos));
    let tagged_photos = photos
        .iter()
        .filter(|p| !p.hierarchical_tags.is_empty())
        .count();
    let gps_photos = photos
        .iter()
        .filter(|p| p.gps_latitude.is_some() && p.gps_longitude.is_some())
        .count();
    let dated_from_metadata = photos.iter().filter(|p| p.date_from_metadata).count();
    assert!(
        tagged_photos > 0,
        "enrichment must read digiKam tags from .xmp"
    );
    assert!(
        dated_from_metadata > 0,
        "enrichment must read EXIF capture dates"
    );

    let mut index = None;
    let index_build_ms = millis(|| index = Some(LibraryIndex::build(photos.clone())));
    let index = index.unwrap();
    let (all_tags, _people) = index.tag_suggestions();
    let mut anna = Vec::new();
    let search_anna_ms = millis(|| anna = index.search("anna", &[], &all_tags));
    let mut rome = Vec::new();
    let search_rome_ms = millis(|| rome = index.search("rome", &[], &all_tags));
    assert!(!anna.is_empty(), "search 'anna' must hit People/Anna… tags");
    assert!(
        !rome.is_empty(),
        "search 'rome' must hit Places/…/Rome tags"
    );
    assert!(
        search_anna_ms < SEARCH_BUDGET_MS,
        "search anna took {search_anna_ms:.2} ms (budget {SEARCH_BUDGET_MS} ms)"
    );
    assert!(
        search_rome_ms < SEARCH_BUDGET_MS,
        "search rome took {search_rome_ms:.2} ms (budget {SEARCH_BUDGET_MS} ms)"
    );

    let now = AppleDate::from_unix_secs_f64(parse_today(&today).as_naive_unix_secs() as f64);
    let mut inputs = GenerationInputs::empty(now, UtcOffset::UTC, today.clone());
    if let Some(root_folder) = &cold.root_folder {
        inputs.leaf_folders = leaf_folders(root_folder);
    }
    inputs = inputs.with_photos(photos);
    let mut memories = Vec::new();
    let memories_generate_ms = millis(|| memories = generate(&inputs));
    let mut scheduled = Vec::new();
    let horizon_ms = millis(|| {
        scheduled = compute_scheduled(&inputs, SCHEDULED_MEMORY_HORIZON_DAYS, &Default::default())
    });
    assert!(
        !memories.is_empty(),
        "generate() must produce memories from the enriched tree"
    );
    assert!(
        horizon_ms < HORIZON_BUDGET_MS,
        "7-day horizon took {horizon_ms:.1} ms (Finding 3 budget {HORIZON_BUDGET_MS} ms)"
    );

    let structure = Structure {
        version: 1,
        count,
        seed,
        today: today.clone(),
        stills,
        videos,
        sidecars: cold.sidecar_manifest.len(),
        folders: cold.stats.folders,
        files: cold.flat_photos.len(),
        tagged_photos,
        gps_photos,
        dated_from_metadata,
        search_anna: anna.len(),
        search_rome: rome.len(),
        memories_generated: memories.len(),
        scheduled_horizon: scheduled.len(),
    };
    let timing = Timing {
        cold_ms,
        light_ms,
        enrich_ms,
        index_build_ms,
        search_anna_ms,
        search_rome_ms,
        memories_generate_ms,
        horizon_ms,
    };

    let s = cold.stats;
    println!(
        "\nScan totals: {} files in {} folders, list={}ms hits={} slow={}\n\
         stills={stills} videos={videos} sidecars={}\n\
         tagged={tagged_photos} gps={gps_photos} dated={dated_from_metadata}\n\
         search anna={} rome={}\n\
         memories={} scheduled={}\n\
         cold={cold_ms:.1}ms light={light_ms:.1}ms enrich={enrich_ms:.1}ms\n\
         index={index_build_ms:.1}ms search={search_anna_ms:.2}/{search_rome_ms:.2}ms\n\
         generate={memories_generate_ms:.1}ms horizon={horizon_ms:.1}ms\n",
        cold.flat_photos.len(),
        s.folders,
        s.list_micros / 1000,
        s.cache_hits,
        s.slow_path,
        cold.sidecar_manifest.len(),
        anna.len(),
        rome.len(),
        memories.len(),
        scheduled.len(),
    );

    assert!(
        cold_ms < scaled(60_000.0, count, 5_000.0),
        "cold scan took {cold_ms:.0} ms"
    );
    assert!(
        light_ms < scaled(15_000.0, count, 2_000.0),
        "light scan took {light_ms:.0} ms"
    );
    assert!(
        enrich_ms < scaled(180_000.0, count, 15_000.0),
        "enrich took {enrich_ms:.0} ms"
    );
    assert!(
        index_build_ms < scaled(5_000.0, count, 500.0),
        "index build took {index_build_ms:.0} ms"
    );
    assert!(
        memories_generate_ms < scaled(10_000.0, count, 1_000.0),
        "memories generate took {memories_generate_ms:.0} ms"
    );

    let golden = baseline_path(count, seed, &today);
    if recording() {
        write_json(&golden, &structure);
        write_json(&timing_path(&root), &timing);
        println!(
            "recorded structure {}\nrecorded timing {}",
            golden.display(),
            timing_path(&root).display()
        );
    } else if golden.is_file() {
        let want: Structure = serde_json::from_str(&fs::read_to_string(&golden).unwrap()).unwrap();
        assert_structure(&structure, &want);
    } else {
        println!(
            "no committed golden at {} — qualitative gates only. Record with LOCALGALLERY_E2E_RECORD=1\n{}",
            golden.display(),
            serde_json::to_string_pretty(&structure).unwrap()
        );
    }

    let timing_file = timing_path(&root);
    if recording() {
        // already written
    } else if timing_file.is_file() {
        let recorded: Timing =
            serde_json::from_str(&fs::read_to_string(&timing_file).unwrap()).unwrap();
        assert_timing_vs_recorded(&timing, &recorded);
    } else {
        write_json(&timing_file, &timing);
        println!("wrote first-run timing baseline {}", timing_file.display());
    }
}
