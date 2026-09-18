//! Leftover analysis HEIC host. Opens the path itself; pixels stay `heif-oxide`.
//!
//! Viewer and XDG thumbs stay on [`crate::decode`]. This is the
//! [`gallery_ml::HostHeicDecoder`] door Scan Photos installs — not a new codec.

use std::sync::Arc;

use gallery_meta::media::isobmff;
use gallery_ml::{
    ErrorCode, HeifDecoder, HostHeicDecoder, ImageDecoder, ImageKind, MlError, MlResult, RgbImage,
};

use crate::decode::looks_like_heif;

/// Same ceiling as core `HeifDecoder` (`MAX_PIXELS`). Not the viewer 80 MP cap.
const MAX_HEIF_PIXELS: u64 = 120_000_000;

/// Leftover-owned adapter. Software HEVC via [`HeifDecoder`].
pub struct LinuxHeicDecoder;

/// Install the leftover analysis host.
pub fn linux_heic_decoder() -> Arc<dyn HostHeicDecoder> {
    Arc::new(LinuxHeicDecoder)
}

impl HostHeicDecoder for LinuxHeicDecoder {
    fn decode(&self, path: &str) -> MlResult<RgbImage> {
        let fail = |code: ErrorCode, detail: String| MlError::Preprocess {
            path: path.to_string(),
            code,
            detail,
        };

        let bytes = std::fs::read(path).map_err(|e| fail(ErrorCode::Decode, e.to_string()))?;
        if !looks_like_heif(&bytes) {
            return Err(fail(ErrorCode::BadImage, "not a HEIF file".into()));
        }
        if let Some((w, h)) = isobmff::max_declared_extent(&bytes) {
            let pixels = u64::from(w) * u64::from(h);
            if pixels == 0 || pixels > MAX_HEIF_PIXELS {
                return Err(fail(
                    ErrorCode::BadImage,
                    format!("container declares {w}×{h}"),
                ));
            }
        }
        // HeifDecoder applies irot/imir (and the imir-axis correction).
        // Do not apply EXIF on top.
        let image = HeifDecoder.decode(path, &bytes, ImageKind::Heic)?;
        Ok(image.to_rgb8())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_path_is_decode_not_none() {
        let err = linux_heic_decoder()
            .decode("/no/such/linux-heic-host.heic")
            .unwrap_err();
        assert!(
            matches!(
                err,
                MlError::Preprocess {
                    code: ErrorCode::Decode,
                    ..
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn jpeg_bytes_named_heic_fail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.heic");
        std::fs::write(&path, crate::decode::tests_jpeg()).unwrap();
        let err = linux_heic_decoder()
            .decode(path.to_str().unwrap())
            .unwrap_err();
        assert!(matches!(err, MlError::Preprocess { .. }), "{err:?}");
    }

    #[test]
    fn declared_bomb_extent_is_refused_before_decode() {
        let mut ipco = Vec::new();
        let mut ispe = vec![0u8, 0, 0, 0];
        ispe.extend_from_slice(&40_000u32.to_be_bytes());
        ispe.extend_from_slice(&40_000u32.to_be_bytes());
        ipco.extend_from_slice(&boxed(b"ispe", &ispe));

        let mut ftyp = b"heic".to_vec();
        ftyp.extend_from_slice(&0u32.to_be_bytes());
        ftyp.extend_from_slice(b"heic");
        let mut file = boxed(b"ftyp", &ftyp);
        let mut meta = vec![0u8, 0, 0, 0];
        meta.extend_from_slice(&boxed(b"iprp", &boxed(b"ipco", &ipco)));
        file.extend_from_slice(&boxed(b"meta", &meta));

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bomb.heic");
        std::fs::write(&path, &file).unwrap();
        let err = linux_heic_decoder()
            .decode(path.to_str().unwrap())
            .unwrap_err();
        assert!(
            matches!(
                err,
                MlError::Preprocess {
                    code: ErrorCode::BadImage,
                    ..
                }
            ),
            "{err:?}"
        );
    }

    fn boxed(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        out
    }
}
