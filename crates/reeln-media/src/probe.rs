use std::path::Path;

use crate::MediaError;

/// Information extracted from a media file.
#[derive(Debug, Clone)]
pub struct MediaInfo {
    pub duration_secs: Option<f64>,
    pub fps: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub codec: Option<String>,
}

/// Convert raw AV_TIME_BASE duration to seconds, returning None if non-positive.
fn duration_to_secs(raw: i64) -> Option<f64> {
    if raw > 0 {
        Some(raw as f64 / f64::from(ffmpeg_next::ffi::AV_TIME_BASE))
    } else {
        None
    }
}

/// Convert a rational frame rate to f64, returning None if invalid.
fn rational_to_fps(num: i32, den: i32) -> Option<f64> {
    if den > 0 && num > 0 {
        Some(f64::from(ffmpeg_next::Rational::new(num, den)))
    } else {
        None
    }
}

/// Convert a raw dimension to Option<u32>, returning None if non-positive.
fn positive_dimension(val: i32) -> Option<u32> {
    if val > 0 { Some(val as u32) } else { None }
}

/// Convert a codec name to Option<String>, filtering out empty/none values.
fn valid_codec_name(name: &str) -> Option<String> {
    if name.is_empty() || name == "none" {
        None
    } else {
        Some(name.to_string())
    }
}

/// Probe a media file for duration, FPS, resolution, and codec info.
pub fn probe(path: &Path) -> Result<MediaInfo, MediaError> {
    let ctx = ffmpeg_next::format::input(path)
        .map_err(|e| MediaError::OpenFailed(format!("{}: {}", path.display(), e)))?;

    let duration_secs = duration_to_secs(ctx.duration());

    let video_stream = ctx.streams().best(ffmpeg_next::media::Type::Video);

    let (fps, width, height, codec) = match video_stream {
        Some(stream) => {
            let avg_rate = stream.avg_frame_rate();
            let fps = rational_to_fps(avg_rate.numerator(), avg_rate.denominator());

            let params = stream.parameters();
            let (w, h) = unsafe {
                let ptr = params.as_ptr();
                ((*ptr).width, (*ptr).height)
            };
            let width = positive_dimension(w);
            let height = positive_dimension(h);

            let codec_name = params.id().name();
            let codec = valid_codec_name(codec_name);

            (fps, width, height, codec)
        }
        None => (None, None, None, None),
    };

    Ok(MediaInfo {
        duration_secs,
        fps,
        width,
        height,
        codec,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    /// Create a small test video file using the system ffmpeg binary.
    fn create_test_video(path: &Path, duration_secs: u32, width: u32, height: u32, fps: u32) {
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                &format!("testsrc=duration={duration_secs}:size={width}x{height}:rate={fps}"),
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(path.as_os_str())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("failed to run ffmpeg");
        assert!(status.success(), "ffmpeg failed to create test video");
    }

    /// Create a small test audio file.
    fn create_test_audio(path: &Path, duration_secs: u32) {
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                &format!("sine=frequency=440:duration={duration_secs}"),
                "-c:a",
                "aac",
            ])
            .arg(path.as_os_str())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("failed to run ffmpeg");
        assert!(status.success(), "ffmpeg failed to create test audio");
    }

    #[test]
    fn test_probe_video_file() {
        let dir = tempfile::tempdir().unwrap();
        let video_path = dir.path().join("test.mp4");
        create_test_video(&video_path, 2, 320, 240, 30);

        let info = probe(&video_path).unwrap();

        assert!(info.duration_secs.is_some());
        let dur = info.duration_secs.unwrap();
        assert!(dur > 1.0 && dur < 3.0, "duration was {dur}");

        assert_eq!(info.width, Some(320));
        assert_eq!(info.height, Some(240));

        assert!(info.fps.is_some());
        let fps = info.fps.unwrap();
        assert!((fps - 30.0).abs() < 1.0, "fps was {fps}");

        assert!(info.codec.is_some());
        let codec = info.codec.unwrap();
        assert!(
            codec.contains("h264") || codec.contains("264"),
            "codec was {codec}"
        );
    }

    #[test]
    fn test_probe_audio_only_file() {
        let dir = tempfile::tempdir().unwrap();
        let audio_path = dir.path().join("test.m4a");
        create_test_audio(&audio_path, 1);

        let info = probe(&audio_path).unwrap();

        assert!(info.duration_secs.is_some());
        // Audio-only: no video stream fields.
        assert!(info.width.is_none());
        assert!(info.height.is_none());
        assert!(info.fps.is_none());
        assert!(info.codec.is_none());
    }

    #[test]
    fn test_probe_nonexistent_file() {
        let result = probe(Path::new("/tmp/nonexistent_file_12345.mp4"));
        assert!(result.is_err());
        match result.unwrap_err() {
            MediaError::OpenFailed(msg) => {
                assert!(msg.contains("nonexistent_file_12345"));
            }
            other => panic!("expected OpenFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_probe_invalid_file() {
        let dir = tempfile::tempdir().unwrap();
        let bad_path = dir.path().join("not_a_video.mp4");
        std::fs::write(&bad_path, b"this is not a video file").unwrap();

        let result = probe(&bad_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_media_info_clone() {
        let info = MediaInfo {
            duration_secs: Some(1.5),
            fps: Some(30.0),
            width: Some(1920),
            height: Some(1080),
            codec: Some("h264".to_string()),
        };
        let cloned = info.clone();
        assert_eq!(cloned.duration_secs, info.duration_secs);
        assert_eq!(cloned.fps, info.fps);
        assert_eq!(cloned.width, info.width);
        assert_eq!(cloned.height, info.height);
        assert_eq!(cloned.codec, info.codec);
    }

    #[test]
    fn test_media_info_debug() {
        let info = MediaInfo {
            duration_secs: None,
            fps: None,
            width: None,
            height: None,
            codec: None,
        };
        let debug = format!("{info:?}");
        assert!(debug.contains("MediaInfo"));
    }

    #[test]
    fn test_probe_returns_path_in_error() {
        let bad = PathBuf::from("/no/such/path/video.mp4");
        let err = probe(&bad).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("/no/such/path/video.mp4"),
            "error message should contain path: {msg}"
        );
    }

    // Unit tests for helper functions to cover all branches.

    #[test]
    fn test_duration_to_secs_positive() {
        let result = duration_to_secs(2_000_000);
        assert_eq!(result, Some(2.0));
    }

    #[test]
    fn test_duration_to_secs_zero() {
        assert_eq!(duration_to_secs(0), None);
    }

    #[test]
    fn test_duration_to_secs_negative() {
        assert_eq!(duration_to_secs(-1), None);
    }

    #[test]
    fn test_rational_to_fps_valid() {
        let fps = rational_to_fps(30, 1);
        assert!(fps.is_some());
        assert!((fps.unwrap() - 30.0).abs() < 0.01);
    }

    #[test]
    fn test_rational_to_fps_zero_denominator() {
        assert_eq!(rational_to_fps(30, 0), None);
    }

    #[test]
    fn test_rational_to_fps_zero_numerator() {
        assert_eq!(rational_to_fps(0, 1), None);
    }

    #[test]
    fn test_rational_to_fps_negative() {
        assert_eq!(rational_to_fps(-1, 1), None);
    }

    #[test]
    fn test_positive_dimension_valid() {
        assert_eq!(positive_dimension(1920), Some(1920));
    }

    #[test]
    fn test_positive_dimension_zero() {
        assert_eq!(positive_dimension(0), None);
    }

    #[test]
    fn test_positive_dimension_negative() {
        assert_eq!(positive_dimension(-1), None);
    }

    #[test]
    fn test_valid_codec_name_h264() {
        assert_eq!(valid_codec_name("h264"), Some("h264".to_string()));
    }

    #[test]
    fn test_valid_codec_name_empty() {
        assert_eq!(valid_codec_name(""), None);
    }

    #[test]
    fn test_valid_codec_name_none_string() {
        assert_eq!(valid_codec_name("none"), None);
    }
}
