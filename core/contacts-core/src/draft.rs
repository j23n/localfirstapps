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
        if !rows.is_empty() {
            rows.remove(0);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearing_visible_phone_preserves_additional_values() {
        let mut card = Card::new("alice.vcf");
        card.phones = vec![
            Labeled {
                label: "cell".into(),
                value: "111".into(),
            },
            Labeled {
                label: "work".into(),
                value: "222".into(),
            },
        ];
        let mut draft = draft_from_card(&card);
        draft.phone.clear();

        apply_draft(&mut card, &draft);

        assert_eq!(
            card.phones,
            vec![Labeled {
                label: "work".into(),
                value: "222".into(),
            }]
        );
    }

    #[test]
    fn editing_visible_email_preserves_additional_values() {
        let mut card = Card::new("alice.vcf");
        card.emails = vec![
            Labeled {
                label: "home".into(),
                value: "old@example.com".into(),
            },
            Labeled {
                label: "work".into(),
                value: "work@example.com".into(),
            },
        ];
        let mut draft = draft_from_card(&card);
        draft.email = "new@example.com".into();

        apply_draft(&mut card, &draft);

        assert_eq!(card.emails.len(), 2);
        assert_eq!(card.emails[0].value, "new@example.com");
        assert_eq!(card.emails[1].value, "work@example.com");
    }
}
