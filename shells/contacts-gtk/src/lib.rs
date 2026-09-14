//! LocalContacts GTK shell. Headless helpers live here so tests need no display.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use contacts_core::{
    append_deleted, append_group_resolved, append_saved, apply_merge, plan_merge, valid_device,
    Card, Labeled, MergeKind, Store, StoreError,
};
use localcore_vfs::Vfs;

mod window;

pub use window::Window;

/// Desktop file / libadwaita application id.
pub const APP_ID: &str = "com.j23n.LocalContacts";
/// Window title. Same product name as iOS.
pub const APP_TITLE: &str = "LocalContacts";
/// XDG application directory (ADR 0005 R5 exceptions).
pub const CONFIG_DIR_NAME: &str = "localcontacts";

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

/// Edit form. Shell mapping; the core still owns [`Card`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContactDraft {
    /// Existing id when editing.
    pub id: Option<String>,
    /// N given.
    pub given: String,
    /// N family.
    pub family: String,
    /// ORG.
    pub organization: String,
    /// First TEL, or empty.
    pub phone: String,
    /// First EMAIL, or empty.
    pub email: String,
    /// NOTE.
    pub note: String,
}

/// Prefill a draft from a card.
#[must_use]
pub fn draft_from_card(card: &Card) -> ContactDraft {
    ContactDraft {
        id: Some(card.local_id.clone()),
        given: card.given_name.clone(),
        family: card.family_name.clone(),
        organization: card.organization.clone(),
        phone: card
            .phones
            .first()
            .map(|p| p.value.clone())
            .unwrap_or_default(),
        email: card
            .emails
            .first()
            .map(|e| e.value.clone())
            .unwrap_or_default(),
        note: card.note.clone(),
    }
}

/// Apply form fields. FN is left empty so write uses [`Card::display_name`].
pub fn apply_draft(card: &mut Card, draft: &ContactDraft) {
    card.given_name = draft.given.trim().to_string();
    card.family_name = draft.family.trim().to_string();
    card.full_name.clear();
    card.organization = draft.organization.trim().to_string();
    card.note = draft.note.trim().to_string();
    set_first_labeled(&mut card.phones, "cell", draft.phone.trim());
    set_first_labeled(&mut card.emails, "home", draft.email.trim());
}

fn set_first_labeled(rows: &mut Vec<Labeled>, default_label: &str, value: &str) {
    if value.is_empty() {
        rows.clear();
        return;
    }
    if let Some(first) = rows.first_mut() {
        first.value = value.to_string();
    } else {
        rows.push(Labeled {
            label: default_label.into(),
            value: value.to_string(),
        });
    }
}

/// One contact-list row. Copy matches `contacts-ffi` `list_rows`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListRow {
    /// `X-LOCALCONTACTS-ID`.
    pub id: String,
    /// Display name.
    pub title: String,
    /// Organization, else first email.
    pub subtitle: Option<String>,
}

/// Sorted list rows for `query` (core search).
#[must_use]
pub fn list_rows(store: &Store, query: &str) -> Vec<ListRow> {
    store
        .search(query)
        .into_iter()
        .map(|card| ListRow {
            id: card.local_id.clone(),
            title: card.display_name(),
            subtitle: if !card.organization.is_empty() {
                Some(card.organization.clone())
            } else {
                card.emails.first().map(|e| e.value.clone())
            },
        })
        .collect()
}

/// One detail field. Copy matches `contacts-ffi` `field_rows`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldView {
    /// Field label.
    pub label: String,
    /// Ready-to-show value.
    pub value: String,
}

/// Detail fields for one card.
#[must_use]
pub fn detail_fields(card: &Card) -> Vec<FieldView> {
    let mut rows = vec![FieldView {
        label: "Name".into(),
        value: card.display_name(),
    }];
    if !card.organization.is_empty() {
        rows.push(FieldView {
            label: "Organization".into(),
            value: card.organization.clone(),
        });
    }
    for phone in &card.phones {
        rows.push(FieldView {
            label: phone.label.clone(),
            value: phone.value.clone(),
        });
    }
    for email in &card.emails {
        rows.push(FieldView {
            label: email.label.clone(),
            value: email.value.clone(),
        });
    }
    if !card.note.is_empty() {
        rows.push(FieldView {
            label: "Note".into(),
            value: card.note.clone(),
        });
    }
    rows
}

/// Trailing string on a conflict row. Same as `contacts-ffi`.
#[must_use]
pub fn merge_trailing(kind: MergeKind) -> &'static str {
    match kind {
        MergeKind::Auto => "auto",
        MergeKind::Choice => "needs choice",
        MergeKind::DeletedVersusModified => "keep copy",
    }
}

/// One Syncthing group as a list row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictSummary {
    /// Surviving basename.
    pub canonical: String,
    /// Number of conflict copies.
    pub copies: usize,
    /// `auto` / `needs choice` / `keep copy`.
    pub trailing: &'static str,
}

/// Conflict groups for the banner and sheet.
pub fn conflict_summaries(
    vfs: &dyn Vfs,
    store: &Store,
) -> Result<Vec<ConflictSummary>, StoreError> {
    let mut rows = Vec::new();
    for group in store.conflict_groups() {
        let plan = plan_merge(vfs, &store.root, group)?;
        rows.push(ConflictSummary {
            canonical: group.canonical_name.clone(),
            copies: group.copies.len(),
            trailing: merge_trailing(plan.kind),
        });
    }
    Ok(rows)
}

/// One field-choice side. `id` is `field|source`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceView {
    /// `field|source`.
    pub id: String,
    /// Field key.
    pub field: String,
    /// `surviving` or a copy name.
    pub source: String,
    /// Formatted value.
    pub value: String,
}

/// Choice rows for one group.
pub fn choice_views(
    vfs: &dyn Vfs,
    store: &Store,
    canonical: &str,
) -> Result<Vec<ChoiceView>, StoreError> {
    let group = store
        .conflict_groups()
        .iter()
        .find(|g| g.canonical_name == canonical)
        .cloned()
        .ok_or(StoreError::NotFound)?;
    let plan = plan_merge(vfs, &store.root, &group)?;
    let mut rows = Vec::new();
    for conflict in plan.conflicts {
        for (source, value) in conflict.sides {
            rows.push(ChoiceView {
                id: format!("{}|{source}", conflict.field),
                field: conflict.field.clone(),
                source,
                value,
            });
        }
    }
    Ok(rows)
}

/// Save and append `contact_saved`.
pub fn save_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    card: Card,
) -> Result<Card, StoreError> {
    let saved = store.save(vfs, card)?;
    append_saved(vfs, &store.root, device, &saved.local_id)?;
    Ok(saved)
}

/// Delete and append `contact_deleted`.
pub fn delete_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    id: &str,
) -> Result<(), StoreError> {
    store.delete(vfs, id)?;
    append_deleted(vfs, &store.root, device, id)?;
    Ok(())
}

/// Apply a merge, log it, and re-walk.
pub fn resolve_logged(
    vfs: &dyn Vfs,
    store: &mut Store,
    device: &str,
    canonical: &str,
    choice_ids: &[String],
) -> Result<(), StoreError> {
    let group = store
        .conflict_groups()
        .iter()
        .find(|g| g.canonical_name == canonical)
        .cloned()
        .ok_or(StoreError::NotFound)?;
    let plan = plan_merge(vfs, &store.root, &group)?;
    if plan.kind == MergeKind::Choice && choice_ids.is_empty() {
        return Err(StoreError::NeedsChoice);
    }
    let choices: Vec<(String, String)> = choice_ids
        .iter()
        .filter_map(|raw| {
            raw.split_once('|')
                .map(|(field, source)| (field.to_owned(), source.to_owned()))
        })
        .collect();
    apply_merge(vfs, &store.root, &plan, &choices)?;
    let kind = match plan.kind {
        MergeKind::Auto => "auto",
        MergeKind::Choice => "choice",
        MergeKind::DeletedVersusModified => "keep_copy",
    };
    append_group_resolved(vfs, &store.root, device, canonical, kind)?;
    *store = Store::open(vfs, &store.root)?;
    Ok(())
}

/// User-visible [`StoreError`].
#[must_use]
pub fn format_store_error(err: &StoreError) -> String {
    match err {
        StoreError::Io(message) => message.clone(),
        StoreError::NotFound => "Not found".into(),
        StoreError::NeedsChoice => "This group needs a field choice".into(),
        StoreError::IncompleteChoices => "Choose a value for every field".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use contacts_core::{read_ops, write, TYPE_CONTACT_SAVED};
    use localcore_vfs::MemVfs;

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
        let fields = detail_fields(store.get(&saved.local_id).unwrap());
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
        let rows = conflict_summaries(&vfs, &store).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].canonical, "alice.vcf");
        assert_eq!(rows[0].copies, 1);
        assert_eq!(rows[0].trailing, "needs choice");
    }

    #[test]
    fn generated_css_has_sourced_dark_accent() {
        let css = include_str!("../../../design/tokens/generated/contacts.css");
        assert!(css.contains("#336BC7"));
        assert!(css.contains("#4D85DE"));
        assert!(css.contains("prefers-color-scheme: dark"));
    }
}
