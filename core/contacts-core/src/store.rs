//! Folder index: walk `.vcf`, search, save, delete. Through [`Vfs`] only.

use localcore_conflict::ConflictGroup;
use localcore_vfs::{Vfs, VfsError};
use localcore_walk::walk;
use uuid::Uuid;

use crate::card::{Card, Layout};
use crate::vcard::{parse_multiple, suggested_file_name, write};

/// Temp prefix already listed in `apps/contacts/.stignore`.
pub const TEMP_PREFIX: &str = ".contacts-tmp-";

/// Join `root` and a basename without inventing a scheme.
#[must_use]
pub fn join_root(root: &str, name: &str) -> String {
    let root = root.trim_end_matches('/');
    if name.is_empty() {
        return root.to_owned();
    }
    format!("{root}/{name}")
}

/// Headless store. Cards are the authority; this index is a projection (R3).
#[derive(Debug, Clone)]
pub struct Store {
    /// Folder the walk started at.
    pub root: String,
    cards: Vec<Card>,
    groups: Vec<ConflictGroup>,
}

/// Failures the store surfaces. No domain record crosses FFI; this stays here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// Path is not UTF-8 vCard bytes, or the folder cannot be read.
    Io(String),
    /// No card with that id.
    NotFound,
    /// Automatic merge cannot apply: a field needs a choice.
    NeedsChoice,
    /// Apply was called without a pick for every conflicted field.
    IncompleteChoices,
    /// An edit was based on an older authoritative card.
    StaleEdit {
        /// Token supplied by the editor.
        expected: String,
        /// Token for the card currently on disk.
        actual: String,
    },
    /// A typed command contains invalid or inconsistent values.
    InvalidCommand(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) => write!(f, "{message}"),
            Self::NotFound => write!(f, "Not found"),
            Self::NeedsChoice => write!(f, "This group needs a field choice"),
            Self::IncompleteChoices => write!(f, "Choose a value for every field"),
            Self::StaleEdit { .. } => {
                write!(f, "This contact changed on disk. Reopen it before saving")
            }
            Self::InvalidCommand(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<VfsError> for StoreError {
    fn from(err: VfsError) -> Self {
        Self::Io(err.to_string())
    }
}

impl Store {
    /// Walk `root`, load surviving `.vcf` files, assign missing ids once.
    pub fn open(vfs: &dyn Vfs, root: &str) -> Result<Self, StoreError> {
        let outcome = walk(vfs, root, |name| {
            name.to_ascii_lowercase().ends_with(".vcf")
        });
        let mut cards = Vec::new();
        for file in &outcome.files {
            let bytes = vfs.read(&file.path)?;
            let file_name = relative_to_root(root, &file.path);
            let mut parsed = parse_multiple(&bytes, &file_name, false);
            let mut rewrite = false;
            for card in &mut parsed {
                if card.local_id.is_empty() {
                    card.local_id = Uuid::new_v4().to_string();
                    rewrite = true;
                }
            }
            if rewrite {
                let joined: String = parsed.iter().map(write).collect();
                vfs.write_atomic(&file.path, joined.as_bytes())?;
            }
            cards.extend(parsed);
        }
        Ok(Self {
            root: root.to_owned(),
            cards,
            groups: outcome.conflict_groups,
        })
    }

    /// Loaded cards, in walk order.
    #[must_use]
    pub fn cards(&self) -> &[Card] {
        &self.cards
    }

    /// Syncthing groups from the last walk. Copies are never cards.
    #[must_use]
    pub fn conflict_groups(&self) -> &[ConflictGroup] {
        &self.groups
    }

    /// Stable folder-relative key for a conflict group.
    #[must_use]
    pub(crate) fn conflict_id(&self, group: &ConflictGroup) -> String {
        relative_to_root(&self.root, &group.id())
    }

    /// Conflict group selected by its opaque folder-relative key.
    #[must_use]
    pub(crate) fn conflict_group(&self, id: &str) -> Option<&ConflictGroup> {
        self.groups
            .iter()
            .find(|group| self.conflict_id(group) == id)
    }

    /// Folder layout.
    #[must_use]
    pub fn layout(&self) -> Layout {
        Layout::of(&self.cards)
    }

    /// Card by `X-LOCALCONTACTS-ID`.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Card> {
        self.cards.iter().find(|c| c.local_id == id)
    }

    /// Substring search on display name, org, title, phones, emails.
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<&Card> {
        let q = query.to_lowercase();
        if q.is_empty() {
            let mut all: Vec<&Card> = self.cards.iter().collect();
            all.sort_by(|a, b| {
                a.display_name()
                    .to_lowercase()
                    .cmp(&b.display_name().to_lowercase())
            });
            return all;
        }
        let mut hits: Vec<&Card> = self
            .cards
            .iter()
            .filter(|c| {
                c.display_name().to_lowercase().contains(&q)
                    || c.organization.to_lowercase().contains(&q)
                    || c.job_title.to_lowercase().contains(&q)
                    || c.phones.iter().any(|p| p.value.to_lowercase().contains(&q))
                    || c.emails.iter().any(|e| e.value.to_lowercase().contains(&q))
            })
            .collect();
        hits.sort_by(|a, b| {
            a.display_name()
                .to_lowercase()
                .cmp(&b.display_name().to_lowercase())
        });
        hits
    }

    /// Write `card` through [`Vfs::write_atomic`]. Re-reads siblings on disk.
    pub fn save(&mut self, vfs: &dyn Vfs, mut card: Card) -> Result<Card, StoreError> {
        if card.local_id.is_empty() {
            card.local_id = Uuid::new_v4().to_string();
        }
        if card.file_name.is_empty() {
            card.file_name = self
                .get(&card.local_id)
                .map(|existing| existing.file_name.clone())
                .unwrap_or_else(|| match self.layout() {
                    Layout::SingleFile { file_name } => file_name,
                    _ => String::new(),
                });
            if card.file_name.is_empty() {
                card.file_name = self.unique_file_name(vfs, &card)?;
            }
        }
        let path = join_root(&self.root, &card.file_name);
        let mut file_cards: Vec<Card> = if vfs.try_exists(&path)? {
            let bytes = vfs.read(&path)?;
            let mut disk = parse_multiple(&bytes, &card.file_name, false);
            for sibling in &mut disk {
                if sibling.local_id.is_empty() {
                    sibling.local_id = Uuid::new_v4().to_string();
                }
            }
            if disk.is_empty() {
                let mut keep: Vec<Card> = self
                    .cards
                    .iter()
                    .filter(|c| c.file_name == card.file_name && c.local_id != card.local_id)
                    .cloned()
                    .collect();
                keep.push(card.clone());
                keep
            } else if let Some(idx) = disk.iter().position(|c| c.local_id == card.local_id) {
                disk[idx] = card.clone();
                disk
            } else {
                disk.push(card.clone());
                disk
            }
        } else {
            vec![card.clone()]
        };
        let joined: String = file_cards.iter().map(write).collect();
        vfs.write_atomic(&path, joined.as_bytes())?;
        self.upsert(card.clone());
        for sibling in file_cards.drain(..) {
            if sibling.local_id != card.local_id {
                self.upsert(sibling);
            }
        }
        Ok(card)
    }

    /// Remove a card. Deletes the file when it was the last sibling.
    pub fn delete(&mut self, vfs: &dyn Vfs, id: &str) -> Result<(), StoreError> {
        let idx = self
            .cards
            .iter()
            .position(|c| c.local_id == id)
            .ok_or(StoreError::NotFound)?;
        let file_name = self.cards[idx].file_name.clone();
        self.cards.remove(idx);
        if file_name.is_empty() {
            return Ok(());
        }
        let path = join_root(&self.root, &file_name);
        let remaining: Vec<Card> = self
            .cards
            .iter()
            .filter(|c| c.file_name == file_name)
            .cloned()
            .collect();
        if remaining.is_empty() {
            if vfs.try_exists(&path)? {
                vfs.remove(&path)?;
            }
        } else {
            let joined: String = remaining.iter().map(write).collect();
            vfs.write_atomic(&path, joined.as_bytes())?;
        }
        Ok(())
    }

    fn upsert(&mut self, card: Card) {
        if let Some(idx) = self.cards.iter().position(|c| c.local_id == card.local_id) {
            self.cards[idx] = card;
        } else {
            self.cards.push(card);
        }
    }

    fn unique_file_name(&self, vfs: &dyn Vfs, card: &Card) -> Result<String, StoreError> {
        let base = suggested_file_name(card);
        let stem = base.trim_end_matches(".vcf");
        let in_memory: std::collections::BTreeSet<&str> =
            self.cards.iter().map(|c| c.file_name.as_str()).collect();
        let mut candidate = base.clone();
        let mut n = 1;
        while vfs.try_exists(&join_root(&self.root, &candidate))?
            || in_memory.contains(candidate.as_str())
        {
            candidate = format!("{stem}-{n}.vcf");
            n += 1;
        }
        Ok(candidate)
    }
}

fn relative_to_root(root: &str, path: &str) -> String {
    let root = root.trim_end_matches(['/', '\\']);
    path.strip_prefix(root)
        .and_then(|rest| rest.strip_prefix(['/', '\\']))
        .unwrap_or(path)
        .to_owned()
}
