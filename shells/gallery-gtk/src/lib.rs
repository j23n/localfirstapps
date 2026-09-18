//! LocalGallery GTK shell. Host paths and `--comet` live here so tests
//! need no display. Photo windows come from `gallery-ffi`; leftover
//! `localgallery` is host-only (`default-features = false`). Phase 5.7
//! promotions: kit `media_item` / `chip_bar`, Newsreader display face.
//! Phase 5.9: Photos-tab year rail is shell chrome, not a kit kind.

#![forbid(unsafe_code)]

use std::path::PathBuf;

use shell_kit_gtk::{BindingId, GalleryScreen};

mod folders;
mod paging;
mod routing;
mod session;
mod thumbs;
mod window;
mod year_rail;

pub use gallery_ffi::{LibraryIndex, ViewContentState, ViewStructure};
pub use localgallery::Config;
pub use paging::{years_from_structure, PageCache, ViewItem, ViewList, ViewListModel, YearMark};
pub use routing::{gtk_route, ROUTED_SCREENS};
pub use session::{
    EventFolder, MemoriesCache, PersonMarks, PhotoHost, PhotoIntent, PhotoPage, PreparedCatalog,
    PreparedCollection, PreparedHub, PreparedUi, ScanStatus, Session, ShellError, TextPage,
};
pub use shell_kit_gtk::{LogEntry, LogLevel, LogStore};
pub use window::Window;

/// Desktop file / libadwaita application id.
pub const APP_ID: &str = "com.j23n.LocalGallery";
/// Window title. Same product name as iOS.
pub const APP_TITLE: &str = "LocalGallery";
/// Width at or below which the GTK shell uses Comet chrome (bottom nav).
pub const COMPACT_WIDTH: i32 = 550;
/// Person card edge on the Collections hub (iOS `PersonCard` is 128).
pub const PERSON_TILE_PX: i32 = 128;
/// Gutter between person cards in the two-row hub preview.
pub const PERSON_GAP_PX: i32 = 10;

/// Extra columns bound past a full fit and clipped by the rail. Two keeps a
/// cut-off tile visible even when leftover width is almost another card.
pub const HUB_RAIL_PEEK_COLUMNS: usize = 2;

/// Columns that fully fit in a two-row Collections preview.
pub fn people_hub_columns(content_width: i32) -> usize {
    let stride = PERSON_TILE_PX + PERSON_GAP_PX;
    (content_width.max(stride) / stride).max(2) as usize
}

/// How many people to bind in the two-row Collections preview.
///
/// Includes [`HUB_RAIL_PEEK_COLUMNS`] past the last full column so resize
/// reveals more instead of empty track. Further people stay behind **See all**.
pub fn people_hub_preview_limit(content_width: i32) -> usize {
    people_hub_columns(content_width)
        .saturating_add(HUB_RAIL_PEEK_COLUMNS)
        .saturating_mul(2)
}

/// Event card edge on the Collections hub rail.
pub const EVENT_TILE_PX: i32 = 160;
/// Gutter between event cards in the hub rail.
pub const EVENT_GAP_PX: i32 = 12;

/// How many event tiles to bind on the hub rail before **See all**.
///
/// Includes [`HUB_RAIL_PEEK_COLUMNS`] past the last full tile so the clipped
/// edge stays obviously incomplete while resizing.
pub fn events_hub_preview_limit(content_width: i32) -> usize {
    let stride = EVENT_TILE_PX + EVENT_GAP_PX;
    let fit = (content_width.max(stride) / stride).max(4) as usize;
    fit.saturating_add(HUB_RAIL_PEEK_COLUMNS)
}

/// Public kit behavior exercised by the Gallery skeleton.
pub const KIT_BINDINGS: &[BindingId] = &[
    BindingId::AboutDialog,
    BindingId::ActionRow,
    BindingId::AdaptiveShell,
    BindingId::Diagnostics,
    BindingId::ChoiceDropdown,
    BindingId::ChipBar,
    BindingId::ChromeProgress,
    BindingId::EmptyState,
    BindingId::FieldRow,
    BindingId::ListScreen,
    BindingId::MediaItem,
    BindingId::NavRow,
    BindingId::NavigationView,
    BindingId::Page,
    BindingId::PreferencesDialog,
    BindingId::PrimaryAction,
    BindingId::PrimaryMenu,
    BindingId::ProgressRow,
    BindingId::SelectionBar,
    BindingId::ConfirmDialog,
    BindingId::SettingsScreen,
    BindingId::Sheet,
    BindingId::StatusRow,
    BindingId::TextRow,
    BindingId::TokenCss,
];

/// Flags the binary understands besides GTK's own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchArgs {
    /// Compact 540×620 default size.
    pub comet: bool,
    /// Generated [`GalleryScreen::as_str`] id.
    pub route: Option<String>,
    /// Debug PNG destination. Release builds ignore this.
    pub snapshot: Option<PathBuf>,
    /// Window default size (`--size WxH`).
    pub size: Option<(i32, i32)>,
    /// Photo folder that bypasses leftover `Config.library_root`.
    pub folder: Option<PathBuf>,
    /// Time open+bind, print `[gallery-gtk-perf]` metrics, quit.
    pub bench: bool,
}

/// True when `--comet` is among the process arguments.
#[must_use]
pub fn wants_comet<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().any(|arg| arg.as_ref() == "--comet")
}

/// Parse `540x620` / `1280x800` style sizes.
pub fn parse_size(text: &str) -> Result<(i32, i32), String> {
    let (width, height) = text
        .split_once('x')
        .or_else(|| text.split_once('X'))
        .ok_or_else(|| format!("size must be WxH, got {text}"))?;
    let width: i32 = width
        .parse()
        .map_err(|_| format!("invalid width in {text}"))?;
    let height: i32 = height
        .parse()
        .map_err(|_| format!("invalid height in {text}"))?;
    if width <= 0 || height <= 0 {
        return Err(format!("size must be positive, got {text}"));
    }
    Ok((width, height))
}

/// Resolve a generated Gallery screen id the GTK shell can open.
pub fn parse_gallery_route(id: &str) -> Result<GalleryScreen, String> {
    for screen in GalleryScreen::ALL {
        if screen.as_str() == id {
            return gtk_route(*screen).ok_or_else(|| format!("no GTK route for {id}"));
        }
    }
    Err(format!("unknown gallery route {id}"))
}

/// Parse `--comet`, `--route`, `--snapshot`, `--size`, `--folder`, and `--bench`.
pub fn parse_launch_args<I, S>(args: I) -> Result<LaunchArgs, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut launch = LaunchArgs::default();
    let mut iter = args.into_iter();
    let _argv0 = iter.next();
    while let Some(raw) = iter.next() {
        let arg = raw.as_ref();
        match arg {
            "--comet" => launch.comet = true,
            "--route" => {
                let value = required_value(&mut iter, "--route")?;
                parse_gallery_route(&value)?;
                launch.route = Some(value);
            }
            "--snapshot" => {
                launch.snapshot = Some(PathBuf::from(required_value(&mut iter, "--snapshot")?));
            }
            "--size" => {
                launch.size = Some(parse_size(&required_value(&mut iter, "--size")?)?);
            }
            "--folder" => {
                launch.folder = Some(PathBuf::from(required_value(&mut iter, "--folder")?));
            }
            "--bench" => launch.bench = true,
            other if other.starts_with('-') => {
                return Err(format!("unknown argument: {other}"));
            }
            _ => {}
        }
    }
    Ok(launch)
}

fn required_value<I, S>(iter: &mut I, flag: &str) -> Result<String, String>
where
    I: Iterator<Item = S>,
    S: AsRef<str>,
{
    iter.next()
        .map(|value| value.as_ref().to_string())
        .ok_or_else(|| format!("{flag} needs a value"))
}

/// Touch both path deps so a missing pin fails the shells workspace build.
pub fn pin_probe() -> String {
    let _ = Config::default();
    gallery_ffi::core_version()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gallery_ffi::ViewError;
    use std::path::Path;

    #[test]
    fn pin_probe_touches_both_path_deps() {
        assert!(!pin_probe().is_empty());
    }

    #[test]
    fn location_windows_compile_from_the_shells_lockfile() {
        let index = LibraryIndex::new();
        let folders = index.folder_structure(None);
        assert_eq!(folders.state, ViewContentState::Empty);
        assert_eq!(folders.sections[0].id, "folders");
        assert!(index
            .folder_window("folders".into(), 0, 8, folders.generation)
            .unwrap()
            .is_empty());

        let people = index.people_structure();
        assert_eq!(people.sections[0].id, "people");
        assert!(index
            .people_window("people".into(), 0, 8, people.generation)
            .unwrap()
            .is_empty());

        let collections = index.collection_structure();
        assert!(collections.sections.is_empty());
        assert!(matches!(
            index.collection_window("places".into(), 0, 8, collections.generation),
            Err(ViewError::SectionNotFound { .. })
        ));
    }

    #[test]
    fn comet_flag_is_opt_in() {
        assert!(!wants_comet(["localgallery"]));
        assert!(wants_comet(["localgallery", "--comet"]));
    }

    #[test]
    fn launch_args_parse_route_size_and_folder() {
        let launch = parse_launch_args([
            "localgallery",
            "--route",
            "photos",
            "--size",
            "540x620",
            "--folder",
            "/tmp/photos",
            "--snapshot",
            "/tmp/out.png",
        ])
        .unwrap();
        assert_eq!(launch.route.as_deref(), Some("photos"));
        assert_eq!(launch.size, Some((540, 620)));
        assert_eq!(launch.folder.as_deref(), Some(Path::new("/tmp/photos")));
        assert_eq!(launch.snapshot.as_deref(), Some(Path::new("/tmp/out.png")));
        assert!(!launch.bench);
        assert!(
            parse_launch_args(["localgallery", "--bench", "--folder", "/tmp/photos"])
                .unwrap()
                .bench
        );
        assert_eq!(parse_size("1280x800").unwrap(), (1280, 800));
        assert!(parse_size("wide").is_err());
        assert!(parse_gallery_route("folder-picker").is_ok());
        assert!(parse_gallery_route("logs").is_ok());
        assert!(parse_gallery_route("viewer").is_ok());
        assert!(parse_gallery_route("photo-info").is_ok());
        assert!(parse_gallery_route("folder").is_ok());
        assert!(parse_gallery_route("people").is_ok());
        assert!(parse_gallery_route("face-review").is_err());
        assert!(parse_gallery_route("sync-conflict-group").is_ok());
        assert!(parse_gallery_route("not-a-screen").is_err());
        assert!(parse_launch_args(["localgallery", "--route"]).is_err());
        assert!(
            parse_launch_args(["localgallery", "--comet"])
                .unwrap()
                .comet
        );
    }

    #[test]
    fn generated_css_has_authored_gallery_accent() {
        let css = include_str!("../../../design/tokens/generated/gallery.css");
        assert!(css.contains("#C48A3E"));
        assert!(css.contains("prefers-color-scheme: dark"));
        assert!(css.contains("--grid-gutter: 2px"));
        assert!(css.contains("--thumb-radius: 8px"));
        assert!(!css.contains("Newsreader"));
        assert!(!css.contains("font-family"));
    }

    #[test]
    fn gallery_publishes_promoted_bindings() {
        assert!(KIT_BINDINGS.contains(&BindingId::MediaItem));
        assert!(KIT_BINDINGS.contains(&BindingId::ChipBar));
        assert!(KIT_BINDINGS.contains(&BindingId::SelectionBar));
        assert!(KIT_BINDINGS.contains(&BindingId::ConfirmDialog));
    }

    #[test]
    fn display_font_file_is_in_the_repo() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../design/fonts")
            .join(localcore_ui::gallery::DISPLAY_FILE);
        assert!(path.is_file(), "{}", path.display());
    }

    #[test]
    fn people_hub_preview_is_two_rows_and_clips_to_width() {
        assert_eq!(people_hub_preview_limit(280), 8);
        assert_eq!(people_hub_preview_limit(516), 10);
        assert_eq!(people_hub_preview_limit(1256), 22);
        assert_eq!(people_hub_preview_limit(2000), 32);
        assert_eq!(people_hub_preview_limit(4000), 60);
        assert_eq!(people_hub_preview_limit(0), 8);
        assert_eq!(
            people_hub_preview_limit(516) - people_hub_columns(516) * 2,
            HUB_RAIL_PEEK_COLUMNS * 2
        );
    }

    #[test]
    fn events_hub_preview_clips_to_width() {
        assert_eq!(events_hub_preview_limit(280), 6);
        assert_eq!(events_hub_preview_limit(516), 6);
        assert_eq!(events_hub_preview_limit(1256), 9);
        assert_eq!(events_hub_preview_limit(0), 6);
    }
}
