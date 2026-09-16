//! vCard 3.0 parse / write, matching the Swift `VCardParser` / `VCardWriter`.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use uuid::Uuid;

use crate::card::{Birthday, Card, Labeled, LabeledAddress, PostalAddress};

/// Parse one vCard. `None` when the bytes are not UTF-8 or lack `BEGIN:VCARD`.
#[must_use]
pub fn parse(data: &[u8], file_name: &str, assign_default_id: bool) -> Option<Card> {
    let text = std::str::from_utf8(data).ok()?;
    parse_str(text, file_name, assign_default_id)
}

/// Parse every complete `BEGIN`/`END` block in `data`.
#[must_use]
pub fn parse_multiple(data: &[u8], file_name: &str, assign_default_id: bool) -> Vec<Card> {
    let Ok(text) = std::str::from_utf8(data) else {
        return Vec::new();
    };
    let unfolded = unfold(text);
    let mut cards = Vec::new();
    let mut current = String::new();
    let mut in_card = false;
    for line in unfolded.split('\n') {
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("BEGIN:VCARD") {
            in_card = true;
            current.clear();
            current.push_str(line);
            current.push('\n');
        } else if upper.starts_with("END:VCARD") {
            current.push_str(line);
            current.push('\n');
            if let Some(card) = parse_str(&current, file_name, assign_default_id) {
                cards.push(card);
            }
            current.clear();
            in_card = false;
        } else if in_card {
            current.push_str(line);
            current.push('\n');
        }
    }
    cards
}

/// Canonical vCard 3.0 bytes (CRLF, folded, fixed field order).
#[must_use]
pub fn write(card: &Card) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("BEGIN:VCARD".into());
    lines.push("VERSION:3.0".into());
    lines.push(format!("X-LOCALCONTACTS-ID:{}", card.local_id));
    let n = [
        escape(&card.family_name),
        escape(&card.given_name),
        escape(&card.middle_name),
        escape(&card.name_prefix),
        escape(&card.name_suffix),
    ]
    .join(";");
    lines.push(format!("N:{n}"));
    let fn_ = if card.full_name.is_empty() {
        card.display_name()
    } else {
        card.full_name.clone()
    };
    lines.push(format!("FN:{}", escape(&fn_)));
    if !card.organization.is_empty() {
        lines.push(format!("ORG:{}", escape(&card.organization)));
    }
    if !card.job_title.is_empty() {
        lines.push(format!("TITLE:{}", escape(&card.job_title)));
    }
    if !card.nickname.is_empty() {
        lines.push(format!("NICKNAME:{}", escape(&card.nickname)));
    }
    for url in &card.urls {
        let ty = sanitize_type(&url.label, "homepage");
        lines.push(format!("URL;TYPE={ty}:{}", escape(&url.value)));
    }
    for phone in &card.phones {
        let ty = sanitize_type(&phone.label, "cell");
        lines.push(format!("TEL;TYPE={ty}:{}", escape(&phone.value)));
    }
    for email in &card.emails {
        let ty = sanitize_type(&email.label, "home");
        lines.push(format!("EMAIL;TYPE={ty}:{}", escape(&email.value)));
    }
    for addr in &card.addresses {
        let ty = sanitize_type(&addr.label, "home");
        let adr = format!(
            ";;{};{};{};{};{}",
            escape(&addr.value.street),
            escape(&addr.value.city),
            escape(&addr.value.state),
            escape(&addr.value.postal_code),
            escape(&addr.value.country)
        );
        lines.push(format!("ADR;TYPE={ty}:{adr}"));
    }
    if let Some(bday) = &card.birthday {
        if let Some(year) = bday.year {
            lines.push(format!("BDAY:{year:04}-{:02}-{:02}", bday.month, bday.day));
        } else {
            lines.push(format!("BDAY:--{:02}-{:02}", bday.month, bday.day));
        }
    }
    if let Some(photo) = &card.photo {
        let b64 = STANDARD.encode(photo);
        let media_type = sanitize_type(card.photo_media_type.as_deref().unwrap_or("JPEG"), "JPEG");
        lines.push(format!("PHOTO;ENCODING=b;TYPE={media_type}:{b64}"));
    }
    if !card.note.is_empty() {
        lines.push(format!("NOTE:{}", escape(&card.note)));
    }
    if !card.categories.is_empty() {
        let cats = card
            .categories
            .iter()
            .map(|c| escape(c))
            .collect::<Vec<_>>()
            .join(",");
        lines.push(format!("CATEGORIES:{cats}"));
    }
    for field in &card.unknown_fields {
        lines.push(field.clone());
    }
    lines.push("END:VCARD".into());
    let mut out = String::new();
    for line in lines {
        out.push_str(&fold_line(&line));
        out.push_str("\r\n");
    }
    out
}

/// `given-family.vcf`, sanitized to `[a-z0-9-]`, or `{id}.vcf`.
#[must_use]
pub fn suggested_file_name(card: &Card) -> String {
    let name = [card.given_name.as_str(), card.family_name.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase();
    let sanitized: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    if sanitized.is_empty() {
        format!("{}.vcf", card.local_id)
    } else {
        format!("{sanitized}.vcf")
    }
}

fn parse_str(text: &str, file_name: &str, assign_default_id: bool) -> Option<Card> {
    let unfolded = unfold(text);
    if !unfolded
        .lines()
        .any(|l| l.to_ascii_uppercase().starts_with("BEGIN:VCARD"))
    {
        return None;
    }
    let mut card = Card::new(file_name);
    if assign_default_id {
        card.local_id = Uuid::new_v4().to_string();
    }
    let mut groups = std::collections::HashMap::<String, GroupTarget>::new();
    for line in unfolded.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let upper = trimmed.to_ascii_uppercase();
        if upper == "BEGIN:VCARD" || upper == "END:VCARD" || upper.starts_with("VERSION:") {
            continue;
        }
        let Some((group, field, params, value)) = parse_line(trimmed) else {
            card.unknown_fields.push(trimmed.to_owned());
            continue;
        };
        match field.to_ascii_uppercase().as_str() {
            "FN" => card.full_name = unescape(&value),
            "N" => {
                let parts = split_escaped(&value, ';');
                card.family_name = unescape(parts.first().copied().unwrap_or(""));
                card.given_name = unescape(parts.get(1).copied().unwrap_or(""));
                card.middle_name = unescape(parts.get(2).copied().unwrap_or(""));
                card.name_prefix = unescape(parts.get(3).copied().unwrap_or(""));
                card.name_suffix = unescape(parts.get(4).copied().unwrap_or(""));
            }
            "TEL" => {
                card.phones.push(Labeled {
                    label: extract_type_label(&params, "mobile"),
                    value: unescape(&value),
                });
                if let Some(group) = group {
                    groups.insert(group, GroupTarget::Phone(card.phones.len() - 1));
                }
            }
            "EMAIL" => {
                card.emails.push(Labeled {
                    label: extract_type_label(&params, "home"),
                    value: unescape(&value),
                });
                if let Some(group) = group {
                    groups.insert(group, GroupTarget::Email(card.emails.len() - 1));
                }
            }
            "ADR" => {
                let parts = split_escaped(&value, ';');
                card.addresses.push(LabeledAddress {
                    label: extract_type_label(&params, "home"),
                    value: PostalAddress {
                        street: unescape(parts.get(2).copied().unwrap_or("")),
                        city: unescape(parts.get(3).copied().unwrap_or("")),
                        state: unescape(parts.get(4).copied().unwrap_or("")),
                        postal_code: unescape(parts.get(5).copied().unwrap_or("")),
                        country: unescape(parts.get(6).copied().unwrap_or("")),
                    },
                });
            }
            "ORG" => {
                card.organization =
                    unescape(split_escaped(&value, ';').first().copied().unwrap_or(""));
            }
            "TITLE" => card.job_title = unescape(&value),
            "NICKNAME" => card.nickname = unescape(&value),
            "URL" => {
                card.urls.push(Labeled {
                    label: extract_type_label(&params, "homepage"),
                    value: unescape(&value),
                });
                if let Some(group) = group {
                    groups.insert(group, GroupTarget::Url(card.urls.len() - 1));
                }
            }
            "X-ABLABEL" => {
                let label = unescape(&value);
                let applied = group.as_ref().is_some_and(|name| groups.contains_key(name));
                if applied {
                    apply_group_label(&mut card, &groups, group.as_deref().unwrap_or(""), &label);
                } else {
                    card.unknown_fields.push(trimmed.to_owned());
                }
            }
            "BDAY" => card.birthday = parse_birthday(&value),
            "PHOTO" => match parse_photo(&value, &params) {
                Some(bytes) => {
                    card.photo = Some(bytes);
                    card.photo_media_type = Some(extract_photo_media_type(&params));
                }
                None => card.unknown_fields.push(trimmed.to_owned()),
            },
            "NOTE" => card.note = unescape(&value),
            "CATEGORIES" => {
                card.categories = split_escaped(&value, ',')
                    .into_iter()
                    .map(|s| unescape(s.trim()))
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            "X-LOCALCONTACTS-ID" => card.local_id = value,
            _ => card.unknown_fields.push(trimmed.to_owned()),
        }
    }
    lift_item_dump_from_note(&mut card);
    Some(card)
}

enum GroupTarget {
    Url(usize),
    Phone(usize),
    Email(usize),
}

fn apply_group_label(
    card: &mut Card,
    groups: &std::collections::HashMap<String, GroupTarget>,
    group: &str,
    label: &str,
) {
    let label = label.trim();
    if label.is_empty() {
        return;
    }
    match groups.get(group) {
        Some(GroupTarget::Url(index)) => {
            if let Some(url) = card.urls.get_mut(*index) {
                url.label = label.to_owned();
            }
        }
        Some(GroupTarget::Phone(index)) => {
            if let Some(phone) = card.phones.get_mut(*index) {
                phone.label = label.to_owned();
            }
        }
        Some(GroupTarget::Email(index)) => {
            if let Some(email) = card.emails.get_mut(*index) {
                email.label = label.to_owned();
            }
        }
        None => {}
    }
}

/// Apple Contacts sometimes dumps grouped URL/label pairs into NOTE.
fn lift_item_dump_from_note(card: &mut Card) {
    if card.note.is_empty() {
        return;
    }
    let mut kept = Vec::new();
    let mut dumped = String::new();
    for line in card.note.lines() {
        let trimmed = line.trim();
        let upper = trimmed.to_ascii_uppercase();
        let looks_like_item = upper.contains("ITEM")
            && (upper.contains(".URL:")
                || upper.contains(".X-ABLABEL:")
                || upper.contains("URL:")
                || upper.contains("X-ABLABEL:"));
        if looks_like_item {
            dumped.push_str(trimmed);
            dumped.push('\n');
        } else if !trimmed.is_empty() {
            kept.push(trimmed.to_owned());
        }
    }
    if dumped.is_empty() {
        return;
    }
    let wrapped = format!("BEGIN:VCARD\r\nVERSION:3.0\r\nFN:_\r\n{dumped}END:VCARD\r\n");
    let Some(partial) = parse_without_note_lift(wrapped.as_bytes(), "note.vcf") else {
        return;
    };
    for url in partial.urls {
        if !card.urls.iter().any(|existing| existing.value == url.value) {
            card.urls.push(url);
        }
    }
    card.note = kept.join("\n");
}

fn parse_without_note_lift(bytes: &[u8], file_name: &str) -> Option<Card> {
    let text = std::str::from_utf8(bytes).ok()?;
    let unfolded = unfold(text);
    let mut card = Card::new(file_name);
    let mut groups = std::collections::HashMap::<String, GroupTarget>::new();
    for line in unfolded.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let upper = trimmed.to_ascii_uppercase();
        if upper == "BEGIN:VCARD" || upper == "END:VCARD" || upper.starts_with("VERSION:") {
            continue;
        }
        let Some((group, field, params, value)) = parse_line(trimmed) else {
            continue;
        };
        match field.to_ascii_uppercase().as_str() {
            "URL" => {
                card.urls.push(Labeled {
                    label: extract_type_label(&params, "homepage"),
                    value: unescape(&value),
                });
                if let Some(group) = group {
                    groups.insert(group, GroupTarget::Url(card.urls.len() - 1));
                }
            }
            "X-ABLABEL" => {
                if let Some(group) = group {
                    apply_group_label(&mut card, &groups, &group, &unescape(&value));
                }
            }
            _ => {}
        }
    }
    Some(card)
}

fn parse_line(line: &str) -> Option<(Option<String>, String, Vec<String>, String)> {
    let mut working = line.to_owned();
    let mut group = None;
    if let Some(dot) = working.find('.') {
        if let Some(colon) = working.find(':') {
            if dot < colon && !working[..dot].contains(';') {
                group = Some(working[..dot].to_owned());
                working = working[dot + 1..].to_owned();
            }
        }
    }
    let colon = working.find(':')?;
    let field_and_params = &working[..colon];
    let value = working[colon + 1..].to_owned();
    let mut parts = field_and_params.split(';');
    let field = parts.next()?.to_owned();
    let params = parts.map(str::to_owned).collect();
    Some((group, field, params, value))
}

fn unfold(text: &str) -> String {
    text.replace("\r\n ", "")
        .replace("\r\n\t", "")
        .replace("\n ", "")
        .replace("\n\t", "")
}

fn unescape(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => result.push('\n'),
                Some(escaped @ ('\\' | ',' | ';')) => result.push(escaped),
                Some(other) => result.push(other),
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn split_escaped(text: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut escaped = false;
    for (index, character) in text.char_indices() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == delimiter {
            parts.push(&text[start..index]);
            start = index + character.len_utf8();
        }
    }
    parts.push(&text[start..]);
    parts
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace(',', "\\,")
        .replace(';', "\\;")
}

fn extract_type_label(params: &[String], default: &str) -> String {
    const BARE: &[&str] = &[
        "HOME", "WORK", "CELL", "MOBILE", "FAX", "PAGER", "MAIN", "IPHONE", "OTHER",
    ];
    for param in params {
        let upper = param.to_ascii_uppercase();
        if let Some(_rest) = upper.strip_prefix("TYPE=") {
            // TYPE= is case-insensitive; values come from the original param.
            let original = param.split_once('=').map(|(_, v)| v).unwrap_or("");
            for ty in original.split(',') {
                let t = ty.trim().to_ascii_lowercase();
                if t != "pref" && t != "voice" && t != "internet" {
                    return t;
                }
            }
        }
        if BARE.contains(&upper.as_str()) {
            return param.to_ascii_lowercase();
        }
    }
    default.to_owned()
}

fn parse_birthday(value: &str) -> Option<Birthday> {
    let cleaned: String = value.chars().filter(|c| *c != '-').collect();
    if value.starts_with("--") && cleaned.len() == 4 {
        let month = cleaned[..2].parse().ok()?;
        let day = cleaned[2..].parse().ok()?;
        return Some(Birthday {
            year: None,
            month,
            day,
        });
    }
    if cleaned.len() == 8 && cleaned.bytes().all(|b| b.is_ascii_digit()) {
        let year = cleaned[..4].parse().ok()?;
        let month = cleaned[4..6].parse().ok()?;
        let day = cleaned[6..].parse().ok()?;
        return Some(Birthday {
            year: Some(year),
            month,
            day,
        });
    }
    None
}

fn parse_photo(value: &str, params: &[String]) -> Option<Vec<u8>> {
    let is_b64 = params.iter().any(|p| {
        let u = p.to_ascii_uppercase();
        u.contains("BASE64") || u.contains("ENCODING=B") || u.contains("ENCODING=BASE64")
    });
    if !is_b64 {
        return None;
    }
    let cleaned: String = value.chars().filter(|c| !c.is_whitespace()).collect();
    STANDARD.decode(cleaned).ok()
}

fn extract_photo_media_type(params: &[String]) -> String {
    for param in params {
        if let Some((name, value)) = param.split_once('=') {
            if name.eq_ignore_ascii_case("TYPE") {
                if let Some(media_type) = value.split(',').find(|part| !part.trim().is_empty()) {
                    return media_type.trim().to_owned();
                }
            }
        }
    }
    "JPEG".to_owned()
}

fn sanitize_type(label: &str, default: &str) -> String {
    let filtered: String = label
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if filtered.is_empty() {
        default.to_owned()
    } else {
        filtered
    }
}

/// RFC 6350 §3.2: 75 octets excluding CRLF; continuations start with SPACE.
fn fold_line(line: &str) -> String {
    let bytes = line.as_bytes();
    if bytes.len() <= 75 {
        return line.to_owned();
    }
    let mut result = Vec::new();
    let mut offset = 0;
    let mut is_first = true;
    while offset < bytes.len() {
        let limit = if is_first { 75 } else { 74 };
        let mut end = (offset + limit).min(bytes.len());
        while end > offset && end < bytes.len() && bytes[end] & 0xC0 == 0x80 {
            end -= 1;
        }
        if end == offset {
            end = (offset + 1).min(bytes.len());
        }
        if !is_first {
            result.extend_from_slice(b"\r\n ");
        }
        result.extend_from_slice(&bytes[offset..end]);
        offset = end;
        is_first = false;
    }
    String::from_utf8(result).unwrap_or_else(|_| line.to_owned())
}
