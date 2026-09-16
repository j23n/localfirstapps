//! ADR 0005 R19: pre-M1 identity state survives the NFC re-key.
//!
//! The fixture holds a path → old UUID mapping from the byte-exact hasher.
//! Post-M1, `derive(nfd) == derive(nfc) ==` the new id, and `migrate_id`
//! is how caches keyed by the old UUID are rebuilt from the stored path.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
struct Fixture {
    entries: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    label: String,
    path: String,
    nfc_twin: String,
    old_uuid: String,
}

fn fixture() -> Fixture {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/m1-pre/pre_m1_ids.json");
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    serde_json::from_str(&json).expect("m1-pre fixture is not valid JSON")
}

#[test]
fn pre_m1_nfd_path_rekeys_to_the_nfc_id() {
    let fixture = fixture();
    assert!(
        !fixture.entries.is_empty(),
        "m1-pre fixture must hold at least one NFD path"
    );
    for entry in &fixture.entries {
        assert_ne!(
            entry.path, entry.nfc_twin,
            "{}: fixture path was normalized against its NFC twin",
            entry.label
        );
        let new_from_stored = localcore_id::derive(&entry.path);
        let new_from_nfc = localcore_id::derive(&entry.nfc_twin);
        let migrated = localcore_id::migrate_id(&entry.path);
        assert_eq!(
            new_from_stored, new_from_nfc,
            "{}: derive(nfd) must equal derive(nfc)",
            entry.label
        );
        assert_eq!(
            migrated, new_from_nfc,
            "{}: migrate_id(old_path_bytes_as_stored) must be the new id",
            entry.label
        );
        assert_ne!(
            entry.old_uuid,
            new_from_nfc.to_string(),
            "{}: fixture must record the pre-M1 UUID, not the post-M1 one",
            entry.label
        );
        assert_ne!(
            entry.old_uuid,
            migrated.to_string(),
            "{}: caches keyed by old_uuid cannot be reused; rebuild from the path",
            entry.label
        );
    }
}
