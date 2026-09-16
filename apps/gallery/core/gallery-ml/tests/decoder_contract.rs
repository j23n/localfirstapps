//! Contract for the one analysis-decode seam shared by tagging and faces.

use std::sync::atomic::{AtomicUsize, Ordering};

use gallery_ml::{
    decode_for_analysis, HostHeicDecoder, MlError, MlResult, RgbImage, ANALYSIS_MAX_LONG_SIDE,
};
use gallery_vfs::MemVfs;
use image::{DynamicImage, ImageFormat};

struct Host {
    calls: AtomicUsize,
    fails: bool,
    size: (u32, u32),
}

impl HostHeicDecoder for Host {
    fn decode(&self, path: &str) -> MlResult<RgbImage> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.fails {
            return Err(MlError::Preprocess {
                path: path.to_string(),
                code: gallery_ml::ErrorCode::Decode,
                detail: "host refused fixture".into(),
            });
        }
        Ok(RgbImage::from_pixel(
            self.size.0,
            self.size.1,
            image::Rgb([1, 2, 3]),
        ))
    }
}

fn png(rgb: [u8; 3]) -> Vec<u8> {
    let mut bytes = Vec::new();
    DynamicImage::ImageRgb8(RgbImage::from_pixel(2, 1, image::Rgb(rgb)))
        .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    bytes
}

#[test]
fn the_host_port_is_heic_only_and_its_failure_is_final() {
    let vfs = MemVfs::new();
    vfs.insert("/lib/not-really.heic", png([9, 8, 7]));
    vfs.insert("/lib/photo.jpg", png([6, 5, 4]));
    let failing = Host {
        calls: AtomicUsize::new(0),
        fails: true,
        size: (1, 1),
    };

    let err =
        decode_for_analysis(&vfs, Some(&failing), "/lib/not-really.heic", &[7; 32]).unwrap_err();
    assert!(matches!(
        err,
        MlError::Preprocess {
            code: gallery_ml::ErrorCode::Decode,
            ..
        }
    ));
    assert_eq!(failing.calls.load(Ordering::Relaxed), 1);

    let (rgb, _) = decode_for_analysis(&vfs, Some(&failing), "/lib/photo.jpg", &[8; 32]).unwrap();
    assert_eq!(rgb.get_pixel(0, 0).0, [6, 5, 4]);
    assert_eq!(
        failing.calls.load(Ordering::Relaxed),
        1,
        "a non-HEIC path reached the hardware port"
    );
}

#[test]
fn the_software_path_hashes_the_bytes_it_actually_decodes() {
    let vfs = MemVfs::new();
    let bytes = png([3, 4, 5]);
    vfs.insert("/lib/wrong-extension.jpg", bytes.clone());
    let stale_probe = [0xAA; 32];

    let (rgb, hash) =
        decode_for_analysis(&vfs, None, "/lib/wrong-extension.jpg", &stale_probe).unwrap();
    assert_eq!(rgb.get_pixel(0, 0).0, [3, 4, 5]);
    assert_eq!(hash, gallery_ml::hash::hash_bytes(&bytes));
    assert_ne!(hash, stale_probe);
}

#[test]
fn the_host_output_is_capped_at_the_shared_analysis_size() {
    let vfs = MemVfs::new();
    let host = Host {
        calls: AtomicUsize::new(0),
        fails: false,
        size: (ANALYSIS_MAX_LONG_SIDE * 2, ANALYSIS_MAX_LONG_SIDE),
    };

    let (rgb, key) = decode_for_analysis(&vfs, Some(&host), "/lib/photo.heic", &[9; 32]).unwrap();
    assert_eq!(
        (rgb.width(), rgb.height()),
        (ANALYSIS_MAX_LONG_SIDE, ANALYSIS_MAX_LONG_SIDE / 2)
    );
    assert_eq!(key, [9; 32]);
}
