#!/usr/bin/env python3
"""Static guard for the iOS MusicSession shell boundary."""

from pathlib import Path
import sys


APP = Path(__file__).resolve().parents[1] / "LocalMusic"


def require(path: str, *tokens: str) -> list[str]:
    body = (APP / path).read_text(encoding="utf-8")
    return [
        f"{path}: missing {token}"
        for token in tokens
        if token not in body
    ]


def forbid(path: str, *tokens: str) -> list[str]:
    body = (APP / path).read_text(encoding="utf-8")
    return [
        f"{path}: shell-owned authority returned: {token}"
        for token in tokens
        if token in body
    ]


def main() -> int:
    errors: list[str] = []
    errors += require(
        "Services/MusicCoreClient.swift",
        "MusicSession.open(",
        "librarySectionRows()",
        "libraryTrackRows(",
        "playlistEntryRows(",
        "playlistEntryMediaSources(",
        "AddTracksCommand(",
        "RemovePlaylistEntriesCommand(",
        "MovePlaylistEntryCommand(",
        "ResolveConflictCommand(",
    )
    errors += require(
        "Services/MetadataLoader.swift",
        "AVURLAsset",
        "MetadataResult(",
        "identityMismatch",
    )
    errors += forbid(
        "Services/MetadataLoader.swift",
        "contentsOfDirectory(",
        "enumerator(at:",
        "scanPlaylists",
        "parseM3U",
        "parsePLS",
        "writePlaylist",
        "buildM3U",
        "buildPLS",
    )
    errors += forbid(
        "Services/LibraryStore.swift",
        "saveLibrary",
        "loadLibrary",
        "folderContentModificationDate",
        "localizedCaseInsensitiveCompare",
    )
    errors += require(
        "Services/PersistenceManager.swift",
        "migrateLegacyLibraryIfNeeded",
        "obsolete library.json",
    )

    routes = {
        "Views/LibraryView.swift": (
            "MusicScreen.folderPicker.rawValue",
            "MusicScreen.library.rawValue",
        ),
        "Views/PlaylistsView.swift": ("MusicScreen.playlistList.rawValue",),
        "Views/PlaylistDetailView.swift": (
            "MusicScreen.playlistDetail.rawValue",
            "MusicScreen.addTracks.rawValue",
        ),
        "Views/NowPlayingView.swift": ("MusicScreen.nowPlaying.rawValue",),
        "Views/SettingsView.swift": ("MusicScreen.settings.rawValue",),
        "Views/LogsView.swift": ("MusicScreen.logs.rawValue",),
        "Views/MusicSyncConflictSheet.swift": (
            "MusicScreen.syncConflictGroup.rawValue",
        ),
    }
    for path, tokens in routes.items():
        errors += require(path, *tokens)

    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("iOS MusicSession boundary: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
