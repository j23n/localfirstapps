//! LocalMusic GTK shell. Rendering, GStreamer, MPRIS, host paths, and
//! `--comet` stay here; playlist authority remains in `music-core`.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use music_core::valid_device;
use shell_kit_gtk::BindingId;

mod mpris;
mod routing;
mod session;
mod transport;
mod window;

pub use mpris::{MprisHost, RemoteCommand};
pub use routing::{gtk_route, ROUTED_SCREENS};
pub use session::{LibraryRows, LibrarySection, PlaylistDetailRows, Session, ShellError};
pub use transport::{
    system_transport, MockTransport, PlaybackStatus, TransportCommand, TransportError,
    TransportPort, TransportSnapshot,
};
pub use window::Window;

pub const APP_ID: &str = "com.localmusic.app";
pub const APP_TITLE: &str = "LocalMusic";
pub const CONFIG_DIR_NAME: &str = "localmusic";
pub const COMPACT_WIDTH: i32 = 550;

/// Public kit behavior exercised by the Music shell.
pub const KIT_BINDINGS: &[BindingId] = &[
    BindingId::ActionRow,
    BindingId::Banner,
    BindingId::ChoiceDropdown,
    BindingId::ConfirmDialog,
    BindingId::Diagnostics,
    BindingId::FieldRow,
    BindingId::ListPage,
    BindingId::MediaItem,
    BindingId::NavRow,
    BindingId::NavigationView,
    BindingId::PrimaryAction,
    BindingId::PushPage,
    BindingId::SearchEntry,
    BindingId::SettingsPage,
    BindingId::Sheet,
    BindingId::StatusRow,
    BindingId::TextRow,
    BindingId::TokenCss,
];

#[must_use]
pub fn wants_comet<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .any(|argument| argument.as_ref() == "--comet")
}

#[must_use]
pub fn sanitize_device_suffix(raw: &str) -> String {
    let mut output = String::new();
    for character in raw.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
            output.push(character);
        } else if !output.ends_with('-') {
            output.push('-');
        }
    }
    let output = output.trim_matches('-').to_owned();
    if output.is_empty() {
        "host".into()
    } else {
        output
    }
}

#[must_use]
pub fn default_device_id(hostname: &str) -> String {
    format!("linux-{}", sanitize_device_suffix(hostname))
}

#[must_use]
pub fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .and_then(|text| text.lines().next().map(str::trim).map(ToOwned::to_owned))
        .filter(|text| !text.is_empty())
        .or_else(|| {
            std::env::var("HOSTNAME")
                .ok()
                .filter(|text| !text.is_empty())
        })
        .unwrap_or_else(|| "host".into())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config_dir: PathBuf,
}

impl Paths {
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            config_dir: xdg_config_home().join(CONFIG_DIR_NAME),
        }
    }

    fn ensure(&self) {
        let _ = std::fs::create_dir_all(&self.config_dir);
    }

    pub fn load_or_create_device_id(&self, hostname: &str) -> String {
        self.ensure();
        let path = self.config_dir.join("device-id");
        if let Ok(text) = std::fs::read_to_string(&path) {
            let stored = text.trim();
            if valid_device(stored) {
                return stored.to_owned();
            }
        }
        let id = default_device_id(hostname);
        let _ = std::fs::write(path, format!("{id}\n"));
        id
    }

    #[must_use]
    pub fn load_folder(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.config_dir.join("folder")).ok()?;
        let folder = text.trim();
        (!folder.is_empty()).then(|| folder.to_owned())
    }

    pub fn save_folder(&self, folder: &str) {
        self.ensure();
        let _ = std::fs::write(self.config_dir.join("folder"), format!("{folder}\n"));
    }
}

fn xdg_config_home() -> PathBuf {
    if let Ok(directory) = std::env::var("XDG_CONFIG_HOME") {
        if !directory.is_empty() {
            return PathBuf::from(directory);
        }
    }
    std::env::var_os("HOME")
        .map(|home| Path::new(&home).join(".config"))
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use shell_kit_gtk::measure_reuse;

    #[test]
    fn comet_flag_is_only_an_initial_size_hint() {
        assert!(!wants_comet(["localmusic"]));
        assert!(wants_comet(["localmusic", "--comet"]));
    }

    #[test]
    fn device_and_folder_settings_are_per_host() {
        let directory = std::env::temp_dir().join(format!(
            "music-gtk-paths-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = Paths {
            config_dir: directory.clone(),
        };
        assert_eq!(
            paths.load_or_create_device_id("Music Box"),
            "linux-Music-Box"
        );
        paths.save_folder("/tmp/music");
        assert_eq!(paths.load_folder().as_deref(), Some("/tmp/music"));
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn music_is_a_measured_second_consumer_of_the_gtk_kit() {
        let metrics = measure_reuse(contacts_gtk::KIT_BINDINGS, KIT_BINDINGS);
        assert_eq!(metrics.first_unique, 17);
        assert_eq!(metrics.second_unique, 18);
        assert_eq!(metrics.shared, 17);
    }
}
