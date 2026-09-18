use std::fs;

use music_ffi::{
    AddTracksCommand, ConflictDisposition, CreatePlaylistCommand, DeletePlaylistCommand,
    LibraryContentState, MetadataResult, MusicError, MusicSession, SearchKind,
    SetLibraryViewCommand, SortOption,
};

#[test]
fn session_exposes_windowed_display_rows_and_media_host_ports() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("song.mp3"), b"audio").unwrap();
    let session = MusicSession::open(temp.path().to_str().unwrap().into(), "phone".into()).unwrap();
    assert_eq!(
        session.library_content_state().unwrap(),
        LibraryContentState::Content
    );
    let request = session.metadata_requests(0, 20).unwrap().remove(0);
    assert!(request.path.ends_with("/song.mp3"));
    let generation = session
        .apply_metadata(MetadataResult {
            id: request.id.clone(),
            title: "Café".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            duration_ms: 125_000,
            has_artwork: true,
            has_lyrics: false,
        })
        .unwrap();
    let sections = session.library_section_rows().unwrap();
    let row = session
        .library_track_rows(sections[0].id.clone(), 0, 20, generation)
        .unwrap()
        .remove(0);
    assert_eq!(row.label.as_deref(), Some("Café"));
    assert!(row.badge.as_deref().unwrap().contains("2:05"));
    assert!(row.thumbnail_ref.starts_with("artwork:"));
    assert_eq!(session.media_source(request.id).unwrap().path, request.path);
    assert!(session.metadata_requests(0, 20).unwrap().is_empty());
    session.reload().unwrap();
    assert!(
        session.metadata_requests(0, 20).unwrap().is_empty(),
        "reload must preserve enrichment for unchanged stable ids"
    );
    fs::write(temp.path().join("song.mp3"), b"retagged audio bytes").unwrap();
    session.reload().unwrap();
    assert_eq!(
        session.metadata_requests(0, 20).unwrap().len(),
        1,
        "a changed file fingerprint must request fresh host metadata"
    );

    let stale = generation;
    session
        .set_library_view(SetLibraryViewCommand {
            query: "Cafe\u{301}".into(),
            sort: SortOption::Title,
        })
        .unwrap();
    assert!(matches!(
        session.library_track_rows(sections[0].id.clone(), 0, 20, stale),
        Err(MusicError::StaleGeneration {
            user_actionable: true,
            ..
        })
    ));
}

#[test]
fn typed_playlist_commands_do_not_need_playlist_payloads() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("song.mp3"), b"audio").unwrap();
    let session = MusicSession::open(temp.path().to_str().unwrap().into(), "phone".into()).unwrap();
    let playlist_id = session
        .create_playlist(CreatePlaylistCommand { name: "Mix".into() })
        .unwrap();
    let token = session.playlist_content_token(playlist_id.clone()).unwrap();
    let track_id = session.metadata_requests(0, 1).unwrap()[0].id.clone();
    let replacement_token = session
        .add_tracks(AddTracksCommand {
            playlist_id: playlist_id.clone(),
            content_token: token,
            track_ids: vec![track_id.clone()],
        })
        .unwrap();
    let source = session
        .playlist_entry_media_sources(playlist_id.clone())
        .unwrap()
        .remove(0);
    assert_eq!(source.track_id, track_id);
    assert!(source.path.ends_with("/song.mp3"));
    assert_eq!(
        replacement_token,
        session.playlist_content_token(playlist_id.clone()).unwrap()
    );
    assert_eq!(
        session
            .playlist_entry_rows(playlist_id.clone())
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        session
            .playlist_action_rows(playlist_id.clone())
            .unwrap()
            .len(),
        3
    );
    session
        .delete_playlist(DeletePlaylistCommand {
            playlist_id,
            content_token: replacement_token,
        })
        .unwrap();
    assert!(session.playlist_rows().unwrap().is_empty());
}

#[test]
fn metadata_batch_enriches_one_window_and_rejects_unbounded_input() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("one.mp3"), b"audio").unwrap();
    fs::write(temp.path().join("two.mp3"), b"audio").unwrap();
    let session = MusicSession::open(temp.path().to_str().unwrap().into(), "phone".into()).unwrap();
    let requests = session.metadata_requests(0, 500).unwrap();
    let results = requests
        .iter()
        .map(|request| MetadataResult {
            id: request.id.clone(),
            title: format!("Tagged {}", request.id),
            artist: "Artist".into(),
            album: "Album".into(),
            duration_ms: 1_000,
            has_artwork: false,
            has_lyrics: false,
        })
        .collect();
    session.apply_metadata_batch(results).unwrap();
    assert!(session.metadata_requests(0, 500).unwrap().is_empty());

    let oversized = (0..501)
        .map(|index| MetadataResult {
            id: index.to_string(),
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            duration_ms: 0,
            has_artwork: false,
            has_lyrics: false,
        })
        .collect();
    assert!(matches!(
        session.apply_metadata_batch(oversized),
        Err(MusicError::InvalidCommand { .. })
    ));
}

#[test]
fn session_surfaces_and_resolves_disjoint_m3u_conflict() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join("mix.m3u"),
        b"#EXTM3U\nbase.mp3\nleft.mp3\n",
    )
    .unwrap();
    fs::write(
        temp.path()
            .join("mix.sync-conflict-20200901-120000-PHONE01.m3u"),
        b"#EXTM3U\nbase.mp3\nleft.mp3\nright.mp3\n",
    )
    .unwrap();
    let session = MusicSession::open(temp.path().to_str().unwrap().into(), "phone".into()).unwrap();
    let row = session.conflict_rows().unwrap().remove(0);
    assert_eq!(row.disposition, ConflictDisposition::Auto);
    session
        .resolve_conflict(music_ffi::ResolveConflictCommand {
            group_id: row.id,
            selected_source: None,
        })
        .unwrap();
    assert!(session.conflict_rows().unwrap().is_empty());
    assert!(fs::read_to_string(temp.path().join("mix.m3u"))
        .unwrap()
        .contains("right.mp3"));
}

#[test]
fn search_hits_and_settings_rows_are_session_projections() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("song.mp3"), b"audio").unwrap();
    let session = MusicSession::open(temp.path().to_str().unwrap().into(), "phone".into()).unwrap();
    let request = session.metadata_requests(0, 1).unwrap().remove(0);
    session
        .apply_metadata(MetadataResult {
            id: request.id,
            title: "Café".into(),
            artist: "Ada".into(),
            album: "Notes".into(),
            duration_ms: 1_000,
            has_artwork: false,
            has_lyrics: false,
        })
        .unwrap();
    session
        .create_playlist(CreatePlaylistCommand {
            name: "Ada Mix".into(),
        })
        .unwrap();

    let hits = session.search_hits("ada".into()).unwrap();
    assert!(hits
        .iter()
        .any(|hit| hit.kind == SearchKind::Artist && hit.title == "Ada"));
    assert!(hits
        .iter()
        .any(|hit| hit.kind == SearchKind::Playlist && hit.title == "Ada Mix"));
    assert_eq!(
        hits.iter()
            .find(|hit| hit.kind == SearchKind::Artist)
            .map(|hit| hit.symbol.as_str()),
        Some("system-users-symbolic")
    );

    let info = session.settings_info_rows().unwrap();
    assert_eq!(info[0].id, "tracks");
    assert_eq!(info[0].title, "Tracks");
    assert_eq!(info[0].trailing.as_deref(), Some("1 track"));
    assert_eq!(info[1].trailing.as_deref(), Some("1 playlist"));
    assert_eq!(info[2].trailing.as_deref(), Some("0 groups"));
    assert!(session.search_hits(String::new()).unwrap().is_empty());
}
