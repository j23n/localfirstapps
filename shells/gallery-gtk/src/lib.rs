//! Phase 5.3 pin crate.
//!
//! This member exists so `shells/Cargo.lock` resolves `gallery-ffi` and
//! leftover `localgallery` (`default-features = false`) and so
//! `cargo tree -d` can prove one `image` / `uniffi` and no default-graph
//! `ort`. The kit shell (folder picker, Folders · Collections · Photos,
//! Settings dialog) is Phase 5.5 — do not add GTK or `shell-kit-gtk` here.

pub use gallery_ffi::{LibraryIndex, ViewContentState, ViewStructure};
pub use localgallery::Config;

/// Touch both path deps so a missing pin fails the shells workspace build.
pub fn pin_probe() -> String {
    let _ = Config::default();
    gallery_ffi::core_version()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gallery_ffi::ViewError;

    #[test]
    fn pin_probe_touches_both_path_deps() {
        assert!(!pin_probe().is_empty());
    }

    #[test]
    fn location_windows_compile_from_the_shells_lockfile() {
        let index = LibraryIndex::new();
        let folders = index.folder_structure(None);
        assert_eq!(folders.state, ViewContentState::Empty);
        assert_eq!(folders.sections[0].id, "folders");
        assert!(index
            .folder_window("folders".into(), 0, 8, folders.generation)
            .unwrap()
            .is_empty());

        let people = index.people_structure();
        assert_eq!(people.sections[0].id, "people");
        assert!(index
            .people_window("people".into(), 0, 8, people.generation)
            .unwrap()
            .is_empty());

        let collections = index.collection_structure();
        assert!(collections.sections.is_empty());
        assert!(matches!(
            index.collection_window("places".into(), 0, 8, collections.generation),
            Err(ViewError::SectionNotFound { .. })
        ));
    }
}
