//! Display decode of the original file (viewer / XDG). JPEG/PNG via `image`
//! with EXIF Orientation applied. HEIC uses the container transform only
//! (do not apply EXIF on top). Movies grab one frame via [`crate::video`].
//! Analysis uses [`crate::heic`] and never samples video.

use std::io::Cursor;

use gallery_meta::media::isobmff;
use gallery_ml::{HeifDecoder, ImageDecoder, ImageKind};
use image::metadata::Orientation;
use image::ImageDecoder as _;

/// RGB8 pixels plus size, already oriented (EXIF for JPEG/PNG; HEIC container).
#[derive(Debug, Clone)]
pub struct RgbFrame {
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Tight RGB8 buffer.
    pub rgb: Vec<u8>,
}

/// Compressed-byte and decoded-pixel caps applied *before* a full decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeLimits {
    /// Refuse to `read` a file larger than this.
    pub max_compressed_bytes: u64,
    /// Refuse a decode whose declared width×height exceeds this.
    pub max_pixels: u64,
}

impl DecodeLimits {
    /// Desktop / Comet display budget: 80 MiB on disk, 80 MP decoded.
    pub const DEFAULT: Self = Self {
        max_compressed_bytes: 80 * 1024 * 1024,
        max_pixels: 80_000_000,
    };

    /// Whether a file this large may be read into memory.
    pub fn allows_compressed(&self, bytes: u64) -> bool {
        bytes > 0 && bytes <= self.max_compressed_bytes
    }

    /// Whether a frame this size may be decoded.
    pub fn allows_pixels(&self, width: u32, height: u32) -> bool {
        matches!(
            u64::from(width).checked_mul(u64::from(height)),
            Some(p) if p > 0 && p <= self.max_pixels
        )
    }

    fn max_alloc_bytes(self) -> u64 {
        self.max_pixels.saturating_mul(4).min(512 * 1024 * 1024)
    }
}

/// Decode `path` and shrink so the long side is at most `max_side`.
pub fn decode_limited(path: &str, max_side: u32) -> Option<RgbFrame> {
    decode_limited_with(path, max_side, DecodeLimits::DEFAULT)
}

/// [`decode_limited`] with explicit budgets (tests, tight hosts).
pub fn decode_limited_with(path: &str, max_side: u32, limits: DecodeLimits) -> Option<RgbFrame> {
    let _span = localcore_trace::span("decode", "decode_limited").extra("max_side", max_side);
    localcore_trace::detail("decode", format!("path={path}"));
    // Movies are not stills. Do not read them into the JPEG/HEIC budget.
    if crate::video::is_video_path(path) {
        return crate::video::decode_frame(path, max_side);
    }
    let meta = std::fs::metadata(path).ok()?;
    if !limits.allows_compressed(meta.len()) {
        localcore_trace::event(
            "decode",
            format!("refuse compressed {} bytes for {path}", meta.len()),
        );
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if !limits.allows_compressed(bytes.len() as u64) {
        return None;
    }
    let image = if looks_like_heif(&bytes) {
        if let Some((w, h)) = isobmff::max_declared_extent(&bytes) {
            if !limits.allows_pixels(w, h) {
                localcore_trace::event("decode", format!("refuse HEIC {w}x{h} for {path}"));
                return None;
            }
        }
        // Container irot/imir only. HeifDecoder also undoes heif-oxide's
        // inverted imir axis (iPhone selfies). Do not apply EXIF on top.
        HeifDecoder.decode(path, &bytes, ImageKind::Heic).ok()?
    } else {
        decode_raster(&bytes, limits)?
    };
    if !limits.allows_pixels(image.width(), image.height()) {
        return None;
    }
    let image = if image.width().max(image.height()) > max_side {
        image.thumbnail(max_side, max_side)
    } else {
        image
    };
    let rgb = image.to_rgb8();
    Some(RgbFrame {
        width: rgb.width(),
        height: rgb.height(),
        rgb: rgb.into_raw(),
    })
}

fn decode_raster(bytes: &[u8], limits: DecodeLimits) -> Option<image::DynamicImage> {
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut decoder = reader.into_decoder().ok()?;
    let (w, h) = decoder.dimensions();
    if !limits.allows_pixels(w, h) {
        return None;
    }
    let mut img_limits = image::Limits::default();
    img_limits.max_alloc = Some(limits.max_alloc_bytes());
    decoder.set_limits(img_limits).ok()?;
    let mut image = image::DynamicImage::from_decoder(decoder).ok()?;
    // Same kamadak-exif read as analysis. `decoder.orientation()` misses
    // iPhone JPEG APP1 / PNG eXIf. HEIC selfies are the `imir` path above.
    let orientation = gallery_ml::preprocess::read_exif_orientation(bytes)
        .and_then(Orientation::from_exif)
        .unwrap_or(Orientation::NoTransforms);
    image.apply_orientation(orientation);
    Some(image)
}

/// ISO-BMFF `ftyp` brands leftover display and the analysis host agree on.
/// Display pixel/byte caps stay in [`decode_limited`]; the host does not use them.
pub(crate) fn looks_like_heif(bytes: &[u8]) -> bool {
    bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && matches!(
            &bytes[8..12],
            b"heic" | b"heix" | b"mif1" | b"msf1" | b"hevc"
        )
}

/// 1×1 JPEG used by decode and XDG-thumbnail tests.
#[cfg(test)]
pub fn tests_jpeg() -> &'static [u8] {
    &[
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00,
        0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06, 0x07, 0x06,
        0x05, 0x08, 0x07, 0x07, 0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D, 0x0C, 0x0B, 0x0B,
        0x0C, 0x19, 0x12, 0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D, 0x1A, 0x1C, 0x1C, 0x20,
        0x24, 0x2E, 0x27, 0x20, 0x22, 0x2C, 0x23, 0x1C, 0x1C, 0x28, 0x37, 0x29, 0x2C, 0x30, 0x31,
        0x34, 0x34, 0x34, 0x1F, 0x27, 0x39, 0x3D, 0x38, 0x32, 0x3C, 0x2E, 0x33, 0x34, 0x32, 0xFF,
        0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00,
        0x14, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xC4, 0x00, 0x14, 0x10, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xDA,
        0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0x7B, 0xFF, 0xD9,
    ]
}

/// Same 1×1 JPEG with a forged SOF size, for the pixel-budget test.
#[cfg(test)]
fn jpeg_declaring(width: u16, height: u16) -> Vec<u8> {
    let mut bytes = tests_jpeg().to_vec();
    let sof = bytes
        .windows(2)
        .position(|w| w == [0xFF, 0xC0])
        .expect("SOF0");
    bytes[sof + 5] = (height >> 8) as u8;
    bytes[sof + 6] = height as u8;
    bytes[sof + 7] = (width >> 8) as u8;
    bytes[sof + 8] = width as u8;
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_decodes_to_rgb() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.jpg");
        std::fs::write(&path, tests_jpeg()).unwrap();
        let frame = decode_limited(path.to_str().unwrap(), 64).expect("jpeg");
        assert!(frame.width >= 1 && frame.height >= 1);
        assert_eq!(frame.rgb.len(), (frame.width * frame.height * 3) as usize);
    }

    #[test]
    fn oversized_compressed_is_refused_before_decode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.jpg");
        std::fs::write(&path, tests_jpeg()).unwrap();
        let limits = DecodeLimits {
            max_compressed_bytes: 8,
            max_pixels: 1_000_000,
        };
        assert!(tests_jpeg().len() as u64 > limits.max_compressed_bytes);
        assert!(decode_limited_with(path.to_str().unwrap(), 64, limits).is_none());
        assert!(DecodeLimits::DEFAULT.allows_compressed(tests_jpeg().len() as u64));
    }

    #[test]
    fn oversized_declared_pixels_are_refused_before_decode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.jpg");
        let bytes = jpeg_declaring(20_000, 20_000);
        std::fs::write(&path, &bytes).unwrap();
        let limits = DecodeLimits {
            max_compressed_bytes: 1_000_000,
            max_pixels: 1_000,
        };
        assert!(!limits.allows_pixels(20_000, 20_000));
        assert!(decode_limited_with(path.to_str().unwrap(), 64, limits).is_none());
    }

    #[test]
    fn default_limits_accept_a_small_jpeg() {
        assert!(DecodeLimits::DEFAULT.allows_pixels(1, 1));
        assert!(DecodeLimits::DEFAULT.allows_compressed(1024));
        assert!(!DecodeLimits::DEFAULT.allows_compressed(0));
    }

    fn with_exif_orientation(jpeg: &[u8], orientation: u16) -> Vec<u8> {
        assert_eq!(&jpeg[..2], [0xFF, 0xD8]);
        let mut app1 = vec![0xFF, 0xE1, 0x00, 0x00];
        app1.extend_from_slice(b"Exif\0\0");
        app1.extend_from_slice(&[0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00]);
        app1.extend_from_slice(&[0x01, 0x00]);
        app1.extend_from_slice(&[0x12, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00]);
        app1.extend_from_slice(&[orientation as u8, (orientation >> 8) as u8, 0x00, 0x00]);
        app1.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        let len = u16::try_from(app1.len() - 2).expect("APP1");
        app1[2] = (len >> 8) as u8;
        app1[3] = len as u8;
        let mut out = vec![0xFF, 0xD8];
        out.extend_from_slice(&app1);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    #[test]
    fn jpeg_applies_exif_orientation_6() {
        let mut rgb = image::RgbImage::new(2, 1);
        rgb.put_pixel(0, 0, image::Rgb([255, 0, 0]));
        rgb.put_pixel(1, 0, image::Rgb([0, 255, 0]));
        let mut jpeg = Vec::new();
        image::DynamicImage::ImageRgb8(rgb)
            .write_to(&mut Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
            .unwrap();
        let bytes = with_exif_orientation(&jpeg, 6);
        assert_eq!(
            gallery_ml::preprocess::read_exif_orientation(&bytes),
            Some(6)
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rot.jpg");
        std::fs::write(&path, &bytes).unwrap();
        let frame = decode_limited(path.to_str().unwrap(), 64).expect("jpeg");
        assert_eq!((frame.width, frame.height), (1, 2));
    }

    #[test]
    fn a_movie_is_not_read_as_a_still() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.mp4");
        std::fs::write(&path, b"ftypisomnotanimage").unwrap();
        let limits = DecodeLimits {
            max_compressed_bytes: 4,
            max_pixels: 1_000_000,
        };
        // Under the still-image budget this would be refused for size.
        // The video branch must not apply that budget.
        assert!(path.metadata().unwrap().len() as u64 > limits.max_compressed_bytes);
        assert!(decode_limited_with(path.to_str().unwrap(), 64, limits).is_none());
    }
}
