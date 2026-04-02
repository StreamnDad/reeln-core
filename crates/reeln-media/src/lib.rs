pub mod codec;
pub mod composite;
pub mod concat;
pub mod error;
pub mod extract;
pub mod filter;
pub mod probe;
pub mod render;
pub mod xfade;

use std::path::Path;

pub use error::MediaError;
pub use probe::MediaInfo;

/// Options for concatenating media segments.
#[derive(Debug, Clone)]
pub struct ConcatOptions {
    /// Stream-copy (true) vs re-encode (false).
    pub copy: bool,
    /// Video codec (e.g. "libx264").
    pub video_codec: String,
    /// Constant rate factor.
    pub crf: u32,
    /// Audio codec (e.g. "aac").
    pub audio_codec: String,
    /// Audio sample rate.
    pub audio_rate: u32,
}

/// A complete render plan describing input, filters, and output settings.
#[derive(Debug, Clone)]
pub struct RenderPlan {
    pub input: std::path::PathBuf,
    pub output: std::path::PathBuf,
    pub video_codec: String,
    pub crf: u32,
    pub preset: Option<String>,
    pub audio_codec: String,
    pub audio_bitrate: Option<u32>,
    pub filters: Vec<String>,
    /// Full filter_complex string (semicolon-separated graph).
    /// When set, this is used instead of `filters` for the video path.
    pub filter_complex: Option<String>,
    /// Separate audio filter chain (e.g. "atempo=2.0").
    /// Applied independently from the video filter graph.
    pub audio_filter: Option<String>,
}

/// Result of a render operation.
#[derive(Debug, Clone)]
pub struct RenderResult {
    pub output: std::path::PathBuf,
    pub duration_secs: f64,
}

/// Core media backend trait. All media operations go through this interface.
pub trait MediaBackend: Send + Sync {
    fn probe(&self, path: &Path) -> Result<MediaInfo, MediaError>;
    fn concat(
        &self,
        segments: &[&Path],
        output: &Path,
        opts: &ConcatOptions,
    ) -> Result<(), MediaError>;
    fn render(&self, plan: &RenderPlan) -> Result<RenderResult, MediaError>;
}

/// Backend that uses native ffmpeg-next (libav*) for all media operations.
#[derive(Debug, Default, Clone)]
pub struct LibavBackend;

impl LibavBackend {
    pub fn new() -> Self {
        Self
    }
}

impl MediaBackend for LibavBackend {
    fn probe(&self, path: &Path) -> Result<MediaInfo, MediaError> {
        probe::probe(path)
    }

    fn concat(
        &self,
        segments: &[&Path],
        output: &Path,
        opts: &ConcatOptions,
    ) -> Result<(), MediaError> {
        concat::concat_native(segments, output, opts)
    }

    fn render(&self, plan: &RenderPlan) -> Result<RenderResult, MediaError> {
        render::render_native(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    fn create_test_video(path: &Path) {
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=160x120:rate=15",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(path.as_os_str())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("ffmpeg not found");
        assert!(status.success());
    }

    #[test]
    fn test_libav_backend_probe() {
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("test.mp4");
        create_test_video(&video);

        let backend = LibavBackend::new();
        let info = backend.probe(&video).unwrap();
        assert!(info.duration_secs.is_some());
        assert!(info.width.is_some());
    }

    #[test]
    fn test_libav_backend_concat() {
        let dir = tempfile::tempdir().unwrap();
        let seg1 = dir.path().join("s1.mp4");
        let seg2 = dir.path().join("s2.mp4");
        create_test_video(&seg1);
        create_test_video(&seg2);

        let output = dir.path().join("concat_out.mp4");
        let backend = LibavBackend::new();
        let opts = ConcatOptions {
            copy: true,
            video_codec: "libx264".to_string(),
            crf: 23,
            audio_codec: "aac".to_string(),
            audio_rate: 48000,
        };
        backend
            .concat(&[seg1.as_path(), seg2.as_path()], &output, &opts)
            .unwrap();
        assert!(output.exists());
    }

    #[test]
    fn test_libav_backend_render() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("render_in.mp4");
        create_test_video(&input);

        let output = dir.path().join("render_out.mp4");
        let backend = LibavBackend::new();
        let plan = RenderPlan {
            input,
            output: output.clone(),
            video_codec: "libx264".to_string(),
            crf: 23,
            preset: None,
            audio_codec: "aac".to_string(),
            audio_bitrate: None,
            filters: vec![],
            filter_complex: None,
            audio_filter: None,
        };
        let result = backend.render(&plan).unwrap();
        assert_eq!(result.output, output);
        assert!(result.duration_secs > 0.0);
    }

    #[test]
    #[allow(clippy::default_constructed_unit_structs)]
    fn test_libav_backend_default_and_clone() {
        let b1 = LibavBackend::default();
        let b2 = b1.clone();
        let debug = format!("{b1:?}");
        assert!(debug.contains("LibavBackend"));
        let _ = b2;
    }

    #[test]
    fn test_media_backend_is_object_safe() {
        // Verify the trait can be used as a trait object.
        fn _takes_backend(_b: &dyn MediaBackend) {}
        let backend = LibavBackend::new();
        _takes_backend(&backend);
    }

    #[test]
    fn test_concat_options_debug_clone() {
        let opts = ConcatOptions {
            copy: false,
            video_codec: "libx265".to_string(),
            crf: 28,
            audio_codec: "opus".to_string(),
            audio_rate: 44100,
        };
        let cloned = opts.clone();
        assert!(!cloned.copy);
        assert_eq!(cloned.crf, 28);
        let debug = format!("{opts:?}");
        assert!(debug.contains("ConcatOptions"));
    }

    #[test]
    fn test_render_plan_debug_clone() {
        let plan = RenderPlan {
            input: PathBuf::from("/a.mp4"),
            output: PathBuf::from("/b.mp4"),
            video_codec: "libx264".to_string(),
            crf: 23,
            preset: None,
            audio_codec: "aac".to_string(),
            audio_bitrate: None,
            filters: vec!["scale=1:1".to_string()],
            filter_complex: None,
            audio_filter: None,
        };
        let cloned = plan.clone();
        assert_eq!(cloned.filters.len(), 1);
        let debug = format!("{plan:?}");
        assert!(debug.contains("RenderPlan"));
    }

    #[test]
    fn test_render_result_debug_clone() {
        let result = RenderResult {
            output: PathBuf::from("/x.mp4"),
            duration_secs: 42.0,
        };
        let cloned = result.clone();
        assert!((cloned.duration_secs - 42.0).abs() < f64::EPSILON);
        let debug = format!("{result:?}");
        assert!(debug.contains("RenderResult"));
    }
}
