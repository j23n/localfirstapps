//! Where a photo's `.xmp` sidecar lives (schema §1.4).

use gallery_vfs::{Vfs, VfsResult};

/// `IMG_1234.jpg` → `IMG_1234.jpg.xmp`.
///
/// The extension is *appended*, not replaced — the MWG / digiKam convention —
/// so `IMG_1234.jpg` and `IMG_1234.heic` get distinct sidecars. This is also
/// the only form `MetadataReader.readXMPSidecar` looks for, so anything else
/// would be invisible to the app.
pub fn sidecar_path(image_path: &str) -> String {
    format!("{image_path}.xmp")
}

/// `IMG_1234.jpg` → `IMG_1234.xmp`, the Lightroom / Capture One convention.
///
/// **Read-side only** (schema §1.4). `None` when the input has no extension to
/// replace, or already is a `.xmp`.
pub fn alt_sidecar_path(image_path: &str) -> Option<String> {
    let file_start = image_path.rfind('/').map(|i| i + 1).unwrap_or(0);
    let dot = image_path[file_start..].rfind('.')? + file_start;
    // A leading dot is a hidden file, not an extension: `.hidden` would
    // otherwise yield `<dir>/.xmp`, a single file *shared by every dotfile in
    // the directory*. Reading it would attribute one photo's metadata to
    // another, and a future write side would have them overwrite each other.
    if dot == file_start {
        return None;
    }
    if image_path[dot..].eq_ignore_ascii_case(".xmp") {
        return None;
    }
    Some(format!("{}.xmp", &image_path[..dot]))
}

/// Appended form first, then the Lightroom alt.
///
/// Only [`gallery_vfs::VfsError::NotFound`] means absence; permission and I/O
/// failures propagate to the caller.
pub fn sidecar_exists(vfs: &dyn Vfs, image_path: &str) -> VfsResult<bool> {
    let canonical = sidecar_path(image_path);
    if vfs.try_exists(&canonical)? {
        return Ok(true);
    }
    match alt_sidecar_path(image_path) {
        Some(path) => vfs.try_exists(&path),
        None => Ok(false),
    }
}

/// Bytes of the sidecar that exists, appended form preferred.
pub fn read_sidecar_bytes(vfs: &dyn Vfs, image_path: &str) -> Option<Vec<u8>> {
    let canonical = sidecar_path(image_path);
    if let Ok(bytes) = vfs.read(&canonical) {
        return Some(bytes);
    }
    alt_sidecar_path(image_path).and_then(|p| vfs.read(&p).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_form_preserves_the_image_suffix() {
        assert_eq!(sidecar_path("/a/IMG_1234.jpg"), "/a/IMG_1234.jpg.xmp");
        assert_eq!(sidecar_path("/a/IMG_1234.heic"), "/a/IMG_1234.heic.xmp");
    }

    #[test]
    fn alt_form_replaces_the_suffix() {
        assert_eq!(
            alt_sidecar_path("/a/IMG_1234.jpg").as_deref(),
            Some("/a/IMG_1234.xmp")
        );
    }

    #[test]
    fn alt_form_is_none_when_there_is_nothing_to_replace() {
        assert_eq!(alt_sidecar_path("/a/IMG_1234"), None);
        assert_eq!(alt_sidecar_path("/a/IMG_1234.xmp"), None);
        assert_eq!(alt_sidecar_path("/a/IMG_1234.XMP"), None);
    }

    #[test]
    fn a_dotfile_has_no_extension_to_replace() {
        // `/a/.xmp` would be shared by every dotfile in the directory.
        assert_eq!(alt_sidecar_path("/a/.hidden"), None);
        assert_eq!(alt_sidecar_path(".hidden"), None);
        // A dotfile that does have an extension still works.
        assert_eq!(
            alt_sidecar_path("/a/.hidden.jpg").as_deref(),
            Some("/a/.hidden.xmp")
        );
    }

    #[test]
    fn a_dot_in_a_parent_directory_is_not_an_extension() {
        assert_eq!(alt_sidecar_path("/a.b/IMG_1234"), None);
        assert_eq!(
            alt_sidecar_path("/a.b/IMG_1234.jpg").as_deref(),
            Some("/a.b/IMG_1234.xmp")
        );
    }

    #[test]
    fn sidecar_exists_prefers_the_appended_form() {
        let vfs = gallery_vfs::MemVfs::new();
        vfs.insert("/lib/a.jpg.xmp", b"<x/>".to_vec());
        vfs.insert("/lib/a.xmp", b"<alt/>".to_vec());
        assert!(sidecar_exists(&vfs, "/lib/a.jpg").unwrap());
        assert_eq!(
            read_sidecar_bytes(&vfs, "/lib/a.jpg").as_deref(),
            Some(&b"<x/>"[..])
        );
    }

    #[test]
    fn sidecar_exists_falls_back_to_the_lightroom_alt() {
        let vfs = gallery_vfs::MemVfs::new();
        vfs.insert("/lib/a.xmp", b"<alt/>".to_vec());
        assert!(sidecar_exists(&vfs, "/lib/a.jpg").unwrap());
        assert_eq!(
            read_sidecar_bytes(&vfs, "/lib/a.jpg").as_deref(),
            Some(&b"<alt/>"[..])
        );
    }

    #[test]
    fn a_photo_with_no_sidecar_file_is_missing() {
        let vfs = gallery_vfs::MemVfs::new();
        vfs.insert("/lib/a.jpg", b"jpeg".to_vec());
        assert!(!sidecar_exists(&vfs, "/lib/a.jpg").unwrap());
        assert_eq!(read_sidecar_bytes(&vfs, "/lib/a.jpg"), None);
    }

    #[test]
    fn permission_denied_is_not_reported_as_an_absent_sidecar() {
        struct DeniedVfs(gallery_vfs::MemVfs);

        impl Vfs for DeniedVfs {
            fn open(
                &self,
                path: &str,
            ) -> gallery_vfs::VfsResult<Box<dyn gallery_vfs::ReadSeek + Send>> {
                self.0.open(path)
            }

            fn stat(&self, path: &str) -> gallery_vfs::VfsResult<gallery_vfs::Stat> {
                if path == "/lib/a.jpg.xmp" {
                    return Err(gallery_vfs::VfsError::PermissionDenied {
                        path: path.to_string(),
                    });
                }
                self.0.stat(path)
            }

            fn list(&self, dir: &str) -> gallery_vfs::VfsResult<Vec<gallery_vfs::Entry>> {
                self.0.list(dir)
            }

            fn stat_entry(&self, path: &str) -> gallery_vfs::VfsResult<gallery_vfs::Entry> {
                self.0.stat_entry(path)
            }

            fn write_atomic(&self, path: &str, bytes: &[u8]) -> gallery_vfs::VfsResult<()> {
                self.0.write_atomic(path, bytes)
            }

            fn exists(&self, path: &str) -> bool {
                self.0.exists(path)
            }
        }

        let err = sidecar_exists(&DeniedVfs(gallery_vfs::MemVfs::new()), "/lib/a.jpg").unwrap_err();
        assert!(matches!(
            err,
            gallery_vfs::VfsError::PermissionDenied { path }
                if path == "/lib/a.jpg.xmp"
        ));
    }
}
