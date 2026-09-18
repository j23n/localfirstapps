//! Preservation and convergence contract for Gallery's Syncthing sidecars.

use std::fs;
use std::path::{Path, PathBuf};

use gallery_meta::{apply_sidecar_conflict, merge_sidecar_conflicts, read_view, SidecarVersion};
use gallery_vfs::{MemVfs, Vfs};

const CANONICAL: &str = "photo.heic.xmp";
const PHONE: &str = "photo.heic.sync-conflict-20260915-101500-PHONE01.xmp";
const LAPTOP: &str = "photo.heic.sync-conflict-20260916-081000-LAPTOP02.xmp";

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/conflicts/syncthing-preservation")
}

fn read(name: &str) -> Vec<u8> {
    fs::read(fixtures().join(name)).unwrap()
}

fn versions<'a>(canonical: &'a [u8], phone: &'a [u8], laptop: &'a [u8]) -> [SidecarVersion<'a>; 3] {
    [
        SidecarVersion {
            path: CANONICAL,
            modified_ns: 100,
            bytes: canonical,
        },
        SidecarVersion {
            path: PHONE,
            modified_ns: 200,
            bytes: phone,
        },
        SidecarVersion {
            path: LAPTOP,
            modified_ns: 300,
            bytes: laptop,
        },
    ]
}

#[test]
fn divergent_human_edits_union_but_old_core_output_does_not_return() {
    let (canonical, phone, laptop) = (read(CANONICAL), read(PHONE), read(LAPTOP));
    let merged = merge_sidecar_conflicts(&versions(&canonical, &phone, &laptop)).unwrap();
    let view = read_view(&merged.bytes).unwrap();

    assert_eq!(merged.newest_path, LAPTOP);
    assert_eq!(
        view.tags_list,
        [
            "Albums/Favorites",
            "Objects/Animal/Dog",
            "People/Family",
            "Trips/Italy",
        ]
    );
    assert_eq!(view.subject, ["Favorite", "Dog", "Family", "Travel"]);
    assert_eq!(
        view.hierarchical_subject,
        [
            "Albums|Favorites",
            "Objects|Animal|Dog",
            "People|Family",
            "Trips|Italy",
        ]
    );
    assert_eq!(view.core.model_pack.as_deref(), Some("new-pack"));
    assert_eq!(view.core.tags, ["Objects/Animal/Dog"]);
    assert!(
        !view.tags_list.iter().any(|tag| tag == "Objects/Animal/Cat"),
        "an older machine-owned prediction leaked back as a human keyword"
    );
}

#[test]
fn unknown_and_divergent_foreign_metadata_is_preserved() {
    let (canonical, phone, laptop) = (read(CANONICAL), read(PHONE), read(LAPTOP));
    let merged = merge_sidecar_conflicts(&versions(&canonical, &phone, &laptop)).unwrap();
    let text = String::from_utf8(merged.bytes).unwrap();

    for needle in [
        "<acme:Rating>5</acme:Rating>",
        "<acme:Rating>3</acme:Rating>",
        "<acme:LensProfile serial='A-17'>hand-tuned</acme:LensProfile>",
        "<vendor:DevelopRecipe>phone-shadow-lift</vendor:DevelopRecipe>",
    ] {
        assert!(
            text.contains(needle),
            "missing preserved metadata: {needle}"
        );
    }
}

#[test]
fn resolution_is_byte_deterministic_for_every_input_order() {
    let (canonical, phone, laptop) = (read(CANONICAL), read(PHONE), read(LAPTOP));
    let inputs = versions(&canonical, &phone, &laptop);
    let expected = merge_sidecar_conflicts(&inputs).unwrap().bytes;

    for order in [[0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
        let permuted = [inputs[order[0]], inputs[order[1]], inputs[order[2]]];
        assert_eq!(
            merge_sidecar_conflicts(&permuted).unwrap().bytes,
            expected,
            "input order {order:?} changed the output"
        );
    }
}

#[test]
fn planning_never_overwrites_or_deletes_any_input() {
    let (canonical, phone, laptop) = (read(CANONICAL), read(PHONE), read(LAPTOP));
    let vfs = MemVfs::new();
    for (name, bytes) in [(CANONICAL, &canonical), (PHONE, &phone), (LAPTOP, &laptop)] {
        vfs.insert(&format!("/library/{name}"), bytes.clone());
    }

    let merge = merge_sidecar_conflicts(&versions(&canonical, &phone, &laptop)).unwrap();
    assert!(!merge.bytes.is_empty());
    for (name, before) in [(CANONICAL, &canonical), (PHONE, &phone), (LAPTOP, &laptop)] {
        assert_eq!(
            vfs.read(&format!("/library/{name}")).unwrap(),
            *before,
            "planning mutated {name}"
        );
    }
    assert!(
        !vfs.exists("/library/photo.heic.merged.xmp"),
        "a pure plan unexpectedly wrote output"
    );
}

#[test]
fn apply_writes_surviving_and_deletes_copies() {
    let (canonical, phone, laptop) = (read(CANONICAL), read(PHONE), read(LAPTOP));
    let vfs = MemVfs::new();
    for (name, bytes) in [(CANONICAL, &canonical), (PHONE, &phone), (LAPTOP, &laptop)] {
        vfs.insert(&format!("/library/{name}"), bytes.clone());
    }

    let merge = merge_sidecar_conflicts(&versions(&canonical, &phone, &laptop)).unwrap();
    apply_sidecar_conflict(
        &vfs,
        &format!("/library/{CANONICAL}"),
        &[format!("/library/{PHONE}"), format!("/library/{LAPTOP}")],
        &merge.bytes,
    )
    .unwrap();

    assert_eq!(
        vfs.read(&format!("/library/{CANONICAL}")).unwrap(),
        merge.bytes
    );
    assert!(!vfs.exists(&format!("/library/{PHONE}")));
    assert!(!vfs.exists(&format!("/library/{LAPTOP}")));
}

#[test]
fn every_fixture_name_has_the_expected_syncthing_shape() {
    assert!(Path::new(CANONICAL)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .ends_with(".xmp"));
    for conflict in [PHONE, LAPTOP] {
        assert!(localcore_conflict::is_conflict_name(conflict), "{conflict}");
    }
}
