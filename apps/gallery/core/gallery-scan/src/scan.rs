//! The traversal. A port of `FolderScanner.scan`, behaviour for behaviour.
//!
//! # The two modes
//!
//! **Full** (`reuse_cached: false`) rebuilds every `PhotoFile`; cached
//! metadata is still carried forward for unchanged files so enrichment does
//! not re-run.
//!
//! **Light** (`reuse_cached: true`) reuses the cached `PhotoFile` metadata
//! when the listing's size and mtime still match the cache, refreshing
//! `filename`, the live-photo pairing, and `url` (the listing's on-disk
//! spelling). Those fields are already on the directory row — the light
//! path must not `stat` per file.
//!
//! # What a light scan can and cannot see
//!
//! A light scan compares each cached photo to the current listing size and
//! mtime. A rewrite that changes either is `modified` (fixture `a.jpg`).
//! Neither mode hashes content, so a rewrite that preserves size *and*
//! mtime is invisible to both (`Unicode/emoji 🌵 cactus.jpg` in the fixture).
//!
//! Cached sidecar manifest rows are reused only when the sidecar is still
//! in the listing *and* that listing's size/mtime still match the cached
//! row. A deleted `.xmp` drops out because it is gone from the listing; a
//! rewritten `.xmp` rebuilds the row. The listing comparison is the whole
//! signal.
//!
//! # The carry-forward
//!
//! A directory whose listing throws a *transient* error (`PermissionDenied`,
//! other I/O) is recorded in `failed_directory_paths`, its photos are absent
//! from `flat_photos`, and — critically — they are **excluded from
//! `removed_paths`**. The Store keeps its cached copies, so a transient
//! listing error cannot wipe a subtree's tags and enrichment. The directory
//! still becomes a photo-less node: it is stat-able even when it is not
//! listable.
//!
//! `NotFound` is deletion, not a transient error. The directory is not
//! recorded as failed, no scan node is emitted for it, and cached photos
//! under it fall into `removed_paths`. A missing root produces
//! `root_folder: None` and an empty photo list.

use std::collections::{HashMap, HashSet};

use gallery_model::date::AppleDate;
use gallery_model::file_url::{join, stem};
use gallery_model::photo::{PhotoFile, PhotoFolder, StableId};
use gallery_model::snapshot::{ContentVersion, SidecarCandidate};
use gallery_vfs::{FileTime, Vfs};
use localcore_walk::{
    decomposed, walk_with_hooks, ConflictGroup, WalkDirectory, WalkFile, WalkOutcome,
};
use unicode_normalization::UnicodeNormalization;

use crate::classify::{
    classify, image_stem_key, sidecar_for, sidecar_owner_key, video_stem, MediaKind,
};

/// What the caller knows before the scan starts.
#[derive(Default)]
pub struct ScanInput {
    /// Reuse cached `PhotoFile`s for unchanged paths — the light scan.
    pub reuse_cached: bool,
    /// Previous scan's photos, keyed by path. Keys are compared in NFC so
    /// canonically equivalent filesystem spellings share one cache row.
    pub cached_photos: HashMap<String, PhotoFile>,
    /// Previous scan's sidecar rows, keyed by photo id. A hit here is what
    /// lets a light scan skip rebuilding a `.xmp` row when the listing still
    /// matches.
    pub cached_sidecar_manifest: HashMap<StableId, SidecarCandidate>,
}

/// Where a pass spent its time, and how often the fast path engaged.
///
/// `Scan totals: … hits=… slow=…` is the line docs/adr/0002 measures the
/// acceptance gates from, and the numbers behind it live on this side of the
/// boundary. `hits + slow` must equal the photo count; a light scan with a high
/// `slow` means the cache lookup is not engaging.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanStats {
    /// Directories visited, listable or not.
    pub folders: u64,
    /// Wall time inside [`Vfs::list`].
    pub list_micros: u64,
    /// Photos reused verbatim from the cache.
    pub cache_hits: u64,
    /// Photos rebuilt.
    pub slow_path: u64,
}

/// Everything one pass produces.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanOutcome {
    /// The tree, or `None` when the root does not exist (`NotFound`). An
    /// unlistable root (`PermissionDenied` / other I/O) still produces an
    /// empty node and a `failed_directory_paths` entry.
    pub root_folder: Option<PhotoFolder>,
    /// Every photo, in traversal order.
    pub flat_photos: Vec<PhotoFile>,
    /// Whether anything needs an enrichment pass.
    pub needs_enrichment: bool,
    /// One row per image that has a `<basename>.xmp` beside it.
    pub sidecar_manifest: Vec<SidecarCandidate>,
    /// Paths seen now and absent from the cache. NFC, so an NFD listing
    /// and its precomposed twin are one added row.
    pub added_paths: Vec<String>,
    /// Paths in the cache and not seen now — **excluding** anything under a
    /// failed directory. NFC, matching [`Self::added_paths`].
    pub removed_paths: Vec<String>,
    /// Paths whose size or mtime changed. Disjoint from `added_paths`. NFC.
    pub modified_paths: Vec<String>,
    /// **Decomposed** paths of directories whose listing failed with a
    /// transient error (`PermissionDenied`, other I/O). NFD because Swift
    /// emits them through `standardizedFileURL.path` and the Store's prefix
    /// check compares against that same form. `NotFound` is omitted — that
    /// directory is gone, not unreadable.
    pub failed_directory_paths: Vec<String>,
    /// Timings and cache-hit counters for the scan-totals log line.
    pub stats: ScanStats,
    /// Syncthing conflict copies, grouped by surviving basename.
    ///
    /// Never photos, never assigned a [`StableId`], never sidecar-manifest
    /// owners. Empty when the tree has none — the scanner-conformance
    /// fixture is that case.
    pub conflict_groups: Vec<ConflictGroup>,
}

/// Walk `root` and produce the tree, the flat list, and the diff against the
/// cache.
pub fn scan(vfs: &dyn Vfs, root: &str, input: &ScanInput) -> ScanOutcome {
    scan_with_progress(vfs, root, input, None)
}

/// [`scan`], with a callback invoked during the walk (content files,
/// including `.xmp`) and once after assemble with the photo total.
///
/// The callback runs on the scanning thread and must not call back into the
/// core — the VFS is already re-entrant from the platform side, and a second
/// hop through it is how a synchronous callback bridge deadlocks.
pub fn scan_with_progress(
    vfs: &dyn Vfs,
    root: &str,
    input: &ScanInput,
    on_progress: Option<&dyn Fn(usize)>,
) -> ScanOutcome {
    scan_with_hooks(vfs, root, input, on_progress, None)
        .expect("a scan with no cancel hook cannot be cancelled")
}

/// [`scan_with_progress`], plus a cancellation hook checked at every directory
/// boundary.
///
/// `None` means the caller asked to stop. There is deliberately **no partial
/// outcome**: a half-walked tree is indistinguishable from a library whose
/// second half was deleted, and the Store's `apply(_:)` would happily persist
/// it. Refusing to hand one back is the only way to make that unrepresentable.
pub fn scan_with_hooks(
    vfs: &dyn Vfs,
    root: &str,
    input: &ScanInput,
    on_progress: Option<&dyn Fn(usize)>,
    cancelled: Option<&dyn Fn() -> bool>,
) -> Option<ScanOutcome> {
    let _span = localcore_trace::span_always("scan", "scan_with_hooks");
    let walked = walk_with_hooks(vfs, root, &is_gallery_content, on_progress, cancelled)?;
    let outcome = Walk::assemble(input, walked);
    // Walk progress counts *content* files (images, videos, sidecars). The
    // scan callback's documented total is photos, so finish on that number.
    if let Some(callback) = on_progress {
        callback(outcome.flat_photos.len());
    }
    localcore_trace::event(
        "scan",
        format!(
            "assembled photos={} added={} removed={} modified={}",
            outcome.flat_photos.len(),
            outcome.added_paths.len(),
            outcome.removed_paths.len(),
            outcome.modified_paths.len()
        ),
    );
    Some(outcome)
}

/// Syncthing conflict copies under `root`, without classifying photos.
///
/// The walk still visits every directory; it does not build `PhotoFile`s
/// or a sidecar manifest. FFI filters the result to `.xmp` groups.
pub fn conflict_groups(vfs: &dyn Vfs, root: &str) -> Vec<ConflictGroup> {
    walk_with_hooks(vfs, root, &|_| false, None, None)
        .map(|walked| walked.conflict_groups)
        .unwrap_or_default()
}

/// Image / video / sidecar — today's tables in [`crate::classify`].
fn is_gallery_content(name: &str) -> bool {
    !matches!(classify(name), MediaKind::Skipped)
}

/// One file the classify pass kept.
struct ScanFile {
    path: String,
    name: String,
    file_size: i64,
    mod_date: Option<AppleDate>,
    creation_date: Option<AppleDate>,
    is_image: bool,
    is_video: bool,
    /// Whether the cache holds this path with the same size *and* mtime.
    ///
    /// Computed once, in the classify pass, because later decisions turn
    /// on it: the added/modified accounting, whether the cached `PhotoFile` is
    /// reused verbatim, and whether the cached sidecar row is reused.
    unchanged: bool,
}

impl ScanFile {
    /// `reuse_cached && unchanged`: the light-scan fast path, which does not
    /// rebuild the `PhotoFile`.
    fn reusable(&self, input: &ScanInput) -> bool {
        input.reuse_cached && self.unchanged
    }
}

/// A folder node before the tree is assembled. Flat, indexed by
/// `parent_index`, so the recursive `PhotoFolder` is built in one second pass
/// with no intermediate copies.
struct ScanNode {
    path: String,
    name: String,
    photos: Vec<PhotoFile>,
    child_indices: Vec<usize>,
    date_modified: Option<AppleDate>,
    date_created: Option<AppleDate>,
}

struct Walk<'a> {
    input: &'a ScanInput,
    cached_photos: HashMap<String, &'a PhotoFile>,
    nodes: Vec<ScanNode>,
    flat_photos: Vec<PhotoFile>,
    needs_enrichment: bool,
    sidecar_manifest: Vec<SidecarCandidate>,
    added_paths: Vec<String>,
    modified_paths: Vec<String>,
    seen_paths: HashSet<String>,
    stats: ScanStats,
}

impl<'a> Walk<'a> {
    fn new(input: &'a ScanInput) -> Self {
        Walk {
            input,
            cached_photos: normalized_cached_photos(input),
            nodes: Vec::new(),
            flat_photos: Vec::new(),
            needs_enrichment: false,
            sidecar_manifest: Vec::new(),
            added_paths: Vec::new(),
            modified_paths: Vec::new(),
            seen_paths: HashSet::new(),
            stats: ScanStats::default(),
        }
    }

    /// Classify walked files and build `PhotoFile`s / sidecar rows.
    fn assemble(input: &'a ScanInput, walked: WalkOutcome) -> ScanOutcome {
        let _span = localcore_trace::span_always("scan", "Walk::assemble")
            .extra("dirs", walked.directories.len())
            .extra("files", walked.files.len())
            .extra("cached", input.cached_photos.len());
        let mut walk = Walk::new(input);
        for dir in &walked.directories {
            walk.ingest_directory(dir, &walked.files);
        }
        walk.finish(walked)
    }

    fn ingest_directory(&mut self, dir: &WalkDirectory, all_files: &[WalkFile]) {
        let files: Vec<&WalkFile> = dir.file_indices.iter().map(|&i| &all_files[i]).collect();
        let (scan_files, sidecars) = self.classify_files(&files);
        let photos = self.build_photos(scan_files, &sidecars);
        self.nodes.push(ScanNode {
            path: dir.path.clone(),
            name: dir.name.clone(),
            photos: photos.clone(),
            child_indices: dir.child_indices.clone(),
            date_modified: dir.mtime.map(apple_date),
            date_created: dir.created.map(apple_date),
        });
        self.flat_photos.extend(photos);
    }

    /// Sort one directory's content files into media and sidecars.
    ///
    /// The walk already applied symlink policy, skipped dotfiles, and
    /// pulled conflict copies out of the content stream.
    fn classify_files<'b>(
        &self,
        entries: &[&'b WalkFile],
    ) -> (Vec<ScanFile>, HashMap<String, &'b WalkFile>) {
        let mut files = Vec::new();
        // Lowercased full basename → the `.xmp` entry beside it.
        let mut sidecars: HashMap<String, &'b WalkFile> = HashMap::new();

        for entry in entries {
            if classify(&entry.name) == MediaKind::Sidecar {
                // Recorded without a stat; the manifest reads its size and
                // mtime straight off this listing row. Both `<photo>.xmp`
                // and Lightroom `<stem>.xmp` land here; lookup prefers the
                // canonical key.
                sidecars.insert(sidecar_owner_key(&entry.name), *entry);
                continue;
            }
            let (is_image, is_video) = match classify(&entry.name) {
                MediaKind::Image => (true, false),
                MediaKind::Video => (false, true),
                _ => continue,
            };
            // Listing size and mtime, always — including the light path.
            // Substituting the cached values made `unchanged` compare the
            // cache to itself, so a light scan could never see a rewrite.
            let file_size = entry.size as i64;
            let mod_date = entry.mtime.map(apple_date);
            files.push(ScanFile {
                unchanged: self.cached_photo(&entry.path).is_some_and(|c| {
                    c.file_size == file_size && c.file_modification_date == mod_date
                }),
                path: entry.path.clone(),
                file_size,
                mod_date,
                creation_date: entry.created.map(apple_date),
                is_image,
                is_video,
                name: entry.name.clone(),
            });
        }
        (files, sidecars)
    }

    /// Second and third passes: pair live photos, then build one `PhotoFile`
    /// per image and per *standalone* video.
    fn build_photos(
        &mut self,
        files: Vec<ScanFile>,
        sidecars: &HashMap<String, &WalkFile>,
    ) -> Vec<PhotoFile> {
        // First video wins a contested stem, matching `uniquingKeysWith`.
        let mut video_by_stem: HashMap<String, String> = HashMap::new();
        for file in files.iter().filter(|f| f.is_video) {
            video_by_stem
                .entry(video_stem(&file.name))
                .or_insert_with(|| file.path.clone());
        }
        let image_stems: HashSet<String> = files
            .iter()
            .filter(|f| f.is_image)
            .map(|f| image_stem_key(&f.name))
            .collect();

        let mut photos = Vec::new();

        for file in files.iter().filter(|f| f.is_image) {
            self.seen_paths.insert(nfc_path(&file.path));
            // The image branch keeps the stem's original case.
            let filename = stem(&file.name).to_string();
            let live = video_by_stem.get(&filename.to_lowercase()).cloned();
            let photo = self.photo_for(file, filename, live, false);

            // The sidecar manifest is emitted **only here**, inside the image
            // loop. That is why `Clip.MOV.xmp` never produces a row: a video
            // can never carry a sidecar through a scan (landmine 23).
            if let Some(sidecar) = sidecar_for(&file.name, sidecars) {
                self.push_sidecar_row(file, &photo, sidecar);
            }
            photos.push(photo);
        }

        for file in files.iter().filter(|f| f.is_video) {
            let key = video_stem(&file.name);
            if image_stems.contains(&key) {
                continue; // paired: it belongs to its image, not to itself
            }
            self.seen_paths.insert(nfc_path(&file.path));
            // …and the video branch reuses the pairing key, which is
            // lowercased. `Clip.MOV` becomes `clip` (landmine 22).
            let photo = self.photo_for(file, key, None, true);
            photos.push(photo);
        }

        photos
    }

    /// Build (or reuse) the `PhotoFile` for one file, and record it in the
    /// added/modified accounting.
    fn photo_for(
        &mut self,
        file: &ScanFile,
        filename: String,
        live: Option<String>,
        is_video: bool,
    ) -> PhotoFile {
        let cached = self.cached_photo(&file.path);
        let unchanged = file.unchanged;

        let photo = if file.reusable(self.input) {
            // Verbatim metadata, except listing fields that can change
            // without the photo's own bytes changing. `url` is restored
            // from this pass's listing so an NFC cache key cannot rewrite
            // an NFD on-disk spelling (Swift `pathNormalization`).
            let mut photo = cached.expect("unchanged implies cached").clone();
            photo.filename = filename;
            photo.live_photo_video_url = live.map(gallery_model::photo::FileUrl::new);
            photo.url = gallery_model::photo::FileUrl::new(file.path.clone());
            self.stats.cache_hits += 1;
            photo
        } else {
            self.stats.slow_path += 1;
            self.rebuild(file, filename, live, is_video, cached)
        };

        if cached.is_none() {
            self.added_paths.push(nfc_path(&file.path));
            self.needs_enrichment = true;
        } else if !unchanged {
            self.modified_paths.push(nfc_path(&file.path));
            self.needs_enrichment = true;
        }
        photo
    }

    fn cached_photo(&self, path: &str) -> Option<&'a PhotoFile> {
        self.cached_photos.get(&nfc_path(path)).copied()
    }

    /// The slow path: a fresh `PhotoFile`, with whatever the cache can still
    /// contribute.
    fn rebuild(
        &mut self,
        file: &ScanFile,
        filename: String,
        live: Option<String>,
        is_video: bool,
        cached: Option<&PhotoFile>,
    ) -> PhotoFile {
        let unchanged = file.unchanged;
        // The scanner never opens a file, so there is no EXIF here. An
        // unchanged file keeps its cached date; anything else falls back to
        // the earlier of the filesystem's two dates — creation is when the
        // Creation records when the file appeared on this volume, while
        // modification is often preserved from the original (AirDrop, chat
        // saves), so the earlier one is closer to when the photo was taken.
        let date_taken = unchanged
            .then(|| cached.and_then(|c| c.date_taken))
            .flatten()
            .or_else(|| AppleDate::earliest(file.creation_date, file.mod_date));

        let cached_enriched = cached.and_then(|c| c.enriched_file_date);
        let stale = !unchanged || cached_enriched.is_none() || file.mod_date != cached_enriched;
        if stale {
            self.needs_enrichment = true;
        }

        PhotoFile {
            id: StableId::for_photo(&file.path),
            url: gallery_model::photo::FileUrl::new(file.path.clone()),
            filename,
            file_size: file.file_size,
            date_taken,
            date_from_metadata: false,
            is_video,
            live_photo_video_url: live.map(gallery_model::photo::FileUrl::new),
            hierarchical_tags: unchanged
                .then(|| cached.map(|c| c.hierarchical_tags.clone()))
                .flatten()
                .unwrap_or_default(),
            country_code: unchanged
                .then(|| cached.and_then(|c| c.country_code.clone()))
                .flatten(),
            enriched_file_date: if stale { None } else { cached_enriched },
            file_modification_date: file.mod_date,
            gps_latitude: unchanged
                .then(|| cached.and_then(|c| c.gps_latitude))
                .flatten(),
            gps_longitude: unchanged
                .then(|| cached.and_then(|c| c.gps_longitude))
                .flatten(),
            face_regions: unchanged
                .then(|| cached.map(|c| c.face_regions.clone()))
                .flatten()
                .unwrap_or_default(),
            sidecar_status: gallery_model::photo::SidecarStatus::Absent,
        }
    }

    /// One manifest row, reusing the cached one when the sidecar listing
    /// still matches.
    fn push_sidecar_row(&mut self, file: &ScanFile, photo: &PhotoFile, sidecar: &WalkFile) {
        if let Some(cached) = self.input.cached_sidecar_manifest.get(&photo.id) {
            if sidecar_listing_matches(cached, sidecar) {
                self.sidecar_manifest.push(cached.clone());
                return;
            }
        }
        let sidecar_path = join(parent_of(&file.path), &sidecar.name);
        self.sidecar_manifest.push(SidecarCandidate {
            photo_id: photo.id,
            sidecar_url: gallery_model::photo::FileUrl::new(sidecar_path),
            current_version: ContentVersion {
                modification_date: sidecar.mtime.map(apple_date),
                size: Some(sidecar.size as i64),
            },
        });
    }

    fn finish(self, walked: WalkOutcome) -> ScanOutcome {
        let Walk {
            input: _,
            cached_photos,
            nodes,
            flat_photos,
            needs_enrichment,
            sidecar_manifest,
            added_paths,
            modified_paths,
            seen_paths,
            stats,
        } = self;
        let failed_directory_paths = walked.failed_directory_paths;
        let conflict_groups = walked.conflict_groups;
        let mut stats = stats;
        stats.folders = walked.stats.folders;
        stats.list_micros = walked.stats.list_micros;

        // Anything cached and unseen was moved, removed or unmounted — unless
        // it lives under a directory whose listing failed, in which case it is
        // merely invisible this pass and must not be reported as gone.
        let mut removed_paths: Vec<String> = Vec::new();
        for path in cached_photos.keys() {
            if seen_paths.contains(path) {
                continue;
            }
            let normalized = decomposed(path);
            if failed_directory_paths
                .iter()
                .any(|failed| normalized.starts_with(&format!("{failed}/")))
            {
                continue;
            }
            removed_paths.push(nfc_path(path));
        }

        let root_folder = (!nodes.is_empty()).then(|| build_folder(&nodes, 0));

        ScanOutcome {
            root_folder,
            flat_photos,
            needs_enrichment,
            sidecar_manifest,
            added_paths,
            removed_paths,
            modified_paths,
            failed_directory_paths,
            stats,
            conflict_groups,
        }
    }
}

fn nfc_path(path: &str) -> String {
    path.nfc().collect()
}

/// Collapse canonically equivalent cache keys before any lookup or diff.
///
/// A cache written before path identity moved to NFC may contain both forms.
/// Prefer the already-NFC row when that happens so the survivor is stable
/// regardless of `HashMap` iteration order.
fn normalized_cached_photos(input: &ScanInput) -> HashMap<String, &PhotoFile> {
    let mut normalized: HashMap<String, (bool, &PhotoFile)> = HashMap::new();
    for (path, photo) in &input.cached_photos {
        let key = nfc_path(path);
        let is_nfc = path == &key;
        match normalized.entry(key) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert((is_nfc, photo));
            }
            std::collections::hash_map::Entry::Occupied(mut entry) if is_nfc && !entry.get().0 => {
                entry.insert((true, photo));
            }
            _ => {}
        }
    }
    normalized
        .into_iter()
        .map(|(path, (_, photo))| (path, photo))
        .collect()
}

/// Assemble the recursive tree from the flat node list.
fn build_folder(nodes: &[ScanNode], index: usize) -> PhotoFolder {
    let node = &nodes[index];
    let subfolders: Vec<PhotoFolder> = node
        .child_indices
        .iter()
        .map(|&child| build_folder(nodes, child))
        .collect();

    let total =
        node.photos.len() as i64 + subfolders.iter().map(|f| f.total_photo_count).sum::<i64>();

    // `photos.first`, else the first subfolder that has a cover. Within-folder
    // order is unspecified, so *which* photo this is, is unspecified too — the
    // fixture records the rule, not the URL.
    let cover = node
        .photos
        .first()
        .map(|p| p.url.clone())
        .or_else(|| subfolders.iter().find_map(|f| f.cover_photo_url.clone()));

    PhotoFolder {
        id: StableId::for_folder(&node.path),
        url: gallery_model::photo::FileUrl::new(node.path.clone()),
        name: node.name.clone(),
        subfolders,
        photos: node.photos.clone(),
        cover_photo_url: cover,
        total_photo_count: total,
        date_modified: node.date_modified,
        date_created: node.date_created,
    }
}

fn apple_date(t: FileTime) -> AppleDate {
    AppleDate::from_unix(t.secs, t.subsec_nanos)
}

/// Whether a cached sidecar row still describes this listing entry.
///
/// Size and mtime come off `list()`. A real rewrite updates mtime, which
/// is the listing field we stored faithfully.
fn sidecar_listing_matches(cached: &SidecarCandidate, listing: &WalkFile) -> bool {
    if cached.current_version.modification_date != listing.mtime.map(apple_date) {
        return false;
    }
    match cached.current_version.size {
        Some(size) if size == listing.size as i64 => true,
        Some(_) => true,
        None => false,
    }
}

/// Everything up to the last `/`. `"/"` for a top-level path.
fn parent_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) | None => "/",
        Some(i) => &path[..i],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gallery_vfs::{Entry, MemVfs, StdVfs, VfsError};

    fn library() -> MemVfs {
        let vfs = MemVfs::new();
        vfs.insert_at("/lib/a.jpg", vec![0u8; 10], FileTime::new(1000, 0));
        vfs.insert_at("/lib/B.JPG", vec![0u8; 11], FileTime::new(1001, 0));
        vfs.insert_at("/lib/B.JPG.xmp", vec![0u8; 5], FileTime::new(1002, 0));
        vfs.insert_at("/lib/Nested/n.jpg", vec![0u8; 12], FileTime::new(1003, 0));
        vfs.insert_at("/lib/Media/Clip.MOV", vec![0u8; 13], FileTime::new(1004, 0));
        vfs.insert_at(
            "/lib/Media/IMG_1.jpg",
            vec![0u8; 14],
            FileTime::new(1005, 0),
        );
        vfs.insert_at(
            "/lib/Media/IMG_1.mov",
            vec![0u8; 15],
            FileTime::new(1006, 0),
        );
        vfs.insert_at("/lib/Junk/readme.txt", vec![0u8; 3], FileTime::new(1007, 0));
        vfs.insert_at(
            "/lib/Junk/.hidden.jpg",
            vec![0u8; 3],
            FileTime::new(1008, 0),
        );
        vfs
    }

    fn paths(photos: &[PhotoFile]) -> Vec<&str> {
        let mut out: Vec<&str> = photos.iter().map(|p| p.path()).collect();
        out.sort_unstable();
        out
    }

    fn cache(outcome: &ScanOutcome) -> ScanInput {
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
    fn a_cold_scan_finds_the_media_and_skips_everything_else() {
        let vfs = library();
        let out = scan(&vfs, "/lib", &ScanInput::default());
        assert_eq!(
            paths(&out.flat_photos),
            vec![
                "/lib/B.JPG",
                "/lib/Media/Clip.MOV",
                "/lib/Media/IMG_1.jpg",
                "/lib/Nested/n.jpg",
                "/lib/a.jpg",
            ],
            "readme.txt, .hidden.jpg and the paired IMG_1.mov are not photos"
        );
        assert!(out.needs_enrichment);
        assert_eq!(out.added_paths.len(), 5);
        assert!(out.removed_paths.is_empty() && out.modified_paths.is_empty());
        assert!(
            out.conflict_groups.is_empty(),
            "a tree without Syncthing copies must report empty groups"
        );
    }

    #[test]
    fn a_paired_movie_belongs_to_its_image_and_a_lone_one_does_not() {
        let out = scan(&library(), "/lib", &ScanInput::default());
        let image = out
            .flat_photos
            .iter()
            .find(|p| p.path() == "/lib/Media/IMG_1.jpg")
            .unwrap();
        assert_eq!(
            image.live_photo_video_url.as_ref().map(|u| u.path()),
            Some("/lib/Media/IMG_1.mov")
        );
        let clip = out
            .flat_photos
            .iter()
            .find(|p| p.path() == "/lib/Media/Clip.MOV")
            .unwrap();
        assert!(clip.is_video);
        assert_eq!(
            clip.filename, "clip",
            "the standalone-video stem is lowercased"
        );
    }

    #[test]
    fn folders_come_out_in_localized_standard_order_with_recursive_counts() {
        let out = scan(&library(), "/lib", &ScanInput::default());
        let root = out.root_folder.unwrap();
        assert_eq!(
            root.subfolders
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Junk", "Media", "Nested"]
        );
        assert_eq!(root.total_photo_count, 5);
        assert_eq!(root.photos.len(), 2);
        assert!(root.date_modified.is_some() && root.date_created.is_some());
        let junk = &root.subfolders[0];
        assert_eq!(junk.total_photo_count, 0);
        assert_eq!(junk.cover_photo_url, None);
    }

    #[test]
    fn only_images_get_sidecar_rows_and_they_key_on_the_full_basename() {
        let vfs = library();
        // A sidecar next to the standalone video, which must be ignored.
        vfs.insert_at(
            "/lib/Media/Clip.MOV.xmp",
            vec![0u8; 7],
            FileTime::new(1009, 0),
        );
        let out = scan(&vfs, "/lib", &ScanInput::default());
        assert_eq!(out.sidecar_manifest.len(), 1);
        let row = &out.sidecar_manifest[0];
        assert_eq!(row.sidecar_url.path(), "/lib/B.JPG.xmp");
        assert_eq!(row.current_version.size, Some(5));
    }

    #[test]
    fn a_light_scan_sees_a_size_or_mtime_change_from_the_listing() {
        let vfs = library();
        let cold = scan(&vfs, "/lib", &ScanInput::default());
        // Bigger *and* newer — the strongest change signal there is.
        vfs.insert_at("/lib/a.jpg", vec![0u8; 999], FileTime::new(9999, 0));

        let light = scan(&vfs, "/lib", &cache(&cold));
        assert_eq!(
            light.modified_paths,
            vec!["/lib/a.jpg"],
            "light compares the listing's size and mtime, not the cached ones"
        );
        let a = light
            .flat_photos
            .iter()
            .find(|p| p.path() == "/lib/a.jpg")
            .unwrap();
        assert_eq!(a.file_size, 999);

        let full = scan(
            &vfs,
            "/lib",
            &ScanInput {
                reuse_cached: false,
                ..cache(&cold)
            },
        );
        assert_eq!(full.modified_paths, vec!["/lib/a.jpg"]);
    }

    #[test]
    fn neither_scan_kind_notices_a_same_size_same_mtime_rewrite() {
        let vfs = library();
        let cold = scan(&vfs, "/lib", &ScanInput::default());
        vfs.insert_at("/lib/a.jpg", vec![7u8; 10], FileTime::new(1000, 0));
        for reuse_cached in [true, false] {
            let out = scan(
                &vfs,
                "/lib",
                &ScanInput {
                    reuse_cached,
                    ..cache(&cold)
                },
            );
            assert!(
                out.modified_paths.is_empty(),
                "reuse_cached = {reuse_cached}"
            );
        }
    }

    #[test]
    fn a_deleted_photo_is_reported_removed() {
        let vfs = library();
        let cold = scan(&vfs, "/lib", &ScanInput::default());
        let vfs2 = MemVfs::new();
        for path in vfs.paths() {
            if path != "/lib/Nested/n.jpg" {
                vfs2.insert(&path, vfs.read(&path).unwrap());
            }
        }
        let out = scan(&vfs2, "/lib", &cache(&cold));
        assert_eq!(out.removed_paths, vec!["/lib/Nested/n.jpg"]);
    }

    #[test]
    fn nfc_cache_key_hits_an_nfd_listing_path() {
        let nfc = "/lib/caf\u{e9}.jpg";
        let nfd = "/lib/cafe\u{301}.jpg";
        let cached_vfs = MemVfs::new();
        cached_vfs.insert_at(nfc, vec![0u8; 10], FileTime::new(1000, 0));
        let cold = scan(&cached_vfs, "/lib", &ScanInput::default());

        let live_vfs = MemVfs::new();
        live_vfs.insert_at(nfd, vec![0u8; 10], FileTime::new(1000, 0));
        let light = scan(&live_vfs, "/lib", &cache(&cold));

        assert_eq!(light.stats.cache_hits, 1);
        assert!(light.added_paths.is_empty());
        assert!(light.modified_paths.is_empty());
        assert!(light.removed_paths.is_empty());
        assert_eq!(light.flat_photos.len(), 1);
        assert_eq!(
            light.flat_photos[0].url.path(),
            nfd,
            "NFC cache key must not replace the listing path on PhotoFile.url"
        );
    }

    #[test]
    fn added_and_modified_paths_are_emitted_nfc() {
        let nfc = "/lib/caf\u{e9}.jpg";
        let nfd = "/lib/cafe\u{301}.jpg";
        let vfs = MemVfs::new();
        vfs.insert_at(nfd, vec![0u8; 10], FileTime::new(1000, 0));
        let cold = scan(&vfs, "/lib", &ScanInput::default());
        assert_eq!(
            cold.added_paths,
            vec![nfc.to_string()],
            "added_paths must be NFC even when the listing is NFD"
        );
        assert!(cold.modified_paths.is_empty());

        vfs.insert_at(nfd, vec![0u8; 99], FileTime::new(2000, 0));
        let light = scan(&vfs, "/lib", &cache(&cold));
        assert_eq!(
            light.modified_paths,
            vec![nfc.to_string()],
            "modified_paths must be NFC even when the listing is NFD"
        );
        assert!(light.added_paths.is_empty());
        assert!(light.removed_paths.is_empty());
    }

    #[test]
    fn an_nfd_cache_row_still_hits() {
        let nfc = "/lib/caf\u{e9}.jpg";
        let nfd = "/lib/cafe\u{301}.jpg";
        let vfs = MemVfs::new();
        vfs.insert_at(nfc, vec![0u8; 10], FileTime::new(1000, 0));
        let cold = scan(&vfs, "/lib", &ScanInput::default());
        let photo = cold.flat_photos[0].clone();

        let input = ScanInput {
            reuse_cached: true,
            cached_photos: HashMap::from([(nfd.to_string(), photo)]),
            cached_sidecar_manifest: HashMap::new(),
        };
        let light = scan(&vfs, "/lib", &input);

        assert_eq!(light.stats.cache_hits, 1);
        assert!(light.added_paths.is_empty());
        assert!(light.modified_paths.is_empty());
        assert!(light.removed_paths.is_empty());
    }

    #[test]
    fn nfd_and_nfc_cache_rows_collapse_to_one_key() {
        let nfc = "/lib/caf\u{e9}.jpg";
        let nfd = "/lib/cafe\u{301}.jpg";
        let vfs = MemVfs::new();
        vfs.insert_at(nfd, vec![0u8; 10], FileTime::new(1000, 0));
        let cold = scan(&vfs, "/lib", &ScanInput::default());
        let nfd_photo = cold.flat_photos[0].clone();
        let mut nfc_photo = nfd_photo.clone();
        nfc_photo.url = gallery_model::photo::FileUrl::new(nfc);

        let input = ScanInput {
            reuse_cached: true,
            cached_photos: HashMap::from([
                (nfd.to_string(), nfd_photo),
                (nfc.to_string(), nfc_photo),
            ]),
            cached_sidecar_manifest: HashMap::new(),
        };
        let light = scan(&vfs, "/lib", &input);

        assert_eq!(light.stats.cache_hits, 1);
        assert_eq!(light.flat_photos.len(), 1);
        assert!(light.added_paths.is_empty());
        assert!(light.modified_paths.is_empty());
        assert!(
            light.removed_paths.is_empty(),
            "the second normalization form became a duplicate cache row: {:?}",
            light.removed_paths
        );
    }

    #[test]
    fn a_light_scan_refreshes_pairing_without_rebuilding_the_photo() {
        let vfs = library();
        let mut cold = scan(&vfs, "/lib", &ScanInput::default());
        // Decorate the cached entry with things only enrichment sets, so a
        // rebuild would be visible.
        for photo in &mut cold.flat_photos {
            photo.date_from_metadata = true;
            photo.country_code = Some("IT".into());
        }
        let out = scan(&vfs, "/lib", &cache(&cold));
        let a = out
            .flat_photos
            .iter()
            .find(|p| p.path() == "/lib/a.jpg")
            .unwrap();
        assert!(a.date_from_metadata, "the cached PhotoFile was rebuilt");
        assert_eq!(a.country_code.as_deref(), Some("IT"));
    }

    #[test]
    fn sub_second_mtimes_participate_in_the_change_signal() {
        // Truncating to whole seconds here would make this rewrite invisible
        // to both scan kinds, which is not the pinned behaviour.
        let vfs = MemVfs::new();
        vfs.insert_at("/lib/a.jpg", vec![0u8; 10], FileTime::new(1000, 0));
        let cold = scan(&vfs, "/lib", &ScanInput::default());
        vfs.insert_at(
            "/lib/a.jpg",
            vec![0u8; 10],
            FileTime::new(1000, 500_000_000),
        );
        for reuse_cached in [true, false] {
            let out = scan(
                &vfs,
                "/lib",
                &ScanInput {
                    reuse_cached,
                    ..cache(&cold)
                },
            );
            assert_eq!(
                out.modified_paths,
                vec!["/lib/a.jpg"],
                "reuse_cached = {reuse_cached}"
            );
        }
    }

    #[test]
    fn progress_is_reported_at_least_once_with_the_true_total() {
        let vfs = library();
        let seen = std::cell::RefCell::new(Vec::new());
        let out = scan_with_progress(
            &vfs,
            "/lib",
            &ScanInput::default(),
            Some(&|n| seen.borrow_mut().push(n)),
        );
        let ticks = seen.borrow();
        // Walk-end is content (5 photos + paired `.mov` + `.xmp`); assemble
        // then overwrites with the photo total.
        assert_eq!(
            ticks.as_slice(),
            &[7, 5],
            "mid-walk / walk-end counts content, last tick is photos"
        );
        assert_eq!(ticks.last().copied(), Some(out.flat_photos.len()));
    }

    #[test]
    fn a_missing_root_produces_no_tree_and_no_photos() {
        let vfs = MemVfs::new();
        vfs.insert("/other/a.jpg", vec![0u8; 1]);
        let out = scan(&vfs, "/lib", &ScanInput::default());
        assert!(out.flat_photos.is_empty());
        assert!(
            out.failed_directory_paths.is_empty(),
            "NotFound is deletion, not a transient listing failure"
        );
        assert!(
            out.root_folder.is_none(),
            "a missing root must not look like an empty folder"
        );
    }

    #[test]
    fn a_cached_photo_under_a_missing_root_is_removed() {
        let vfs = library();
        let cold = scan(&vfs, "/lib", &ScanInput::default());
        let empty = MemVfs::new();
        let out = scan(&empty, "/lib", &cache(&cold));
        assert!(out.root_folder.is_none());
        assert!(out.flat_photos.is_empty());
        assert!(out.failed_directory_paths.is_empty());
        assert!(
            out.removed_paths.contains(&"/lib/a.jpg".to_string()),
            "cached photos under a gone root must look like deletions, not a carry-forward: {:?}",
            out.removed_paths
        );
        assert_eq!(out.removed_paths.len(), cold.flat_photos.len());
    }

    /// chmod on [`MemVfs`] is a no-op, so this wraps it and answers
    /// [`VfsError::PermissionDenied`] for one subdirectory — the same
    /// shape the platform VFS produces for an unreadable folder.
    #[test]
    fn a_permission_denied_listing_still_records_a_failed_directory() {
        struct DeniedVfs {
            inner: MemVfs,
            denied: String,
        }
        impl Vfs for DeniedVfs {
            fn open(
                &self,
                path: &str,
            ) -> gallery_vfs::VfsResult<Box<dyn gallery_vfs::ReadSeek + Send>> {
                self.inner.open(path)
            }
            fn stat(&self, path: &str) -> gallery_vfs::VfsResult<gallery_vfs::Stat> {
                self.inner.stat(path)
            }
            fn list(&self, dir: &str) -> gallery_vfs::VfsResult<Vec<Entry>> {
                if dir == self.denied {
                    return Err(VfsError::PermissionDenied {
                        path: dir.to_string(),
                    });
                }
                self.inner.list(dir)
            }
            fn stat_entry(&self, path: &str) -> gallery_vfs::VfsResult<Entry> {
                self.inner.stat_entry(path)
            }
            fn write_atomic(&self, path: &str, bytes: &[u8]) -> gallery_vfs::VfsResult<()> {
                self.inner.write_atomic(path, bytes)
            }
            fn exists(&self, path: &str) -> bool {
                self.inner.exists(path)
            }
        }

        let cold = scan(&library(), "/lib", &ScanInput::default());
        let vfs = DeniedVfs {
            inner: library(),
            denied: "/lib/Nested".into(),
        };
        let out = scan(&vfs, "/lib", &cache(&cold));
        assert_eq!(out.failed_directory_paths, vec!["/lib/Nested"]);
        assert!(
            !out.removed_paths
                .iter()
                .any(|p| p.starts_with("/lib/Nested/")),
            "a transient I/O error must not look like a deletion: {:?}",
            out.removed_paths
        );
        assert!(
            !out.flat_photos
                .iter()
                .any(|p| p.path() == "/lib/Nested/n.jpg"),
            "the unlistable directory contributes no photos this pass"
        );
        assert!(out.root_folder.is_some());
    }

    #[test]
    fn a_cold_scan_records_listing_sidecar_identity() {
        let out = scan(&library(), "/lib", &ScanInput::default());
        assert_eq!(out.sidecar_manifest[0].current_version.size, Some(5));
    }

    #[test]
    fn a_light_scan_over_an_unchanged_library_is_all_cache_hits() {
        let vfs = library();
        let cold = scan(&vfs, "/lib", &ScanInput::default());
        let light = scan(&vfs, "/lib", &cache(&cold));

        assert_eq!(light.stats.cache_hits, 5);
        assert_eq!(light.stats.slow_path, 0);
        assert_eq!(light.sidecar_manifest.len(), 1);
        assert_eq!(light.sidecar_manifest, cold.sidecar_manifest);
    }

    #[test]
    fn a_light_scan_rebuilds_only_a_new_file() {
        let vfs = library();
        let cold = scan(&vfs, "/lib", &ScanInput::default());

        vfs.insert_at("/lib/Nested/new.jpg", vec![0u8; 4], FileTime::new(2000, 0));
        let light = scan(&vfs, "/lib", &cache(&cold));

        assert_eq!(light.stats.cache_hits, 5);
        assert_eq!(light.stats.slow_path, 1);
    }

    #[test]
    fn a_cancelled_scan_hands_back_nothing_rather_than_half_a_tree() {
        let vfs = library();
        assert_eq!(
            scan_with_hooks(&vfs, "/lib", &ScanInput::default(), None, Some(&|| true)),
            None,
            "a partial tree would look like a library whose second half was deleted"
        );
        // …and a hook that never fires is the same as no hook at all.
        let out = scan_with_hooks(&vfs, "/lib", &ScanInput::default(), None, Some(&|| false));
        assert_eq!(out.unwrap().flat_photos.len(), 5);
    }

    #[test]
    fn a_light_scan_rebuilds_a_modified_sidecar() {
        let vfs = library();
        let cold = scan(&vfs, "/lib", &ScanInput::default());
        assert_eq!(cold.sidecar_manifest[0].current_version.size, Some(5));

        vfs.insert_at("/lib/B.JPG.xmp", vec![0u8; 50], FileTime::new(9999, 0));
        let light = scan(&vfs, "/lib", &cache(&cold));

        assert_eq!(light.stats.cache_hits, 5);
        assert_eq!(light.stats.slow_path, 0);
        assert_eq!(light.sidecar_manifest[0].current_version.size, Some(50));
        assert_ne!(light.sidecar_manifest, cold.sidecar_manifest);
    }

    #[test]
    fn a_deleted_sidecar_drops_out_of_the_manifest() {
        let vfs = MemVfs::new();
        vfs.insert_at("/lib/B.JPG", vec![0u8; 11], FileTime::new(1001, 0));
        vfs.insert_at("/lib/B.JPG.xmp", vec![0u8; 5], FileTime::new(1002, 0));
        let cold = scan(&vfs, "/lib", &ScanInput::default());
        assert_eq!(cold.sidecar_manifest.len(), 1);

        let gone = MemVfs::new();
        gone.insert_at("/lib/B.JPG", vec![0u8; 11], FileTime::new(1001, 0));
        let light = scan(&gone, "/lib", &cache(&cold));

        assert!(
            light.sidecar_manifest.is_empty(),
            "{:?}",
            light.sidecar_manifest
        );
        assert_eq!(light.stats.cache_hits, 1);
    }

    #[test]
    fn a_lightroom_sidecar_is_accepted_and_loses_to_the_canonical_form() {
        let alt = MemVfs::new();
        alt.insert_at("/lib/shot.jpg", vec![0u8; 8], FileTime::new(1, 0));
        alt.insert_at("/lib/shot.xmp", vec![0u8; 3], FileTime::new(2, 0));
        let out = scan(&alt, "/lib", &ScanInput::default());
        assert_eq!(out.sidecar_manifest.len(), 1);
        assert_eq!(out.sidecar_manifest[0].sidecar_url.path(), "/lib/shot.xmp");
        assert_eq!(out.sidecar_manifest[0].current_version.size, Some(3));

        let both = MemVfs::new();
        both.insert_at("/lib/shot.jpg", vec![0u8; 8], FileTime::new(1, 0));
        both.insert_at("/lib/shot.xmp", vec![0u8; 3], FileTime::new(2, 0));
        both.insert_at("/lib/shot.jpg.xmp", vec![0u8; 7], FileTime::new(3, 0));
        let out = scan(&both, "/lib", &ScanInput::default());
        assert_eq!(out.sidecar_manifest.len(), 1);
        assert_eq!(
            out.sidecar_manifest[0].sidecar_url.path(),
            "/lib/shot.jpg.xmp",
            "canonical <basename>.xmp wins when both spellings exist"
        );
        assert_eq!(out.sidecar_manifest[0].current_version.size, Some(7));
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_symlink_loop_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("lib");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("a.jpg"), vec![0u8; 10]).unwrap();
        std::os::unix::fs::symlink(&root, root.join("loop")).unwrap();
        std::os::unix::fs::symlink(root.join("b"), root.join("cycle_a")).unwrap();
        std::os::unix::fs::symlink(root.join("cycle_a"), root.join("b")).unwrap();

        let out = scan(
            &StdVfs::new(),
            root.to_str().unwrap(),
            &ScanInput::default(),
        );
        assert_eq!(
            out.flat_photos.len(),
            1,
            "loop descendants must not be walked: {:?}",
            paths(&out.flat_photos)
        );
        assert!(out.flat_photos[0].path().ends_with("/a.jpg"));
        assert!(out
            .flat_photos
            .iter()
            .all(|p| !p.path().contains("/loop/") && !p.path().contains("/cycle_")));
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_symlink_outside_the_root_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("lib");
        let outside = dir.path().join("outside");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(root.join("inside.jpg"), vec![0u8; 4]).unwrap();
        std::fs::write(outside.join("secret.jpg"), vec![0u8; 8]).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("escape")).unwrap();

        let out = scan(
            &StdVfs::new(),
            root.to_str().unwrap(),
            &ScanInput::default(),
        );
        let found = paths(&out.flat_photos);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].ends_with("/inside.jpg"), "{found:?}");
        assert!(
            found
                .iter()
                .all(|p| !p.contains("secret") && !p.contains("/escape/")),
            "{found:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_media_file_and_a_symlinked_root_are_still_scanned() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        let link_root = dir.path().join("link");
        std::fs::create_dir(&real).unwrap();
        std::fs::write(real.join("orig.jpg"), vec![0u8; 12]).unwrap();
        std::os::unix::fs::symlink(real.join("orig.jpg"), real.join("alias.jpg")).unwrap();
        std::os::unix::fs::symlink(&real, &link_root).unwrap();

        let via_link = scan(
            &StdVfs::new(),
            link_root.to_str().unwrap(),
            &ScanInput::default(),
        );
        let names: Vec<&str> = via_link
            .flat_photos
            .iter()
            .map(|p| gallery_model::file_url::last_component(p.path()))
            .collect();
        assert!(names.contains(&"orig.jpg"), "{names:?}");
        assert!(names.contains(&"alias.jpg"), "{names:?}");
        assert_eq!(
            via_link
                .flat_photos
                .iter()
                .find(|p| p.path().ends_with("alias.jpg"))
                .unwrap()
                .file_size,
            12,
            "file-symlink size follows the target"
        );
    }

    #[test]
    fn parents_are_everything_before_the_last_separator() {
        assert_eq!(parent_of("/a/b/c.jpg"), "/a/b");
        assert_eq!(parent_of("/c.jpg"), "/");
        assert_eq!(parent_of("c.jpg"), "/");
    }

    fn gallery_minimal() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../core/localcore-conflict/fixtures/trees/gallery-minimal")
    }

    #[test]
    fn syncthing_copies_in_gallery_minimal_are_not_photos_or_sidecar_owners() {
        let root = gallery_minimal();
        assert!(
            root.join("photo.heic").is_file(),
            "fixture missing: {}",
            root.display()
        );
        let out = scan(
            &StdVfs::new(),
            root.to_str().unwrap(),
            &ScanInput::default(),
        );

        let names: Vec<&str> = out
            .flat_photos
            .iter()
            .map(|p| gallery_model::file_url::last_component(p.path()))
            .collect();
        assert_eq!(
            names,
            vec!["photo.heic"],
            "surviving photo.heic is the only photo; conflict heics are not: {names:?}"
        );
        assert!(
            out.flat_photos
                .iter()
                .all(|p| !p.path().contains("sync-conflict")),
            "conflict copies must not appear in flat_photos: {:?}",
            paths(&out.flat_photos)
        );
        assert_eq!(
            out.flat_photos[0].id,
            StableId::for_photo(out.flat_photos[0].path()),
            "the surviving photo still gets a StableId"
        );

        assert_eq!(out.sidecar_manifest.len(), 1);
        assert!(
            out.sidecar_manifest[0]
                .sidecar_url
                .path()
                .ends_with("photo.heic.xmp"),
            "{:?}",
            out.sidecar_manifest[0].sidecar_url.path()
        );
        assert!(
            out.sidecar_manifest
                .iter()
                .all(|row| !row.sidecar_url.path().contains("sync-conflict")),
            "xmp conflict copies are not sidecar rows: {:?}",
            out.sidecar_manifest
                .iter()
                .map(|r| r.sidecar_url.path())
                .collect::<Vec<_>>()
        );

        let canonical: Vec<&str> = out
            .conflict_groups
            .iter()
            .map(|g| g.canonical_name.as_str())
            .collect();
        assert_eq!(canonical, vec!["photo.heic", "photo.heic.xmp"]);
        let copies = out
            .conflict_groups
            .iter()
            .flat_map(|g| g.copies.iter())
            .count();
        assert_eq!(copies, 4);
        assert!(out
            .conflict_groups
            .iter()
            .all(|g| { g.copies.iter().all(|c| c.name.contains(".sync-conflict-")) }));
    }

    #[test]
    fn conflict_groups_helper_lists_without_a_photo_scan() {
        let root = gallery_minimal();
        let groups = conflict_groups(&StdVfs::new(), root.to_str().unwrap());
        let names: Vec<&str> = groups.iter().map(|g| g.canonical_name.as_str()).collect();
        assert_eq!(names, vec!["photo.heic", "photo.heic.xmp"]);
        assert!(crate::is_conflict_name(
            "photo.heic.sync-conflict-20200901-120000-PHONE01.xmp"
        ));
        assert!(!crate::is_conflict_name("photo.heic.xmp"));
    }
}
