//! LocalContacts GTK shell. Host paths and `--comet` live here so tests
//! need no display. Display rows and logged actions come from
//! `contacts-core` (Milestone C).

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use contacts_core::valid_device;

mod window;

pub use window::Window;

/// Desktop file / libadwaita application id.
pub const APP_ID: &str = "com.j23n.LocalContacts";
/// Window title. Same product name as iOS.
pub const APP_TITLE: &str = "LocalContacts";
/// XDG application directory (ADR 0005 R5 exceptions).
pub const CONFIG_DIR_NAME: &str = "localcontacts";

/// Width at or below which the GTK shell uses Comet chrome (bottom nav).
pub const COMPACT_WIDTH: i32 = 550;

/// True when `--comet` is among the process arguments.
#[must_use]
pub fn wants_comet<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().any(|a| a.as_ref() == "--comet")
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
        apply_draft, conflict_rows, field_rows, list_rows, read_ops, save_logged, write, Card,
        ContactDraft, MemVfs, MergeKind, Store, TYPE_CONTACT_SAVED,
    };
    use shell_kit_gtk::ContactsScreen;

    #[test]
    fn comet_flag_is_opt_in() {
        assert!(!wants_comet(["localcontacts"]));
        assert!(wants_comet(["localcontacts", "--comet"]));
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
        let mut card = Card::new("");
        apply_draft(
            &mut card,
            &ContactDraft {
                given: "Ada".into(),
                family: "Lovelace".into(),
                organization: "Analytical".into(),
                phone: "555".into(),
                email: "ada@example".into(),
                note: "note".into(),
                ..ContactDraft::default()
            },
        );
        let saved = save_logged(&vfs, &mut store, "linux-test", card).unwrap();
        let rows = list_rows(&store, "ada");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, saved.local_id);
        assert_eq!(rows[0].title, "Ada Lovelace");
        assert_eq!(rows[0].subtitle.as_deref(), Some("Analytical"));
        let fields = field_rows(store.get(&saved.local_id).unwrap());
        assert_eq!(fields[0].label, "Name");
        let ops = read_ops(&vfs, "/lib").unwrap();
        assert_eq!(ops[0].event_type, TYPE_CONTACT_SAVED);
    }

    #[test]
    fn conflict_summary_trailing() {
        let vfs = MemVfs::new();
        let mut alice = Card::new("alice.vcf");
        alice.local_id = "alice-1".into();
        alice.full_name = "Alice".into();
        vfs.insert("/lib/alice.vcf", write(&alice).into_bytes());
        vfs.insert(
            "/lib/alice.sync-conflict-20200901-120000-PHONE01.vcf",
            b"BEGIN:VCARD\nVERSION:3.0\nFN:Alice Phone\nEND:VCARD\n".to_vec(),
        );
        let store = Store::open(&vfs, "/lib").unwrap();
        let rows = conflict_rows(&vfs, &store).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "alice.vcf");
        assert_eq!(rows[0].subtitle, "1 copy");
        assert_eq!(rows[0].trailing, "needs choice");
        assert_eq!(rows[0].disposition, MergeKind::Choice);
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
