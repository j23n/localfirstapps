//! GTK routes backed by generated Music screen identifiers.

use shell_kit_gtk::MusicScreen;

pub const ROUTED_SCREENS: &[MusicScreen] = &[
    MusicScreen::FolderPicker,
    MusicScreen::Library,
    MusicScreen::PlaylistList,
    MusicScreen::PlaylistDetail,
    MusicScreen::AddTracks,
    MusicScreen::NowPlaying,
    MusicScreen::Settings,
    MusicScreen::Logs,
    MusicScreen::SyncConflictGroup,
];

/// Exhaustive matching makes a newly generated screen a compile failure.
#[must_use]
pub const fn gtk_route(screen: MusicScreen) -> MusicScreen {
    match screen {
        MusicScreen::FolderPicker
        | MusicScreen::Library
        | MusicScreen::PlaylistList
        | MusicScreen::PlaylistDetail
        | MusicScreen::AddTracks
        | MusicScreen::NowPlaying
        | MusicScreen::Settings
        | MusicScreen::Logs
        | MusicScreen::SyncConflictGroup => screen,
    }
}

#[must_use]
pub(crate) fn route_id(screen: MusicScreen) -> &'static str {
    gtk_route(screen).as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_generated_music_screen_is_routed() {
        let routed = MusicScreen::ALL
            .iter()
            .copied()
            .map(gtk_route)
            .collect::<Vec<_>>();
        assert_eq!(routed, ROUTED_SCREENS);
    }
}
