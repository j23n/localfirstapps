//! Form fields → [`Card`]. Both shells apply the same mapping.

use crate::card::{Card, Labeled};

/// Edit form. The shell collects strings; the core writes the card.
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
