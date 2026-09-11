//! Conformance vectors shared with Swift and `gallery-model`.
//!
//! Reads the *same* file `LocalGalleryTests/Support/Fixtures/stable_uuid_vectors.json`
//! that `StableUUIDVectorTests` and `gallery-model` read — there is exactly one
//! copy in the repo, and it is generated from the Swift implementation by
//! `scripts/gen_stable_uuid_vectors.swift`.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
struct Vector {
    label: String,
    input: String,
    uuid: String,
}

fn vectors_path() -> PathBuf {
    // core/localcore-id → walk up until the shared Swift fixture is visible.
    let relative = PathBuf::from_iter([
        "apps",
        "gallery",
        "LocalGalleryTests",
        "Support",
        "Fixtures",
        "stable_uuid_vectors.json",
    ]);
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = dir.join(&relative);
        if candidate.is_file() {
            return candidate;
        }
        if !dir.pop() {
            panic!(
                "could not find {} walking up from {}",
                relative.display(),
                env!("CARGO_MANIFEST_DIR")
            );
        }
    }
}

fn vectors() -> Vec<Vector> {
    let path = vectors_path();
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    serde_json::from_str(&json).expect("stable_uuid_vectors.json is not a vector array")
}

#[test]
fn matches_every_swift_vector() {
    let vectors = vectors();
    assert!(
        vectors.len() >= 30,
        "expected the full vector set, got {}",
        vectors.len()
    );
    for v in &vectors {
        assert_eq!(
            localcore_id::derive(&v.input).to_string(),
            v.uuid,
            "vector `{}` diverged (input {:?})",
            v.label,
            v.input
        );
    }
}

#[test]
fn nfc_and_nfd_are_distinct() {
    // Guards the fixture itself: if an editor or filesystem ever normalizes the
    // JSON, the paired vectors would collapse and the parity claim would be
    // vacuous. Also documents the standing behaviour — no normalization.
    let vectors = vectors();
    for stem in ["cafe", "zurich", "hangul"] {
        let nfc = vectors
            .iter()
            .find(|v| v.label == format!("nfc-{stem}"))
            .unwrap_or_else(|| panic!("missing nfc-{stem} vector"));
        let nfd = vectors
            .iter()
            .find(|v| v.label == format!("nfd-{stem}"))
            .unwrap_or_else(|| panic!("missing nfd-{stem} vector"));
        assert_ne!(nfc.input, nfd.input, "{stem}: fixture was normalized");
        assert_ne!(
            nfc.uuid, nfd.uuid,
            "{stem}: composed/decomposed must differ"
        );
    }
}
