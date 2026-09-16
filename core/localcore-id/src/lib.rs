//! Deterministic, path-derived photo/folder identity.
//!
//! Lives here (`localcore-id`). Byte-for-byte mirror of `StableUUID.derive(from:)`
//! in `LocalGallery/Models/PhotoFile.swift`. Both implementations NFC-normalise
//! the path (Swift: `precomposedStringWithCanonicalMapping`; Rust:
//! [`unicode_normalization::UnicodeNormalization::nfc`], the same fold as
//! `gallery-model::text::nfc`) **before** SHA-256. Both are pinned against the
//! same conformance vectors
//! (`LocalGalleryTests/Support/Fixtures/stable_uuid_vectors.json`), so a change
//! to either one without the other fails `cargo test` *and* `xcodebuild test`.
//!
//! NFC is M1 (ADR 0002 R4): identity hashes NFC-normalised UTF-8 path bytes.
//! NFC and NFD spellings of one visible name therefore share one UUID.

use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

/// SHA-256-truncated deterministic UUID with RFC 4122 variant + version-5 marker.
///
/// The path is NFC-normalised before hashing (ADR 0002 R4 / M1). Version and
/// variant nibbles stay as they were: first 16 digest bytes, then stamp
/// version 5 and RFC 4122 variant.
///
/// Namespace-less: this is *not* an RFC 4122 name-based UUID (no namespace is
/// mixed in), it only wears the version/variant markers — so do not reach for
/// `Uuid::new_v5`, it would produce different bytes.
///
/// URL standardization (`..` segments, trailing slashes) is the caller's job.
/// Swift feeds in `url.standardized.path`. Unicode NFC is applied here, so
/// NFC and NFD spellings of the same visible name derive the same UUID.
pub fn derive(input: &str) -> Uuid {
    let nfc: String = input.nfc().collect();
    let digest = Sha256::digest(nfc.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0F) | 0x50; // version 5
    bytes[8] = (bytes[8] & 0x3F) | 0x80; // variant RFC 4122
    Uuid::from_bytes(bytes)
}

/// Map a path stored under the pre-M1 (byte-exact) identity rule to the
/// post-M1 NFC-normalised id.
///
/// Caches keyed by the old UUID — thumbnails, widget JSON, `library_cache`
/// ids — cannot be rewritten from the old UUID alone. They must be rebuilt
/// from the stored path: this function is that mapping. This crate does not
/// rewrite on-disk gallery caches.
///
/// Release note: thumbnail files named by the old UUID are orphans until the
/// next scan/rebuild.
pub fn migrate_id(old_path_bytes_as_stored: &str) -> Uuid {
    derive(old_path_bytes_as_stored)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_deterministic() {
        let path = "/Volumes/Library/2024/IMG_0001.jpg";
        assert_eq!(derive(path), derive(path));
    }

    #[test]
    fn differs_across_inputs() {
        assert_ne!(derive("/a/IMG_0001.jpg"), derive("/a/IMG_0002.jpg"));
    }

    #[test]
    fn stamps_version_and_variant_markers() {
        let bytes = *derive("/library/x.jpg").as_bytes();
        assert_eq!(bytes[6] & 0xF0, 0x50, "version-5 nibble must be 5");
        assert_eq!(bytes[8] & 0xC0, 0x80, "RFC 4122 variant must be 10xx");
    }

    #[test]
    fn renders_lowercase_hyphenated() {
        let rendered = derive("/library/x.jpg").to_string();
        assert_eq!(rendered.len(), 36);
        assert_eq!(rendered, rendered.to_lowercase());
    }

    #[test]
    fn nfc_and_nfd_cafe_share_one_id() {
        assert_eq!(
            derive("/lib/caf\u{00E9}.jpg"),
            derive("/lib/cafe\u{0301}.jpg")
        );
    }

    #[test]
    fn ascii_path_keeps_the_pre_m1_vector() {
        // NFC is a no-op on ASCII, so the pre-M1 vector stays valid.
        assert_eq!(
            derive("/Users/j/Pictures/2024/IMG_0001.jpg").to_string(),
            "8f06fa91-04be-532f-b171-92d1943df4b2"
        );
    }
}
