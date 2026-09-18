use music_core::{ConflictDisposition, MemVfs, MetadataUpdate, SortOption};
use music_gtk::{MockTransport, Session, TransportCommand};

#[test]
fn folder_to_list_to_playlist_save_to_conflict_choice_is_headless() {
    let vfs = MemVfs::new();
    vfs.insert("/music/song.mp3", b"audio");
    vfs.insert("/music/song-two.mp3", b"audio");
    vfs.insert(
        "/music/mix.m3u",
        include_bytes!("../../../core/music-core/fixtures/r8/same-field/mix.m3u").as_slice(),
    );
    vfs.insert(
        "/music/mix.sync-conflict-20200901-120000-LAPTOP02.m3u",
        include_bytes!(
            "../../../core/music-core/fixtures/r8/same-field/mix.sync-conflict-20200901-120000-LAPTOP02.m3u"
        )
        .as_slice(),
    );
    let mut session = Session::new(vfs, "linux-test".into(), MockTransport::default(), 32);

    session.open_folder("/music").unwrap();
    let library = session
        .library_rows(String::new(), SortOption::Title)
        .unwrap();
    let track_id = library.sections[0].items[0].id.clone();
    let second_track_id = library.sections[0].items[1].id.clone();
    assert_eq!(library.sections[0].items[0].label.as_deref(), Some("song"));

    let playlist_id = session.create_playlist("Road".into()).unwrap();
    let detail = session.playlist_detail(&playlist_id).unwrap();
    let token = session
        .add_tracks(
            playlist_id.clone(),
            detail.content_token,
            vec![track_id.clone(), second_track_id],
        )
        .unwrap();
    let entries = session.playlist_detail(&playlist_id).unwrap().entries;
    session
        .move_entry(
            playlist_id.clone(),
            token,
            entries[1].id.clone(),
            Some(entries[0].id.clone()),
        )
        .unwrap();
    assert_eq!(
        session.playlist_detail(&playlist_id).unwrap().entries.len(),
        2
    );
    assert_eq!(
        session.playlist_detail(&playlist_id).unwrap().entries[0].title,
        "song-two"
    );

    let conflict = session.conflict_rows().unwrap().remove(0);
    assert_eq!(conflict.disposition, ConflictDisposition::Choice);
    let source = session
        .conflict_choice_rows(&conflict.id)
        .unwrap()
        .remove(0)
        .id;
    session.resolve_conflict(conflict.id, Some(source)).unwrap();
    assert!(session.conflict_rows().unwrap().is_empty());

    let picker = session.picker_track_items("song").unwrap();
    assert!(picker.iter().any(|item| item.id == track_id));
    assert!(
        session
            .picker_track_items("no-such-title")
            .unwrap()
            .is_empty(),
        "picker search must not mutate the live library"
    );
    let after_picker = session
        .library_rows(String::new(), SortOption::Title)
        .unwrap();
    assert_eq!(after_picker.sections[0].items.len(), 2);

    session.play_track(&track_id).unwrap();
    assert!(matches!(
        session.transport().commands(),
        [
            TransportCommand::Stop,
            TransportCommand::Load { .. },
            TransportCommand::Play
        ]
    ));
}

#[test]
fn warm_path_installs_cached_store_before_a_walk() {
    let vfs = MemVfs::new();
    vfs.insert("/music/song.mp3", b"audio");
    let mut cold = Session::new(vfs, "linux-test".into(), MockTransport::default(), 32);
    cold.open_folder("/music").unwrap();
    let request = cold.pending_metadata(1).unwrap().remove(0);
    cold.apply_metadata_batch(vec![MetadataUpdate {
        id: request.id,
        title: "Warm Title".into(),
        artist: "Ada".into(),
        album: "Notes".into(),
        duration_ms: 61_000,
        has_artwork: true,
        has_lyrics: false,
    }])
    .unwrap();
    let snapshot = cold.clone_store().unwrap().to_cache();

    let empty = MemVfs::new();
    let mut warm = Session::new(empty, "linux-test".into(), MockTransport::default(), 32);
    warm.install_from_snapshot("/music", snapshot).unwrap();
    let library = warm.library_rows(String::new(), SortOption::Title).unwrap();
    assert_eq!(
        library.sections[0].items[0].label.as_deref(),
        Some("Warm Title")
    );
    assert!(library.sections[0].items[0]
        .badge
        .as_deref()
        .unwrap()
        .contains("1:01"));
    assert_eq!(warm.pending_metadata_count(), 0);
    assert!(!warm.try_install_cache("/music", "/no/such/music-warm-cache.json"));
}
