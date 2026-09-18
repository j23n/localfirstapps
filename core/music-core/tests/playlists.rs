use music_core::{
    add_tracks_logged, canonical_bytes, create_playlist_logged, delete_playlist_logged,
    move_entry_logged, parse_playlist, playlist_entry_rows, read_ops, remove_entries_logged,
    settings_info_rows, write_playlist, AddTracksCommand, CreatePlaylistCommand,
    DeletePlaylistCommand, MovePlaylistEntryCommand, PlaylistFormat, RemovePlaylistEntriesCommand,
    Store, StoreError, Vfs,
};

#[test]
fn relative_playlist_entries_cannot_escape_the_playlist_folder() {
    let escaped = parse_playlist("/music/mix.m3u", b"#EXTM3U\n../secret.mp3\n").unwrap();
    assert_eq!(escaped.entries[0].resolved_path, None);
    let nested = parse_playlist("/music/mix.m3u", b"#EXTM3U\nNested/song.mp3\n").unwrap();
    assert_eq!(
        nested.entries[0].resolved_path.as_deref(),
        Some("/music/Nested/song.mp3")
    );
    let absolute = parse_playlist("/music/Nested/mix.m3u", b"#EXTM3U\n/music/song.mp3\n").unwrap();
    assert_eq!(
        absolute.entries[0].resolved_path.as_deref(),
        Some("/music/song.mp3")
    );
}

#[test]
fn m3u_parse_retains_missing_remote_and_foreign_directives() {
    let bytes = b"\xef\xbb\xbf#EXTM3U\r\n#EXTINF:12,Foreign title\r\nsong.mp3\r\nhttps://example.test/live.mp3\r\nmissing.flac\r\n";
    let playlist = parse_playlist("/music/mix.m3u8", bytes).unwrap();
    assert_eq!(playlist.format, PlaylistFormat::M3u8);
    assert_eq!(playlist.entries.len(), 3);
    assert_eq!(
        playlist.entries[0].resolved_path.as_deref(),
        Some("/music/song.mp3")
    );
    assert_eq!(playlist.entries[1].resolved_path, None);
    assert_eq!(
        playlist.entries[2].resolved_path.as_deref(),
        Some("/music/missing.flac")
    );
    assert_eq!(playlist.entries[0].directives, ["#EXTINF:12,Foreign title"]);
    let canonical = String::from_utf8(canonical_bytes(&playlist)).unwrap();
    assert!(canonical.contains("#EXTINF:12,Foreign title\n"));
    assert!(canonical.contains("https://example.test/live.mp3\n"));
}

#[test]
fn m3u_legacy_latin1_and_m3u8_strict_utf8_are_distinct() {
    let legacy = parse_playlist("/music/mix.m3u", b"#EXTM3U\nCaf\xe9.mp3\n").unwrap();
    assert_eq!(legacy.entries[0].raw_path, "Café.mp3");
    assert!(matches!(
        parse_playlist("/music/mix.m3u8", b"#EXTM3U\nCaf\xe9.mp3\n"),
        Err(StoreError::InvalidPlaylist { .. })
    ));
}

#[test]
fn pls_parse_orders_file_numbers_and_canonicalizes_known_fields() {
    let bytes = b"[playlist]\nTitle2=Second\nCustom2024=opaque\nFile2=b.mp3\nFile1=a.mp3\nNumberOfEntries=2\nVersion=2\n";
    let mut playlist = parse_playlist("/music/set.pls", bytes).unwrap();
    assert_eq!(
        playlist
            .entries
            .iter()
            .map(|entry| entry.raw_path.as_str())
            .collect::<Vec<_>>(),
        ["a.mp3", "b.mp3"]
    );
    assert_eq!(
        playlist.entries[1].pls_fields,
        [("Title".into(), "Second".into())]
    );
    let canonical = String::from_utf8(canonical_bytes(&playlist)).unwrap();
    assert_eq!(
        canonical,
        "[playlist]\nFile1=a.mp3\nFile2=b.mp3\nTitle2=Second\nCustom2024=opaque\nNumberOfEntries=2\nVersion=2\n"
    );
    playlist.entries.swap(0, 1);
    assert_eq!(
        String::from_utf8(canonical_bytes(&playlist)).unwrap(),
        "[playlist]\nFile1=b.mp3\nTitle1=Second\nFile2=a.mp3\nCustom2024=opaque\nNumberOfEntries=2\nVersion=2\n"
    );
}

#[test]
fn canonical_playlist_write_uses_vfs_atomic_replace() {
    let vfs = music_core::MemVfs::new();
    vfs.insert("/music/mix.m3u", b"old");
    let mut playlist = parse_playlist("/music/mix.m3u", b"#EXTM3U\nsong.mp3\n").unwrap();
    write_playlist(&vfs, &mut playlist).unwrap();
    assert_eq!(
        vfs.read("/music/mix.m3u").unwrap(),
        canonical_bytes(&playlist)
    );
    assert!(!playlist.content_token.is_empty());
}

#[test]
fn typed_edits_preserve_unknown_and_unresolved_entries() {
    let vfs = music_core::MemVfs::new();
    vfs.insert("/music/a.mp3", b"a");
    vfs.insert("/music/b.mp3", b"b");
    vfs.insert(
        "/music/mix.m3u",
        b"#EXTM3U\n#EXT-X-FOREIGN:value\n#EXT-X-FOREIGN:value\nmissing.mp3\nhttps://example.test/live.mp3\na.mp3\n",
    );
    let mut store = Store::open(&vfs, "/music").unwrap();
    let playlist = store.playlists()[0].clone();
    let b_id = store
        .tracks()
        .iter()
        .find(|track| track.path.ends_with("/b.mp3"))
        .unwrap()
        .id
        .clone();
    let token = add_tracks_logged(
        &vfs,
        &mut store,
        "phone",
        AddTracksCommand {
            playlist_id: playlist.id.clone(),
            content_token: playlist.content_token,
            track_ids: vec![b_id],
        },
    )
    .unwrap();
    let written = String::from_utf8(vfs.read("/music/mix.m3u").unwrap()).unwrap();
    assert!(written.contains("#EXT-X-FOREIGN:value\n"));
    assert_eq!(written.matches("#EXT-X-FOREIGN:value\n").count(), 2);
    assert!(written.contains("missing.mp3\n"));
    assert!(written.contains("https://example.test/live.mp3\n"));
    assert!(written.ends_with("b.mp3\n"));

    let entries = store.playlists()[0].entries.clone();
    let missing_id = entries
        .iter()
        .find(|entry| entry.raw_path == "missing.mp3")
        .unwrap()
        .id
        .clone();
    let a_id = entries
        .iter()
        .find(|entry| entry.raw_path == "a.mp3")
        .unwrap()
        .id
        .clone();
    let moved_token = move_entry_logged(
        &vfs,
        &mut store,
        "phone",
        MovePlaylistEntryCommand {
            playlist_id: playlist.id.clone(),
            content_token: token,
            entry_id: a_id,
            before_entry_id: Some(missing_id.clone()),
        },
    )
    .unwrap();
    let moved_missing_id = store.playlists()[0]
        .entries
        .iter()
        .find(|entry| entry.raw_path == "missing.mp3")
        .unwrap()
        .id
        .clone();
    remove_entries_logged(
        &vfs,
        &mut store,
        "phone",
        RemovePlaylistEntriesCommand {
            playlist_id: playlist.id,
            content_token: moved_token,
            entry_ids: vec![moved_missing_id],
        },
    )
    .unwrap();
    assert!(playlist_entry_rows(&store, &store.playlists()[0].id)
        .unwrap()
        .iter()
        .all(|row| row.title != "missing"));
    assert!(String::from_utf8(vfs.read("/music/mix.m3u").unwrap())
        .unwrap()
        .contains("https://example.test/live.mp3"));
}

#[test]
fn stale_typed_edit_refuses_without_overwriting_external_bytes() {
    let vfs = music_core::MemVfs::new();
    vfs.insert("/music/a.mp3", b"a");
    vfs.insert("/music/mix.m3u", b"#EXTM3U\na.mp3\n");
    let mut store = Store::open(&vfs, "/music").unwrap();
    let playlist = store.playlists()[0].clone();
    let track_id = store.tracks()[0].id.clone();
    vfs.write_atomic("/music/mix.m3u", b"#EXTM3U\nexternal.mp3\n")
        .unwrap();
    let error = add_tracks_logged(
        &vfs,
        &mut store,
        "phone",
        AddTracksCommand {
            playlist_id: playlist.id,
            content_token: playlist.content_token,
            track_ids: vec![track_id],
        },
    )
    .unwrap_err();
    assert!(matches!(error, StoreError::StalePlaylist { .. }));
    assert_eq!(
        vfs.read("/music/mix.m3u").unwrap(),
        b"#EXTM3U\nexternal.mp3\n"
    );
}

#[test]
fn create_delete_and_folder_log_are_core_owned() {
    let vfs = music_core::MemVfs::new();
    vfs.insert("/music/.keep", b"");
    let mut store = Store::open(&vfs, "/music").unwrap();
    let id = create_playlist_logged(
        &vfs,
        &mut store,
        "phone",
        CreatePlaylistCommand {
            name: "../Mix/One".into(),
        },
    )
    .unwrap();
    let playlist = store.playlist(&id).unwrap().clone();
    assert_eq!(playlist.path, "/music/Mix-One.m3u");
    assert_eq!(read_ops(&vfs, "/music").unwrap().len(), 1);
    delete_playlist_logged(
        &vfs,
        &mut store,
        "phone",
        DeletePlaylistCommand {
            playlist_id: id,
            content_token: playlist.content_token,
        },
    )
    .unwrap();
    assert!(!vfs.try_exists("/music/Mix-One.m3u").unwrap());
    assert_eq!(read_ops(&vfs, "/music").unwrap().len(), 2);
}

#[test]
fn settings_rows_and_playlist_playback_sources_are_core_projected() {
    let vfs = music_core::MemVfs::new();
    vfs.insert("/music/a.mp3", b"a");
    vfs.insert("/music/missing.m3u", b"#EXTM3U\nmissing.mp3\na.mp3\n");
    let store = Store::open(&vfs, "/music").unwrap();
    let playlist_id = store.playlists()[0].id.clone();

    let info = settings_info_rows(&store);
    assert_eq!(info[0].trailing.as_deref(), Some("1 track"));
    assert_eq!(info[1].trailing.as_deref(), Some("1 playlist"));
    assert_eq!(info[2].trailing.as_deref(), Some("0 groups"));

    let sources = store.playlist_media_sources(&playlist_id).unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, store.tracks()[0].id);
    assert!(sources[0].path.ends_with("/a.mp3"));
}
