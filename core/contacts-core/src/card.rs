//! In-core contact card. This type does **not** cross UniFFI (ADR 0003 R6).

/// One labeled string (TEL, EMAIL, URL).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Labeled {
    /// TYPE token, already lowercased.
    pub label: String,
    /// Unescaped value.
    pub value: String,
}

/// ADR-style postal address (vCard ADR fields 2–6).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct PostalAddress {
    /// Street (ADR field 2).
    pub street: String,
    /// City (field 3).
    pub city: String,
    /// Region (field 4).
    pub state: String,
    /// Postal code (field 5).
    pub postal_code: String,
    /// Country (field 6).
    pub country: String,
}

impl PostalAddress {
    /// True when every component is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.street.is_empty()
            && self.city.is_empty()
            && self.state.is_empty()
            && self.postal_code.is_empty()
            && self.country.is_empty()
    }

    /// Single-line form used as a merge key and a field-row value.
    #[must_use]
    pub fn formatted(&self) -> String {
        let region = [self.state.as_str(), self.postal_code.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        [
            self.street.as_str(),
            self.city.as_str(),
            region.as_str(),
            self.country.as_str(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
    }
}

/// One labeled postal address.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LabeledAddress {
    /// TYPE token.
    pub label: String,
    /// Address components.
    pub value: PostalAddress,
}

/// Parsed BDAY. `year` is `None` for `--MM-DD`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Birthday {
    /// Four-digit year, when present.
    pub year: Option<i32>,
    /// Month 1–12.
    pub month: u8,
    /// Day 1–31.
    pub day: u8,
}

/// One contact as stored and merged. Identity is [`Card::local_id`], never a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    /// `X-LOCALCONTACTS-ID`. Empty only before a migration assign.
    pub local_id: String,
    /// Basename of the `.vcf` this card lives in.
    pub file_name: String,
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
    /// ORG (first component).
    pub organization: String,
    /// TITLE.
    pub job_title: String,
    /// NICKNAME.
    pub nickname: String,
    /// URL rows.
    pub urls: Vec<Labeled>,
    /// TEL rows.
    pub phones: Vec<Labeled>,
    /// EMAIL rows.
    pub emails: Vec<Labeled>,
    /// ADR rows.
    pub addresses: Vec<LabeledAddress>,
    /// BDAY.
    pub birthday: Option<Birthday>,
    /// NOTE.
    pub note: String,
    /// CATEGORIES.
    pub categories: Vec<String>,
    /// Decoded PHOTO bytes.
    pub photo: Option<Vec<u8>>,
    /// PHOTO TYPE parameter retained while the photo bytes are untouched.
    pub photo_media_type: Option<String>,
    /// Unrecognized lines, preserved for round-trip.
    pub unknown_fields: Vec<String>,
}

impl Card {
    /// Empty card with `file_name` set.
    #[must_use]
    pub fn new(file_name: impl Into<String>) -> Self {
        Self {
            local_id: String::new(),
            file_name: file_name.into(),
            full_name: String::new(),
            family_name: String::new(),
            given_name: String::new(),
            middle_name: String::new(),
            name_prefix: String::new(),
            name_suffix: String::new(),
            organization: String::new(),
            job_title: String::new(),
            nickname: String::new(),
            urls: Vec::new(),
            phones: Vec::new(),
            emails: Vec::new(),
            addresses: Vec::new(),
            birthday: None,
            note: String::new(),
            categories: Vec::new(),
            photo: None,
            photo_media_type: None,
            unknown_fields: Vec::new(),
        }
    }

    /// FN, else given/middle/family, else `"No Name"`.
    #[must_use]
    pub fn display_name(&self) -> String {
        if !self.full_name.is_empty() {
            return self.full_name.clone();
        }
        let parts: Vec<&str> = [
            self.given_name.as_str(),
            self.middle_name.as_str(),
            self.family_name.as_str(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
        if parts.is_empty() {
            "No Name".into()
        } else {
            parts.join(" ")
        }
    }

    /// Sort labeled lists and categories so a write is byte-stable (R9).
    pub fn canonicalize(&mut self) {
        self.urls.sort();
        self.phones.sort();
        self.emails.sort();
        self.addresses.sort();
        self.categories.sort();
        self.categories.dedup();
        self.unknown_fields.sort();
    }
}

/// How contacts are distributed across `.vcf` files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layout {
    /// No cards.
    Empty,
    /// Every file holds one card (or the folder has a single card).
    OneFilePerContact,
    /// Every card shares `file_name`.
    SingleFile {
        /// Shared basename.
        file_name: String,
    },
    /// Mix of single- and multi-card files.
    Mixed,
}

impl Layout {
    /// Derive layout from the cards currently loaded.
    #[must_use]
    pub fn of(cards: &[Card]) -> Self {
        let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
        for card in cards {
            *counts.entry(card.file_name.as_str()).or_default() += 1;
        }
        if counts.is_empty() {
            return Self::Empty;
        }
        let oversize = counts.values().any(|&n| n > 1);
        if !oversize {
            return Self::OneFilePerContact;
        }
        if counts.len() == 1 {
            let (name, _) = counts.into_iter().next().expect("len == 1");
            return Self::SingleFile {
                file_name: name.to_owned(),
            };
        }
        Self::Mixed
    }
}
