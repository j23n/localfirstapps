use music_core::{
    classify_name, conflict_rows, FileClass, MetadataUpdate, PlaylistFormat, SortOption, Store,
    StoreError,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct IdVector {
    label: String,
    input: String,
    uuid: String,
}

#[test]
fn extension_classification_matches_the_existing_swift_tables() {
    for extension in [
        "mp3", "M4A", "aac", "wav", "aiff", "aif", "flac", "caf", "opus",
    ] {
        assert_eq!(
            classify_name(&format!("track.{extension}")),
            Some(FileClass::Audio)
        );
    }
    assert_eq!(
        classify_name("set.M3U8"),
        Some(FileClass::Playlist(PlaylistFormat::M3u8))
    );
    assert_eq!(
        classify_name("set.pls"),
        Some(FileClass::Playlist(PlaylistFormat::Pls))
    );
    assert_eq!(classify_name("cover.jpg"), None);
}

#[test]
fn stable_ids_match_swift_vectors_and_nfc_spellings() {
    let vectors: Vec<IdVector> =
        serde_json::from_str(include_str!("../fixtures/identity/stable_id_vectors.json")).unwrap();
    for vector in vectors {
        assert_eq!(
            localcore_id::derive(&vector.input).to_string(),
            vector.uuid,
            "{}",
            vector.label
        );
    }
}

#[test]
fn lexical_standardization_matches_swift_track_identity_input() {
    let direct = music_core::path::standardize("/Music/Album/song.mp3");
    let dotted = music_core::path::standardize("/Music/Album/./song.mp3");
    assert_eq!(direct, dotted);
    assert_eq!(localcore_id::derive(&direct), localcore_id::derive(&dotted));
}

#[test]
fn walk_projects_audio_and_playlists_but_never_conflict_copies() {
    let vfs = music_core::MemVfs::new();
    vfs.insert("/music/song.mp3", b"audio");
    vfs.insert("/music/Nested/other.FLAC", b"audio");
    vfs.insert("/music/mix.m3u", b"#EXTM3U\nsong.mp3\n");
    vfs.insert(
        "/music/mix.sync-conflict-20200901-120000-PHONE01.m3u",
        b"#EXTM3U\nother.mp3\n",
    );
    vfs.insert(
        "/music/song.sync-conflict-20200901-120000-PHONE01.mp3",
        b"other audio",
    );
    let store = Store::open(&vfs, "/music").unwrap();
    assert_eq!(store.tracks().len(), 2);
    assert_eq!(store.playlists().len(), 1);
    assert_eq!(store.conflict_groups().len(), 2);
    assert!(store
        .tracks()
        .iter()
        .all(|track| !track.path.contains("sync-conflict")));
    assert!(store.playlists()[0].entries[0].track_id.is_some());

    let rows = conflict_rows(&vfs, &store).unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|row| row.trailing == "manual file choice"));
}

#[test]
fn metadata_drives_core_search_sort_sections_and_display_copy() {
    let vfs = music_core::MemVfs::new();
    vfs.insert("/music/a.mp3", b"a");
    vfs.insert("/music/b.mp3", b"b");
    let mut store = Store::open(&vfs, "/music").unwrap();
    let first = store.tracks()[0].id.clone();
    let second = store.tracks()[1].id.clone();
    store
        .apply_metadata(MetadataUpdate {
            id: first,
            title: "Café".into(),
            artist: "Zed".into(),
            album: "One".into(),
            duration_ms: 61_000,
            has_artwork: true,
            has_lyrics: false,
        })
        .unwrap();
    store
        .apply_metadata(MetadataUpdate {
            id: second,
            title: "Alpha".into(),
            artist: "Ada".into(),
            album: "Two".into(),
            duration_ms: 30_000,
            has_artwork: false,
            has_lyrics: true,
        })
        .unwrap();

    let generation = store.set_library_view("Cafe\u{301}".into(), SortOption::Title);
    let sections = store.library_section_rows();
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].title, "C");
    let rows = store
        .library_track_rows(&sections[0].id, 0, 50, generation)
        .unwrap();
    assert_eq!(rows[0].label.as_deref(), Some("Café"));
    assert!(rows[0].badge.as_deref().unwrap().contains("1:01"));
    assert!(rows[0].thumbnail_ref.starts_with("artwork:"));

    store.set_library_view(String::new(), SortOption::Duration);
    assert!(matches!(
        store.library_track_rows(&sections[0].id, 0, 50, generation),
        Err(StoreError::StaleGeneration { .. })
    ));
    assert_eq!(
        store
            .library_section_rows()
            .iter()
            .map(|row| row.title.as_str())
            .collect::<Vec<_>>(),
        ["Under 1 min", "1–3 min"]
    );
}
