//! Playback is a Linux shell host port.
//!
//! GStreamer is optional at build time because headless/core builders need no
//! media development packages. CI compiles the real adapter with
//! `gstreamer-playback`; tests use the deterministic mock.

use music_core::MediaSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStatus {
    Stopped,
    Paused,
    Playing,
}

impl PlaybackStatus {
    #[must_use]
    pub const fn mpris_name(self) -> &'static str {
        match self {
            Self::Stopped => "Stopped",
            Self::Paused => "Paused",
            Self::Playing => "Playing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransportSnapshot {
    pub status: PlaybackStatus,
    pub position_ms: u64,
    pub volume: f64,
}

impl Default for TransportSnapshot {
    fn default() -> Self {
        Self {
            status: PlaybackStatus::Stopped,
            position_ms: 0,
            volume: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportError {
    message: String,
}

impl TransportError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for TransportError {}

pub trait TransportPort {
    fn engine_name(&self) -> &'static str;
    fn load(&mut self, source: &MediaSource) -> Result<(), TransportError>;
    fn play(&mut self) -> Result<(), TransportError>;
    fn pause(&mut self) -> Result<(), TransportError>;
    fn stop(&mut self) -> Result<(), TransportError>;
    fn seek_to(&mut self, position_ms: u64) -> Result<(), TransportError>;
    fn set_volume(&mut self, volume: f64) -> Result<(), TransportError>;
    fn snapshot(&self) -> TransportSnapshot;
}

impl<T: TransportPort + ?Sized> TransportPort for Box<T> {
    fn engine_name(&self) -> &'static str {
        (**self).engine_name()
    }

    fn load(&mut self, source: &MediaSource) -> Result<(), TransportError> {
        (**self).load(source)
    }

    fn play(&mut self) -> Result<(), TransportError> {
        (**self).play()
    }

    fn pause(&mut self) -> Result<(), TransportError> {
        (**self).pause()
    }

    fn stop(&mut self) -> Result<(), TransportError> {
        (**self).stop()
    }

    fn seek_to(&mut self, position_ms: u64) -> Result<(), TransportError> {
        (**self).seek_to(position_ms)
    }

    fn set_volume(&mut self, volume: f64) -> Result<(), TransportError> {
        (**self).set_volume(volume)
    }

    fn snapshot(&self) -> TransportSnapshot {
        (**self).snapshot()
    }
}

/// Deterministic headless port. It performs no file or device IO.
#[derive(Debug, Default)]
pub struct MockTransport {
    loaded: Option<MediaSource>,
    snapshot: TransportSnapshot,
    commands: Vec<TransportCommand>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TransportCommand {
    Load { id: String, path: String },
    Play,
    Pause,
    Stop,
    SeekTo(u64),
    SetVolume(f64),
}

impl MockTransport {
    #[must_use]
    pub fn commands(&self) -> &[TransportCommand] {
        &self.commands
    }
}

impl TransportPort for MockTransport {
    fn engine_name(&self) -> &'static str {
        "Headless mock"
    }

    fn load(&mut self, source: &MediaSource) -> Result<(), TransportError> {
        self.loaded = Some(source.clone());
        self.snapshot.position_ms = 0;
        self.snapshot.status = PlaybackStatus::Paused;
        self.commands.push(TransportCommand::Load {
            id: source.id.clone(),
            path: source.path.clone(),
        });
        Ok(())
    }

    fn play(&mut self) -> Result<(), TransportError> {
        if self.loaded.is_none() {
            return Err(TransportError::new("No track is loaded"));
        }
        self.snapshot.status = PlaybackStatus::Playing;
        self.commands.push(TransportCommand::Play);
        Ok(())
    }

    fn pause(&mut self) -> Result<(), TransportError> {
        self.snapshot.status = PlaybackStatus::Paused;
        self.commands.push(TransportCommand::Pause);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), TransportError> {
        self.snapshot.status = PlaybackStatus::Stopped;
        self.snapshot.position_ms = 0;
        self.commands.push(TransportCommand::Stop);
        Ok(())
    }

    fn seek_to(&mut self, position_ms: u64) -> Result<(), TransportError> {
        self.snapshot.position_ms = position_ms;
        self.commands.push(TransportCommand::SeekTo(position_ms));
        Ok(())
    }

    fn set_volume(&mut self, volume: f64) -> Result<(), TransportError> {
        let volume = volume.clamp(0.0, 1.0);
        self.snapshot.volume = volume;
        self.commands.push(TransportCommand::SetVolume(volume));
        Ok(())
    }

    fn snapshot(&self) -> TransportSnapshot {
        self.snapshot
    }
}

struct UnavailableTransport;

impl TransportPort for UnavailableTransport {
    fn engine_name(&self) -> &'static str {
        "GStreamer unavailable"
    }

    fn load(&mut self, _source: &MediaSource) -> Result<(), TransportError> {
        Err(TransportError::new(
            "Playback is not available",
        ))
    }

    fn play(&mut self) -> Result<(), TransportError> {
        self.load(&MediaSource {
            id: String::new(),
            path: String::new(),
        })
    }

    fn pause(&mut self) -> Result<(), TransportError> {
        Ok(())
    }

    fn stop(&mut self) -> Result<(), TransportError> {
        Ok(())
    }

    fn seek_to(&mut self, _position_ms: u64) -> Result<(), TransportError> {
        self.play()
    }

    fn set_volume(&mut self, _volume: f64) -> Result<(), TransportError> {
        Ok(())
    }

    fn snapshot(&self) -> TransportSnapshot {
        TransportSnapshot::default()
    }
}

#[cfg(feature = "gstreamer-playback")]
pub struct GstreamerTransport {
    playbin: gstreamer::Element,
    status: PlaybackStatus,
    volume: f64,
}

#[cfg(feature = "gstreamer-playback")]
impl GstreamerTransport {
    pub fn new() -> Result<Self, TransportError> {
        use gstreamer as gst;

        gst::init().map_err(|error| TransportError::new(error.to_string()))?;
        let playbin = gst::ElementFactory::make("playbin")
            .build()
            .map_err(|error| TransportError::new(error.to_string()))?;
        Ok(Self {
            playbin,
            status: PlaybackStatus::Stopped,
            volume: 1.0,
        })
    }

    fn set_state(&self, state: gstreamer::State) -> Result<(), TransportError> {
        use gstreamer::prelude::*;

        self.playbin
            .set_state(state)
            .map(|_| ())
            .map_err(|error| TransportError::new(error.to_string()))
    }
}

#[cfg(feature = "gstreamer-playback")]
impl TransportPort for GstreamerTransport {
    fn engine_name(&self) -> &'static str {
        "GStreamer"
    }

    fn load(&mut self, source: &MediaSource) -> Result<(), TransportError> {
        use gst::prelude::*;
        use gstreamer as gst;

        self.set_state(gst::State::Null)?;
        let uri = gst::glib::filename_to_uri(&source.path, None)
            .map_err(|error| TransportError::new(error.to_string()))?;
        self.playbin.set_property("uri", uri.as_str());
        self.status = PlaybackStatus::Paused;
        Ok(())
    }

    fn play(&mut self) -> Result<(), TransportError> {
        self.set_state(gstreamer::State::Playing)?;
        self.status = PlaybackStatus::Playing;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), TransportError> {
        self.set_state(gstreamer::State::Paused)?;
        self.status = PlaybackStatus::Paused;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), TransportError> {
        self.set_state(gstreamer::State::Null)?;
        self.status = PlaybackStatus::Stopped;
        Ok(())
    }

    fn seek_to(&mut self, position_ms: u64) -> Result<(), TransportError> {
        use gstreamer::prelude::*;

        self.playbin
            .seek_simple(
                gstreamer::SeekFlags::FLUSH | gstreamer::SeekFlags::KEY_UNIT,
                gstreamer::ClockTime::from_mseconds(position_ms),
            )
            .map_err(|error| TransportError::new(error.to_string()))
    }

    fn set_volume(&mut self, volume: f64) -> Result<(), TransportError> {
        use gstreamer::prelude::*;

        self.volume = volume.clamp(0.0, 1.0);
        self.playbin.set_property("volume", self.volume);
        Ok(())
    }

    fn snapshot(&self) -> TransportSnapshot {
        use gstreamer::prelude::*;

        let position_ms = self
            .playbin
            .query_position::<gstreamer::ClockTime>()
            .map_or(0, |position| position.mseconds());
        TransportSnapshot {
            status: self.status,
            position_ms,
            volume: self.volume,
        }
    }
}

/// Best available production port for this build.
#[must_use]
pub fn system_transport() -> Box<dyn TransportPort> {
    #[cfg(feature = "gstreamer-playback")]
    {
        if let Ok(transport) = GstreamerTransport::new() {
            return Box::new(transport);
        }
    }
    Box::new(UnavailableTransport)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_transport_is_deterministic() {
        let source = MediaSource {
            id: "track-1".into(),
            path: "/music/song.mp3".into(),
        };
        let mut transport = MockTransport::default();
        transport.load(&source).unwrap();
        transport.play().unwrap();
        transport.seek_to(12_345).unwrap();
        transport.set_volume(0.25).unwrap();
        transport.pause().unwrap();

        assert_eq!(
            transport.snapshot(),
            TransportSnapshot {
                status: PlaybackStatus::Paused,
                position_ms: 12_345,
                volume: 0.25,
            }
        );
        assert_eq!(
            transport.commands(),
            &[
                TransportCommand::Load {
                    id: "track-1".into(),
                    path: "/music/song.mp3".into(),
                },
                TransportCommand::Play,
                TransportCommand::SeekTo(12_345),
                TransportCommand::SetVolume(0.25),
                TransportCommand::Pause,
            ]
        );
    }
}
