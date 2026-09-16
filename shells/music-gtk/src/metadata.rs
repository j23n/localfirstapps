//! Linux metadata host. `music-core` asks; `lofty` answers.

use lofty::file::TaggedFileExt;
use lofty::picture::PictureType;
use lofty::prelude::{Accessor, AudioFile};
use lofty::tag::ItemKey;
use music_core::{MetadataRequest, MetadataUpdate};

/// Tags plus optional front-cover bytes. Artwork stays on the host.
#[derive(Debug, Clone)]
pub struct HostRead {
    /// Display metadata applied through `music-core`.
    pub update: MetadataUpdate,
    /// Embedded picture bytes when present.
    pub artwork: Option<Vec<u8>>,
}

/// Read tags for one host request. `None` if the file cannot be probed.
#[must_use]
pub fn read_update(request: &MetadataRequest) -> Option<HostRead> {
    let tagged = lofty::read_from_path(&request.path).ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let duration_ms = tagged.properties().duration().as_millis() as u64;
    let artwork = tag.and_then(artwork_bytes);
    Some(HostRead {
        update: MetadataUpdate {
            id: request.id.clone(),
            title: tag
                .and_then(|tag| tag.title().map(|value| value.to_string()))
                .unwrap_or_default(),
            artist: tag
                .and_then(|tag| tag.artist().map(|value| value.to_string()))
                .unwrap_or_default(),
            album: tag
                .and_then(|tag| tag.album().map(|value| value.to_string()))
                .unwrap_or_default(),
            duration_ms,
            has_artwork: artwork.is_some(),
            has_lyrics: tag.is_some_and(|tag| tag.get_string(&ItemKey::Lyrics).is_some()),
        },
        artwork,
    })
}

/// Always produce an update so a failing probe does not retry forever.
#[must_use]
pub fn read_or_mark(request: &MetadataRequest) -> HostRead {
    read_update(request).unwrap_or(HostRead {
        update: MetadataUpdate {
            id: request.id.clone(),
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            duration_ms: 0,
            has_artwork: false,
            has_lyrics: false,
        },
        artwork: None,
    })
}

fn artwork_bytes(tag: &lofty::tag::Tag) -> Option<Vec<u8>> {
    let pictures = tag.pictures();
    pictures
        .iter()
        .find(|picture| picture.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.first())
        .map(|picture| picture.data().to_vec())
}
