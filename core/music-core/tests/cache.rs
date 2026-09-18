use music_core::{
    canonical_library_root, library_cache_path, parse_library_snapshot, FileTime, LibrarySnapshot,
    MemVfs, MetadataUpdate, StdVfs, Store, Vfs, LIBRARY_CACHE_VERSION,
};

fn enrich(store: &mut Store, title: &str) {
    let id = store.tracks()[0].id.clone();
    store
        .apply_metadata(MetadataUpdate {
            id,
            title: title.into(),
            artist: "Ada".into(),
            album: "Notes".into(),
            duration_ms: 125_000,
            has_artwork: true,
            has_lyrics: false,
        })
        .unwrap();
}

#[test]
fn write_cache_hydrate_paints_before_walk() {
    let vfs = MemVfs::new();
    let mtime = FileTime::new(1_700_000_000, 250);
    vfs.insert_at("/music/song.mp3", b"audio", mtime);
    let mut store = Store::open(&vfs, "/music").unwrap();
    enrich(&mut store, "Café");

    let snapshot = store.to_cache();
    assert_eq!(snapshot.version, LIBRARY_CACHE_VERSION);
    assert_eq!(snapshot.root, canonical_library_root("/music"));
    assert_eq!(snapshot.tracks.len(), 1);
    assert_eq!(snapshot.tracks[0].title, "Café");
    assert_eq!(snapshot.tracks[0].source_size, 5);
    assert_eq!(
        snapshot.tracks[0]
            .source_mtime
            .map(|time| (time.secs, time.subsec_nanos)),
        Some((1_700_000_000, 250))
    );

    let warmed = Store::from_cache(snapshot).unwrap();
    assert_eq!(warmed.tracks()[0].title, "Café");
    assert_eq!(warmed.tracks()[0].artist, "Ada");
    assert_eq!(warmed.tracks()[0].album, "Notes");
    assert_eq!(warmed.tracks()[0].duration_ms, 125_000);
    assert!(warmed.tracks()[0].has_artwork);
    assert!(warmed.tracks()[0].metadata_loaded);
    assert_eq!(warmed.pending_metadata_count(), 0);
    assert!(warmed.playlists().is_empty());
}

#[test]
fn unchanged_size_and_mtime_keeps_cached_metadata() {
    let vfs = MemVfs::new();
    let mtime = FileTime::new(1_700_000_000, 0);
    vfs.insert_at("/music/song.mp3", b"audio", mtime);
    let mut store = Store::open(&vfs, "/music").unwrap();
    enrich(&mut store, "Kept");

    let mut warmed = Store::from_cache(store.to_cache()).unwrap();
    warmed.reload(&vfs).unwrap();
    assert_eq!(warmed.pending_metadata_count(), 0);
    assert_eq!(warmed.tracks()[0].title, "Kept");
    assert!(warmed.tracks()[0].metadata_loaded);
}

#[test]
fn stale_size_or_mtime_leaves_metadata_unloaded() {
    let vfs = MemVfs::new();
    vfs.insert_at("/music/song.mp3", b"audio", FileTime::new(10, 0));
    let mut store = Store::open(&vfs, "/music").unwrap();
    enrich(&mut store, "Stale");

    let mut warmed = Store::from_cache(store.to_cache()).unwrap();
    vfs.insert_at("/music/song.mp3", b"changed", FileTime::new(20, 1));
    warmed.reload(&vfs).unwrap();
    assert_eq!(warmed.pending_metadata_count(), 1);
    assert!(!warmed.tracks()[0].metadata_loaded);
    assert_ne!(warmed.tracks()[0].title, "Stale");
}

#[test]
fn version_root_and_corrupt_payloads_are_evicted() {
    let vfs = MemVfs::new();
    vfs.insert("/music/song.mp3", b"audio");
    let mut store = Store::open(&vfs, "/music").unwrap();
    enrich(&mut store, "Cached");
    store.save_cache(&vfs, "/cache/library.json").unwrap();
    assert!(Store::load_cache(&vfs, "/cache/library.json", "/music").is_some());

    vfs.write_atomic(
        "/cache/version.json",
        br#"{"version":2,"root":"/music","tracks":[]}"#,
    )
    .unwrap();
    assert!(Store::load_cache(&vfs, "/cache/version.json", "/music").is_none());
    assert!(!vfs.exists("/cache/version.json"));

    vfs.write_atomic("/cache/root.json", &store.to_cache().to_bytes().unwrap())
        .unwrap();
    assert!(Store::load_cache(&vfs, "/cache/root.json", "/other").is_none());
    assert!(!vfs.exists("/cache/root.json"));

    vfs.write_atomic("/cache/corrupt.json", b"{not-json")
        .unwrap();
    assert!(Store::load_cache(&vfs, "/cache/corrupt.json", "/music").is_none());
    assert!(!vfs.exists("/cache/corrupt.json"));
    assert!(parse_library_snapshot(b"{not-json").is_none());
}

#[test]
fn cache_paths_do_not_collide_across_roots() {
    assert_ne!(
        library_cache_path("/tmp/cache", "/music/A"),
        library_cache_path("/tmp/cache", "/music/B")
    );
    assert_eq!(
        library_cache_path("/tmp/cache/", "/music"),
        library_cache_path("/tmp/cache", "/music/")
    );
}

#[test]
fn hydrate_retains_query_and_real_files_round_trip() {
    let vfs = MemVfs::new();
    vfs.insert("/music/alpha.mp3", b"a");
    vfs.insert("/music/beta.mp3", b"b");
    let mut store = Store::open(&vfs, "/music").unwrap();
    let first = store.tracks()[0].id.clone();
    store
        .apply_metadata(MetadataUpdate {
            id: first,
            title: "Alpha".into(),
            artist: "Zed".into(),
            album: "One".into(),
            duration_ms: 1_000,
            has_artwork: false,
            has_lyrics: true,
        })
        .unwrap();
    store.set_library_view("alp".into(), music_core::SortOption::Title);

    let snapshot = store.to_cache();
    store.hydrate(snapshot.clone()).unwrap();
    assert_eq!(store.pending_metadata_count(), 1);
    assert_eq!(
        store.library_content_state(),
        music_core::LibraryContentState::Content
    );

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("library.json");
    let path_str = path.to_str().unwrap();
    let disk = StdVfs::new(music_core::TEMP_PREFIX);
    store.save_cache(&disk, path_str).unwrap();
    let loaded = Store::load_cache(&disk, path_str, "/music").unwrap();
    assert_eq!(loaded.tracks().len(), 2);
    assert_eq!(
        loaded
            .tracks()
            .iter()
            .find(|track| track.title == "Alpha")
            .map(|track| track.has_lyrics),
        Some(true)
    );

    let foreign = LibrarySnapshot {
        version: LIBRARY_CACHE_VERSION,
        root: "/music".into(),
        tracks: snapshot.tracks,
    };
    assert!(foreign.is_usable("/music"));
    assert!(!foreign.is_usable("/elsewhere"));
}
