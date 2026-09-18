//! First-frame grab for standalone movies. Display only.
//!
//! The still-image decoder cannot open `.mp4` / `.mov`. Leftover GTK left
//! those tiles blank; gallery-gtk asked `decode_limited` to slurp the file
//! as JPEG and got nothing — or refused anything over 80 MB. Analysis never
//! samples video frames (same as iOS). This is the Linux stand-in for
//! `ThumbnailService.decodeVideo`: grab one frame, write it through the
//! XDG thumb cache, and stop.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use gallery_model::file_url::file_url_string;
use gallery_scan::{classify, MediaKind};

use crate::decode::RgbFrame;

const GRAB_TIMEOUT: Duration = Duration::from_secs(20);

static TOOL: OnceLock<Option<Tool>> = OnceLock::new();
static MISSES: Mutex<Vec<(String, i64)>> = Mutex::new(Vec::new());

#[derive(Clone, Copy)]
enum Tool {
    FfmpegThumbnailer,
    Totem,
    Ffmpeg,
    GstLaunch,
}

/// Same extension table the scanner uses for `is_video`.
pub fn is_video_path(path: &str) -> bool {
    let name = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);
    classify(name) == MediaKind::Video
}

/// One oriented RGB frame, long side ≤ `max_side`. `None` if no system
/// thumbnailer can open the file.
pub fn decode_frame(path: &str, max_side: u32) -> Option<RgbFrame> {
    let _span = localcore_trace::span("video", "decode_frame").extra("max_side", max_side);
    let mtime = file_mtime_secs(path)?;
    if already_missed(path, mtime) {
        return None;
    }
    let Some(tool) = *TOOL.get_or_init(detect_tool) else {
        localcore_trace::event("video", "no thumbnailer on PATH");
        remember_miss(path, mtime);
        return None;
    };
    let tmp = temp_png();
    let ok = match tool {
        Tool::FfmpegThumbnailer => run(Command::new("ffmpegthumbnailer")
            .arg("-i")
            .arg(path)
            .arg("-o")
            .arg(&tmp)
            .arg("-s")
            .arg(max_side.to_string())
            .arg("-t")
            .arg("0")),
        Tool::Totem => run(Command::new("totem-video-thumbnailer")
            .arg("-s")
            .arg(max_side.to_string())
            .arg(path)
            .arg(&tmp)),
        Tool::Ffmpeg => run(Command::new("ffmpeg")
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-ss",
                "0",
            ])
            .arg("-i")
            .arg(path)
            .args(["-frames:v", "1", "-vf", &format!("scale={max_side}:-1")])
            .arg(&tmp)),
        Tool::GstLaunch => {
            let uri = file_url_string(path);
            run(Command::new("gst-launch-1.0").args([
                "-q",
                "uridecodebin",
                &format!("uri={uri}"),
                "!",
                "videoconvert",
                "!",
                "videoscale",
                "!",
                &format!("video/x-raw,width={max_side}"),
                "!",
                "pngenc",
                "snapshot=true",
                "!",
                "filesink",
                &format!("location={}", tmp.display()),
            ]))
        }
    };
    let frame = ok.and_then(|_| load_png(&tmp, max_side));
    let _ = std::fs::remove_file(&tmp);
    if frame.is_none() {
        localcore_trace::detail("video", format!("grab miss {path}"));
        remember_miss(path, mtime);
    }
    frame
}

fn detect_tool() -> Option<Tool> {
    for (name, tool) in [
        ("ffmpegthumbnailer", Tool::FfmpegThumbnailer),
        ("totem-video-thumbnailer", Tool::Totem),
        ("ffmpeg", Tool::Ffmpeg),
        ("gst-launch-1.0", Tool::GstLaunch),
    ] {
        if on_path(name) {
            localcore_trace::event("video", format!("thumbnailer={name}"));
            return Some(tool);
        }
    }
    None
}

fn on_path(name: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(name).is_file())
}

fn run(cmd: &mut Command) -> Option<()> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success().then_some(()),
            Ok(None) if start.elapsed() > GRAB_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => return None,
        }
    }
}

fn load_png(path: &Path, max_side: u32) -> Option<RgbFrame> {
    let bytes = std::fs::read(path).ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    let img = if img.width().max(img.height()) > max_side {
        img.thumbnail(max_side, max_side)
    } else {
        img
    };
    let rgb = img.to_rgb8();
    Some(RgbFrame {
        width: rgb.width(),
        height: rgb.height(),
        rgb: rgb.into_raw(),
    })
}

fn temp_png() -> PathBuf {
    std::env::temp_dir().join(format!(
        "localgallery-video-{}-{}.png",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ))
}

fn file_mtime_secs(path: &str) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(
        modified
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs() as i64,
    )
}

fn already_missed(path: &str, mtime: i64) -> bool {
    let guard = MISSES.lock().unwrap_or_else(|p| p.into_inner());
    guard.iter().any(|(p, t)| p == path && *t == mtime)
}

fn remember_miss(path: &str, mtime: i64) {
    let mut guard = MISSES.lock().unwrap_or_else(|p| p.into_inner());
    if !guard.iter().any(|(p, t)| p == path && *t == mtime) {
        if guard.len() > 2048 {
            guard.clear();
        }
        guard.push((path.to_string(), mtime));
    }
}

#[cfg(test)]
pub(crate) struct TinyMovie {
    pub dir: tempfile::TempDir,
    pub path: PathBuf,
}

/// A few VP8 frames, when this host can encode them. Used by XDG tests.
#[cfg(test)]
pub(crate) fn tiny_movie() -> Option<TinyMovie> {
    if !on_path("gst-launch-1.0") {
        return None;
    }
    let dir = tempfile::tempdir().ok()?;
    let path = dir.path().join("clip.webm");
    let ok = run(Command::new("gst-launch-1.0").args([
        "-q",
        "videotestsrc",
        "num-buffers=3",
        "pattern=smpte",
        "!",
        "video/x-raw,width=64,height=48,framerate=10/1",
        "!",
        "videoconvert",
        "!",
        "vp8enc",
        "deadline=1",
        "!",
        "webmmux",
        "!",
        "filesink",
        &format!("location={}", path.display()),
    ]));
    if ok.is_none() || !path.is_file() || std::fs::metadata(&path).ok()?.len() == 0 {
        return None;
    }
    Some(TinyMovie { dir, path })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanner_extensions_are_videos() {
        assert!(is_video_path("/lib/Clip.MOV"));
        assert!(is_video_path("/lib/a/b.mp4"));
        assert!(is_video_path("n.webm"));
        assert!(!is_video_path("/lib/IMG_1.heic"));
        assert!(!is_video_path("/lib/a.jpg"));
    }

    #[test]
    fn a_truncated_movie_does_not_panic_or_slurp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.mp4");
        std::fs::write(&path, b"not a movie").unwrap();
        assert!(decode_frame(path.to_str().unwrap(), 64).is_none());
    }

    #[test]
    fn a_real_movie_yields_a_frame_when_the_host_can_encode() {
        let Some(movie) = tiny_movie() else {
            return;
        };
        let frame = decode_frame(movie.path.to_str().unwrap(), 64).expect("grab");
        assert!(frame.width >= 1 && frame.height >= 1);
        assert_eq!(frame.rgb.len(), (frame.width * frame.height * 3) as usize);
    }
}
