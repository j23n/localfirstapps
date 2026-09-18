//! Display rows and logged actions — the surface both shells call.

use std::sync::atomic::{AtomicUsize, Ordering};

use contacts_core::{
    apply_draft, assign_tag_logged, choice_rows, conflict_rows, delete_logged, draft_from_card,
    field_rows, list_rows, read_ops, resolve_logged, save_logged, write, Birthday, Card,
    ContactDraft, MemVfs, MergeKind, Store, TYPE_CONTACT_DELETED, TYPE_CONTACT_SAVED,
};
use localcore_vfs::{Entry, ReadSeek, Stat, Vfs, VfsError, VfsResult};

#[test]
fn list_and_fields_match_ffi_copy() {
    let vfs = MemVfs::new();
    let mut store = Store::open(&vfs, "/lib").unwrap();
    let mut card = Card::new("");
    apply_draft(
        &mut card,
        &ContactDraft {
            given: "Ada".into(),
            family: "Lovelace".into(),
            organization: "Analytical".into(),
            email: "ada@example".into(),
            ..ContactDraft::default()
        },
    );
    let saved = save_logged(&vfs, &mut store, "linux-test", card).unwrap();
    let rows = list_rows(&store, "ada");
    assert_eq!(rows[0].title, "Ada Lovelace");
    assert_eq!(rows[0].subtitle.as_deref(), Some("Analytical"));
    let fields = field_rows(store.get(&saved.local_id).unwrap());
    assert_eq!(fields[0].label, "Name");
    assert_eq!(fields[0].value, "Ada Lovelace");
    assert_eq!(
        read_ops(&vfs, "/lib").unwrap()[0].event_type,
        TYPE_CONTACT_SAVED
    );
}

#[test]
fn birthday_field_is_display_ready() {
    let mut card = Card::new("ada.vcf");
    card.birthday = Some(Birthday {
        year: Some(1815),
        month: 12,
        day: 10,
    });
    let with_year = field_rows(&card)
        .into_iter()
        .find(|row| row.id.as_deref() == Some("bday"))
        .unwrap();
    assert_eq!(with_year.value, "December 10, 1815");
    card.birthday = Some(Birthday {
        year: None,
        month: 3,
        day: 14,
    });
    let yearless = field_rows(&card)
        .into_iter()
        .find(|row| row.id.as_deref() == Some("bday"))
        .unwrap();
    assert_eq!(yearless.value, "March 14");
}

#[test]
fn conflict_row_trailing_is_needs_choice() {
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
    assert_eq!(rows[0].trailing, "needs choice");
    assert_eq!(rows[0].subtitle, "1 copy");
    assert_eq!(rows[0].disposition, MergeKind::Choice);
    let choices = choice_rows(&vfs, &store, "alice.vcf").unwrap();
    assert!(choices.iter().any(|row| row.id.contains('|')));
}

#[test]
fn draft_round_trip_and_delete_log() {
    let vfs = MemVfs::new();
    let mut store = Store::open(&vfs, "/lib").unwrap();
    let mut card = Card::new("");
    apply_draft(
        &mut card,
        &ContactDraft {
            given: "Ada".into(),
            family: "Lovelace".into(),
            phone: "555".into(),
            ..ContactDraft::default()
        },
    );
    let saved = save_logged(&vfs, &mut store, "linux-test", card).unwrap();
    let draft = draft_from_card(store.get(&saved.local_id).unwrap());
    assert_eq!(draft.given, "Ada");
    assert_eq!(draft.family, "Lovelace");
    assert_eq!(draft.phone, "555");
    delete_logged(&vfs, &mut store, "linux-test", &saved.local_id).unwrap();
    assert!(store.get(&saved.local_id).is_none());
    assert!(read_ops(&vfs, "/lib")
        .unwrap()
        .iter()
        .any(|op| op.event_type == TYPE_CONTACT_DELETED));
}

struct AppendFails {
    inner: MemVfs,
}

impl Vfs for AppendFails {
    fn open(&self, path: &str) -> VfsResult<Box<dyn ReadSeek + Send>> {
        self.inner.open(path)
    }

    fn stat(&self, path: &str) -> VfsResult<Stat> {
        self.inner.stat(path)
    }

    fn list(&self, dir: &str) -> VfsResult<Vec<Entry>> {
        self.inner.list(dir)
    }

    fn stat_entry(&self, path: &str) -> VfsResult<Entry> {
        self.inner.stat_entry(path)
    }

    fn create_dir_all(&self, dir: &str) -> VfsResult<()> {
        self.inner.create_dir_all(dir)
    }

    fn append(&self, path: &str, _bytes: &[u8]) -> VfsResult<()> {
        Err(VfsError::PermissionDenied {
            path: path.to_owned(),
        })
    }

    fn write_atomic(&self, path: &str, bytes: &[u8]) -> VfsResult<()> {
        self.inner.write_atomic(path, bytes)
    }

    fn exists(&self, path: &str) -> bool {
        self.inner.exists(path)
    }

    fn remove(&self, path: &str) -> VfsResult<()> {
        self.inner.remove(path)
    }

    fn rename(&self, from: &str, to: &str) -> VfsResult<()> {
        self.inner.rename(from, to)
    }
}

#[test]
fn tier_one_save_wins_when_the_folder_log_fails() {
    let vfs = AppendFails {
        inner: MemVfs::new(),
    };
    let mut store = Store::open(&vfs, "/lib").unwrap();
    let mut card = Card::new("alice.vcf");
    card.local_id = "alice".into();
    card.full_name = "Alice".into();

    let saved = save_logged(&vfs, &mut store, "linux-test", card).unwrap();

    assert_eq!(saved.local_id, "alice");
    assert!(vfs.exists("/lib/alice.vcf"));
    assert!(store.get("alice").is_some());
}

#[test]
fn resolution_rewalks_before_ignoring_a_folder_log_failure() {
    let vfs = AppendFails {
        inner: MemVfs::new(),
    };
    let mut card = Card::new("alice.vcf");
    card.local_id = "alice".into();
    card.full_name = "Alice".into();
    let bytes = write(&card).into_bytes();
    vfs.inner.insert("/lib/alice.vcf", bytes.clone());
    vfs.inner.insert(
        "/lib/alice.sync-conflict-20200901-120000-PHONE01.vcf",
        bytes,
    );
    let mut store = Store::open(&vfs, "/lib").unwrap();

    resolve_logged(&vfs, &mut store, "linux-test", "alice.vcf", &[]).unwrap();

    assert!(store.conflict_groups().is_empty());
    assert!(!vfs.exists("/lib/alice.sync-conflict-20200901-120000-PHONE01.vcf"));
}

struct ListCounter {
    inner: MemVfs,
    lists: AtomicUsize,
}

impl ListCounter {
    fn new() -> Self {
        Self {
            inner: MemVfs::new(),
            lists: AtomicUsize::new(0),
        }
    }
}

impl Vfs for ListCounter {
    fn open(&self, path: &str) -> VfsResult<Box<dyn ReadSeek + Send>> {
        self.inner.open(path)
    }

    fn stat(&self, path: &str) -> VfsResult<Stat> {
        self.inner.stat(path)
    }

    fn list(&self, dir: &str) -> VfsResult<Vec<Entry>> {
        self.lists.fetch_add(1, Ordering::SeqCst);
        self.inner.list(dir)
    }

    fn stat_entry(&self, path: &str) -> VfsResult<Entry> {
        self.inner.stat_entry(path)
    }

    fn create_dir_all(&self, dir: &str) -> VfsResult<()> {
        self.inner.create_dir_all(dir)
    }

    fn append(&self, path: &str, bytes: &[u8]) -> VfsResult<()> {
        self.inner.append(path, bytes)
    }

    fn write_atomic(&self, path: &str, bytes: &[u8]) -> VfsResult<()> {
        self.inner.write_atomic(path, bytes)
    }

    fn exists(&self, path: &str) -> bool {
        self.inner.exists(path)
    }

    fn remove(&self, path: &str) -> VfsResult<()> {
        self.inner.remove(path)
    }

    fn rename(&self, from: &str, to: &str) -> VfsResult<()> {
        self.inner.rename(from, to)
    }
}

#[test]
fn assign_tag_does_not_list_once_per_contact() {
    let vfs = ListCounter::new();
    for (name, id) in [("ann.vcf", "a-1"), ("ben.vcf", "b-1"), ("cam.vcf", "c-1")] {
        let mut card = Card::new(name);
        card.local_id = id.into();
        card.full_name = name.trim_end_matches(".vcf").to_ascii_uppercase();
        vfs.inner
            .insert(&format!("/lib/{name}"), write(&card).into_bytes());
    }
    let mut store = Store::open(&vfs, "/lib").unwrap();
    vfs.lists.store(0, Ordering::SeqCst);

    let changed = assign_tag_logged(
        &vfs,
        &mut store,
        "linux-test",
        "vip",
        &["a-1".into(), "b-1".into(), "c-1".into()],
    )
    .unwrap();
    assert_eq!(changed, 3);
    let lists = vfs.lists.load(Ordering::SeqCst);
    assert_eq!(
        lists, 1,
        "assign_tag should walk once, not once per contact"
    );
    for id in ["a-1", "b-1", "c-1"] {
        assert!(store.get(id).unwrap().categories.iter().any(|c| c == "vip"));
    }
}
