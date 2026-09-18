//! LocalContacts GTK shell. Host paths and `--comet` live here so tests
//! need no display. Display rows and logged actions come from
//! `contacts-core` (Milestone C).

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use contacts_core::valid_device;
use shell_kit_gtk::{BindingId, ContactsScreen};

mod routing;
mod window;

pub use routing::{gtk_route, ROUTED_SCREENS};
pub use shell_kit_gtk::{LogEntry, LogLevel, LogStore};
pub use window::Window;

/// Desktop file / libadwaita application id.
pub const APP_ID: &str = "com.j23n.LocalContacts";
/// Window title. Same product name as iOS.
pub const APP_TITLE: &str = "LocalContacts";
/// XDG application directory (ADR 0005 R5 exceptions).
pub const CONFIG_DIR_NAME: &str = "localcontacts";

/// Width at or below which the GTK shell uses Comet chrome (bottom nav).
pub const COMPACT_WIDTH: i32 = 550;

/// Public kit behavior exercised by the Contacts shell.
///
/// Music's second-consumer test intersects this inventory with its own.
pub const KIT_BINDINGS: &[BindingId] = &[
    BindingId::AboutDialog,
    BindingId::ActionRow,
    BindingId::Banner,
    BindingId::ChoiceDropdown,
    BindingId::ChipBar,
    BindingId::ChromeProgress,
    BindingId::ConfirmDialog,
    BindingId::Diagnostics,
    BindingId::EmptyState,
    BindingId::FieldRow,
    BindingId::FormSheet,
    BindingId::ListScreen,
    BindingId::NavRow,
    BindingId::Page,
    BindingId::PreferencesDialog,
    BindingId::PrimaryAction,
    BindingId::PrimaryMenu,
    BindingId::ProgressRow,
    BindingId::SearchBar,
    BindingId::SearchEntry,
    BindingId::SelectionBar,
    BindingId::SettingsPage,
    BindingId::SettingsScreen,
    BindingId::Sheet,
    BindingId::SplitListDetail,
    BindingId::StatusRow,
    BindingId::TextRow,
    BindingId::TokenCss,
];

/// Flags the binary understands besides GTK's own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchArgs {
    /// Compact 540×620 default size.
    pub comet: bool,
    /// Generated [`ContactsScreen::as_str`] id.
    pub route: Option<String>,
    /// Debug PNG destination. Release builds ignore this.
    pub snapshot: Option<PathBuf>,
    /// Window default size (`--size WxH`).
    pub size: Option<(i32, i32)>,
    /// Contacts folder that bypasses the persisted XDG path.
    pub folder: Option<PathBuf>,
}

/// True when `--comet` is among the process arguments.
#[must_use]
pub fn wants_comet<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().any(|a| a.as_ref() == "--comet")
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

/// Resolve a generated Contacts screen id the GTK shell can open.
pub fn parse_contacts_route(id: &str) -> Result<ContactsScreen, String> {
    for screen in ContactsScreen::ALL {
        if screen.as_str() == id {
            return gtk_route(*screen).ok_or_else(|| format!("no GTK route for {id}"));
        }
    }
    Err(format!("unknown contacts route {id}"))
}

/// Parse `--comet`, `--route`, `--snapshot`, `--size`, and `--folder`.
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
                parse_contacts_route(&value)?;
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

/// Keep `[A-Za-z0-9._-]`. Empty after strip becomes `host`.
#[must_use]
pub fn sanitize_device_suffix(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "host".into()
    } else {
        out
    }
}

/// `linux-<sanitized hostname>`. Always [`valid_device`].
#[must_use]
pub fn default_device_id(hostname: &str) -> String {
    format!("linux-{}", sanitize_device_suffix(hostname))
}

/// Best-effort host name for a first-run device id.
#[must_use]
pub fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .and_then(|s| s.lines().next().map(str::trim).map(ToOwned::to_owned))
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "host".into())
}

/// XDG config directory for this device's exceptions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// `{XDG_CONFIG_HOME}/localcontacts` (or an injected test dir).
    pub config_dir: PathBuf,
}

impl Paths {
    /// `$XDG_CONFIG_HOME/localcontacts` or `~/.config/localcontacts`.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            config_dir: xdg_config_home().join(CONFIG_DIR_NAME),
        }
    }

    fn ensure(&self) {
        let _ = std::fs::create_dir_all(&self.config_dir);
    }

    /// Read or create `device-id`. Invalid stored values are replaced.
    pub fn load_or_create_device_id(&self, hostname: &str) -> String {
        self.ensure();
        let path = self.config_dir.join("device-id");
        if let Ok(s) = std::fs::read_to_string(&path) {
            let trimmed = s.trim();
            if valid_device(trimmed) {
                return trimmed.to_string();
            }
        }
        let id = default_device_id(hostname);
        let _ = std::fs::write(&path, format!("{id}\n"));
        id
    }

    /// Last folder path, if the file is non-empty.
    #[must_use]
    pub fn load_folder(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.config_dir.join("folder")).ok()?;
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    /// Persist the last folder (per-device exception, ADR 0005 R5).
    pub fn save_folder(&self, folder: &str) {
        self.ensure();
        let _ = std::fs::write(self.config_dir.join("folder"), format!("{folder}\n"));
    }
}

fn xdg_config_home() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    std::env::var_os("HOME")
        .map(|home| Path::new(&home).join(".config"))
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use contacts_core::{
        assign_tag_logged, bulk_delete_logged, detail_rows, list_rows_filtered, load_edit_draft,
        new_edit_draft, read_ops, remove_tag_logged, rename_tag_logged, save_contact_logged,
        tag_rows, BirthdayDraft, LabeledAddressDraft, LabeledValueDraft, MemVfs,
        SaveContactCommand, Store, Vfs, TYPE_CONTACT_SAVED,
    };
    use shell_kit_gtk::ContactsScreen;

    #[test]
    fn comet_flag_is_opt_in() {
        assert!(!wants_comet(["localcontacts"]));
        assert!(wants_comet(["localcontacts", "--comet"]));
    }

    #[test]
    fn launch_args_parse_route_size_and_folder() {
        let launch = parse_launch_args([
            "localcontacts",
            "--route",
            "contact-list",
            "--size",
            "540x620",
            "--folder",
            "/tmp/contacts",
            "--snapshot",
            "/tmp/out.png",
        ])
        .unwrap();
        assert_eq!(launch.route.as_deref(), Some("contact-list"));
        assert_eq!(launch.size, Some((540, 620)));
        assert_eq!(launch.folder.as_deref(), Some(Path::new("/tmp/contacts")));
        assert_eq!(launch.snapshot.as_deref(), Some(Path::new("/tmp/out.png")));
        assert_eq!(parse_size("1280x800").unwrap(), (1280, 800));
        assert!(parse_size("wide").is_err());
        assert!(parse_contacts_route("sync-conflict-group").is_ok());
        assert!(parse_contacts_route("apple-conflict").is_err());
        assert!(parse_contacts_route("not-a-screen").is_err());
        assert!(parse_launch_args(["localcontacts", "--route"]).is_err());
        assert!(
            parse_launch_args(["localcontacts", "--comet"])
                .unwrap()
                .comet
        );
    }

    #[test]
    fn device_id_is_valid() {
        assert!(valid_device(&default_device_id("My-Host.local")));
        assert!(valid_device(&default_device_id("!!!")));
        assert_eq!(default_device_id("!!!"), "linux-host");
        assert_eq!(sanitize_device_suffix("a b/c"), "a-b-c");
    }

    #[test]
    fn paths_persist_device_and_folder() {
        let dir = std::env::temp_dir().join(format!(
            "lc-paths-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = Paths {
            config_dir: dir.clone(),
        };
        let first = paths.load_or_create_device_id("box");
        assert_eq!(first, "linux-box");
        assert_eq!(paths.load_or_create_device_id("other"), first);
        assert!(paths.load_folder().is_none());
        paths.save_folder("/tmp/contacts");
        assert_eq!(paths.load_folder().as_deref(), Some("/tmp/contacts"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_rows_and_save_log() {
        let vfs = MemVfs::new();
        let mut store = Store::open(&vfs, "/lib").unwrap();
        let mut draft = new_edit_draft();
        draft.given_name = "Ada".into();
        draft.family_name = "Lovelace".into();
        draft.organization = "Analytical".into();
        draft.phones.push(LabeledValueDraft {
            label: "mobile".into(),
            value: "555".into(),
        });
        draft.emails.push(LabeledValueDraft {
            label: "home".into(),
            value: "ada@example".into(),
        });
        draft.note = "note".into();
        let saved =
            save_contact_logged(&vfs, &mut store, "linux-test", SaveContactCommand { draft })
                .unwrap();
        let id = saved.id.unwrap();
        let rows = list_rows_filtered(&store, "ada", None);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].title, "Ada Lovelace");
        assert_eq!(rows[0].subtitle.as_deref(), Some("Analytical"));
        let fields = detail_rows(&store, &id).unwrap();
        assert_eq!(fields[0].label, "Name");
        let ops = read_ops(&vfs, "/lib").unwrap();
        assert_eq!(ops[0].event_type, TYPE_CONTACT_SAVED);
    }

    #[test]
    fn filtered_lists_tags_and_bulk_actions_use_typed_commands() {
        let vfs = MemVfs::new();
        let mut store = Store::open(&vfs, "/lib").unwrap();
        let mut ada = new_edit_draft();
        ada.given_name = "Ada".into();
        ada.categories.push("pioneers".into());
        let ada = save_contact_logged(
            &vfs,
            &mut store,
            "linux-test",
            SaveContactCommand { draft: ada },
        )
        .unwrap()
        .id
        .unwrap();
        let mut grace = new_edit_draft();
        grace.given_name = "Grace".into();
        let grace = save_contact_logged(
            &vfs,
            &mut store,
            "linux-test",
            SaveContactCommand { draft: grace },
        )
        .unwrap()
        .id
        .unwrap();

        assert_eq!(
            assign_tag_logged(
                &vfs,
                &mut store,
                "linux-test",
                "pioneers",
                &[ada.clone(), grace.clone()],
            )
            .unwrap(),
            1
        );
        assert_eq!(list_rows_filtered(&store, "", Some("pioneers")).len(), 2);
        assert_eq!(
            list_rows_filtered(&store, "ada", Some("pioneers"))[0].id,
            ada
        );
        assert_eq!(tag_rows(&store)[0].trailing.as_deref(), Some("2 contacts"));
        assert_eq!(
            rename_tag_logged(&vfs, &mut store, "linux-test", "pioneers", "computing",).unwrap(),
            2
        );
        assert_eq!(
            remove_tag_logged(&vfs, &mut store, "linux-test", "computing").unwrap(),
            2
        );
        assert_eq!(
            bulk_delete_logged(&vfs, &mut store, "linux-test", &[grace]).unwrap(),
            1
        );
        assert_eq!(list_rows_filtered(&store, "", None).len(), 1);
    }

    #[test]
    fn full_draft_save_preserves_values_the_editor_does_not_change() {
        let vfs = MemVfs::new();
        let mut store = Store::open(&vfs, "/lib").unwrap();
        let mut draft = new_edit_draft();
        draft.full_name = "Dr Ada M Lovelace".into();
        draft.family_name = "Lovelace".into();
        draft.given_name = "Ada".into();
        draft.middle_name = "M".into();
        draft.name_prefix = "Dr".into();
        draft.name_suffix = "Countess".into();
        draft.organization = "Analytical".into();
        draft.job_title = "Programmer".into();
        draft.nickname = "Enchantress".into();
        draft.urls = vec![LabeledValueDraft {
            label: "work".into(),
            value: "https://example.test".into(),
        }];
        draft.phones = vec![
            LabeledValueDraft {
                label: "home".into(),
                value: "111".into(),
            },
            LabeledValueDraft {
                label: "work".into(),
                value: "222".into(),
            },
        ];
        draft.emails = vec![LabeledValueDraft {
            label: "home".into(),
            value: "ada@example.test".into(),
        }];
        draft.addresses = vec![LabeledAddressDraft {
            label: "home".into(),
            street: "1 Computing Lane".into(),
            city: "London".into(),
            state: String::new(),
            postal_code: "N1".into(),
            country: "UK".into(),
        }];
        draft.birthday = Some(BirthdayDraft {
            year: None,
            month: 12,
            day: 10,
        });
        draft.note = "Original note".into();
        draft.categories = vec!["friends".into(), "pioneers".into()];
        draft.photo = Some(vec![0xff, 0xd8, 0xff, 0xd9]);

        let saved =
            save_contact_logged(&vfs, &mut store, "linux-test", SaveContactCommand { draft })
                .unwrap();
        let id = saved.id.unwrap();
        let before = load_edit_draft(&vfs, &mut store, &id).unwrap();
        let mut edited = before.clone();
        edited.note = "Changed note".into();
        let after = save_contact_logged(
            &vfs,
            &mut store,
            "linux-test",
            SaveContactCommand { draft: edited },
        )
        .unwrap();

        assert_eq!(after.full_name, before.full_name);
        assert_eq!(after.family_name, before.family_name);
        assert_eq!(after.given_name, before.given_name);
        assert_eq!(after.middle_name, before.middle_name);
        assert_eq!(after.name_prefix, before.name_prefix);
        assert_eq!(after.name_suffix, before.name_suffix);
        assert_eq!(after.organization, before.organization);
        assert_eq!(after.job_title, before.job_title);
        assert_eq!(after.nickname, before.nickname);
        assert_eq!(after.urls, before.urls);
        assert_eq!(after.phones, before.phones);
        assert_eq!(after.emails, before.emails);
        assert_eq!(after.addresses, before.addresses);
        assert_eq!(after.birthday, before.birthday);
        assert_eq!(after.categories, before.categories);
        assert_eq!(after.photo, before.photo);
        assert_eq!(after.note, "Changed note");
    }

    #[test]
    fn structured_name_edit_updates_list_title_after_reload() {
        let vfs = MemVfs::new();
        let mut store = Store::open(&vfs, "/lib").unwrap();
        let mut draft = new_edit_draft();
        draft.given_name = "Ada".into();
        draft.family_name = "Lovelace".into();
        let id = save_contact_logged(&vfs, &mut store, "linux-test", SaveContactCommand { draft })
            .unwrap()
            .id
            .unwrap();

        let mut edited = load_edit_draft(&vfs, &mut store, &id).unwrap();
        assert_eq!(edited.full_name, "Ada Lovelace");
        edited.given_name = "Augusta".into();
        save_contact_logged(
            &vfs,
            &mut store,
            "linux-test",
            SaveContactCommand { draft: edited },
        )
        .unwrap();

        let reopened = Store::open(&vfs, "/lib").unwrap();
        let rows = list_rows_filtered(&reopened, "", None);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Augusta Lovelace");
        let fields = detail_rows(&reopened, &id).unwrap();
        assert_eq!(fields[0].value, "Augusta Lovelace");
        let file_name = reopened.get(&id).unwrap().file_name.clone();
        let disk = vfs.read(&format!("/lib/{file_name}")).unwrap();
        let text = String::from_utf8(disk).unwrap();
        assert!(text.contains("FN:Augusta Lovelace"));
        assert!(text.contains("N:Lovelace;Augusta;"));
    }

    #[test]
    fn generated_css_has_sourced_dark_accent() {
        let css = include_str!("../../../design/tokens/generated/contacts.css");
        assert!(css.contains("#336BC7"));
        assert!(css.contains("#4D85DE"));
        assert!(css.contains("prefers-color-scheme: dark"));
    }

    #[test]
    fn c_loop_screens_are_in_the_spec() {
        use ContactsScreen::*;
        for screen in [
            FolderPicker,
            ContactList,
            ContactDetail,
            ContactEdit,
            Settings,
            TagManagement,
            Logs,
            SyncConflictGroup,
        ] {
            let _ = screen.as_str();
            let _ = screen.kind();
        }
        assert_eq!(AppleConflict.as_str(), "apple-conflict");
        assert_eq!(TagManagement.as_str(), "tag-management");
        assert_eq!(Logs.as_str(), "logs");
    }
}
