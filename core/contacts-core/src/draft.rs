//! Core-owned contact edit boundary.
//!
//! A draft contains every field the current shells may edit, but none of the
//! storage-only fields (`file_name`, unknown vCard properties). Existing
//! drafts carry a deterministic token so save can refuse an edit based on a
//! stale projection.

use sha2::{Digest, Sha256};

use crate::card::{Birthday, Card, Labeled, LabeledAddress, PostalAddress};
use crate::vcard::write;

/// One editable labeled string.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LabeledValueDraft {
    /// TYPE label.
    pub label: String,
    /// Unescaped value.
    pub value: String,
}

/// One editable postal address.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LabeledAddressDraft {
    /// TYPE label.
    pub label: String,
    /// Street.
    pub street: String,
    /// City.
    pub city: String,
    /// Region/state.
    pub state: String,
    /// Postal code.
    pub postal_code: String,
    /// Country.
    pub country: String,
}

/// Editable birthday. `year` is absent for `--MM-DD`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BirthdayDraft {
    /// Four-digit year, when known.
    pub year: Option<i32>,
    /// Month 1–12.
    pub month: u8,
    /// Day 1–31.
    pub day: u8,
}

/// Full edit form owned by the core.
///
/// `id` and `content_token` are concurrency metadata, not editable Card
/// fields. Unknown vCard fields and the file name intentionally do not cross
/// this boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContactEditDraft {
    /// Existing `X-LOCALCONTACTS-ID`; absent for a new contact.
    pub id: Option<String>,
    /// SHA-256 of the deterministic core projection used to open this draft.
    pub content_token: Option<String>,
    /// FN.
    pub full_name: String,
    /// N family.
    pub family_name: String,
    /// N given.
    pub given_name: String,
    /// N middle.
    pub middle_name: String,
    /// N prefix.
    pub name_prefix: String,
    /// N suffix.
    pub name_suffix: String,
    /// ORG.
    pub organization: String,
    /// TITLE.
    pub job_title: String,
    /// NICKNAME.
    pub nickname: String,
    /// URL rows, including repeated labels.
    pub urls: Vec<LabeledValueDraft>,
    /// TEL rows, including repeated labels.
    pub phones: Vec<LabeledValueDraft>,
    /// EMAIL rows, including repeated labels.
    pub emails: Vec<LabeledValueDraft>,
    /// Structured ADR rows, including repeated labels.
    pub addresses: Vec<LabeledAddressDraft>,
    /// BDAY.
    pub birthday: Option<BirthdayDraft>,
    /// NOTE.
    pub note: String,
    /// CATEGORIES.
    pub categories: Vec<String>,
    /// Decoded PHOTO bytes.
    pub photo: Option<Vec<u8>>,
}

/// Typed save intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveContactCommand {
    /// Editable values and the generation token they were based on.
    pub draft: ContactEditDraft,
}

/// Empty full draft for creating a contact.
#[must_use]
pub fn new_edit_draft() -> ContactEditDraft {
    ContactEditDraft::default()
}

/// Deterministic content token for stale-edit detection.
#[must_use]
pub fn content_token(card: &Card) -> String {
    let digest = Sha256::digest(write(card).as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Build a full edit draft from an authoritative Card.
#[must_use]
pub fn edit_draft_from_card(card: &Card) -> ContactEditDraft {
    ContactEditDraft {
        id: Some(card.local_id.clone()),
        content_token: Some(content_token(card)),
        full_name: card.full_name.clone(),
        family_name: card.family_name.clone(),
        given_name: card.given_name.clone(),
        middle_name: card.middle_name.clone(),
        name_prefix: card.name_prefix.clone(),
        name_suffix: card.name_suffix.clone(),
        organization: card.organization.clone(),
        job_title: card.job_title.clone(),
        nickname: card.nickname.clone(),
        urls: to_labeled_drafts(&card.urls),
        phones: to_labeled_drafts(&card.phones),
        emails: to_labeled_drafts(&card.emails),
        addresses: card
            .addresses
            .iter()
            .map(|row| LabeledAddressDraft {
                label: row.label.clone(),
                street: row.value.street.clone(),
                city: row.value.city.clone(),
                state: row.value.state.clone(),
                postal_code: row.value.postal_code.clone(),
                country: row.value.country.clone(),
            })
            .collect(),
        birthday: card.birthday.as_ref().map(|birthday| BirthdayDraft {
            year: birthday.year,
            month: birthday.month,
            day: birthday.day,
        }),
        note: card.note.clone(),
        categories: card.categories.clone(),
        photo: card.photo.clone(),
    }
}

/// Apply only fields declared editable by [`ContactEditDraft`].
///
/// Storage identity, file placement, unknown properties, and PHOTO metadata
/// stay on the authoritative Card. Empty labeled rows are retained because an
/// empty value is explicit editor state, not permission to drop another row.
pub fn apply_edit_draft(card: &mut Card, draft: &ContactEditDraft) {
    let photo_changed = card.photo != draft.photo;
    card.full_name = draft.full_name.clone();
    card.family_name = draft.family_name.clone();
    card.given_name = draft.given_name.clone();
    card.middle_name = draft.middle_name.clone();
    card.name_prefix = draft.name_prefix.clone();
    card.name_suffix = draft.name_suffix.clone();
    card.organization = draft.organization.clone();
    card.job_title = draft.job_title.clone();
    card.nickname = draft.nickname.clone();
    card.urls = from_labeled_drafts(&draft.urls);
    card.phones = from_labeled_drafts(&draft.phones);
    card.emails = from_labeled_drafts(&draft.emails);
    card.addresses = draft
        .addresses
        .iter()
        .map(|row| LabeledAddress {
            label: row.label.clone(),
            value: PostalAddress {
                street: row.street.clone(),
                city: row.city.clone(),
                state: row.state.clone(),
                postal_code: row.postal_code.clone(),
                country: row.country.clone(),
            },
        })
        .collect();
    card.birthday = draft.birthday.as_ref().map(|birthday| Birthday {
        year: birthday.year,
        month: birthday.month,
        day: birthday.day,
    });
    card.note = draft.note.clone();
    card.categories = draft.categories.clone();
    card.photo = draft.photo.clone();
    if photo_changed {
        card.photo_media_type = card.photo.as_ref().map(|_| "JPEG".to_owned());
    }
}

fn to_labeled_drafts(rows: &[Labeled]) -> Vec<LabeledValueDraft> {
    rows.iter()
        .map(|row| LabeledValueDraft {
            label: row.label.clone(),
            value: row.value.clone(),
        })
        .collect()
}

fn from_labeled_drafts(rows: &[LabeledValueDraft]) -> Vec<Labeled> {
    rows.iter()
        .map(|row| Labeled {
            label: row.label.clone(),
            value: row.value.clone(),
        })
        .collect()
}

/// Legacy compact GTK form retained until that shell adopts
/// [`ContactEditDraft`].
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

/// Prefill the legacy compact draft from a card.
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

/// Apply legacy compact form fields.
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
