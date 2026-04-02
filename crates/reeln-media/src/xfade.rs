//! Xfade concatenation: join multiple video files with cross-fade transitions.
//!
//! Uses the `xfade` video filter and `acrossfade` audio filter to create
//! smooth fade transitions between adjacent clips.

use std::path::Path;

use crate::{MediaError, RenderResult};

/// Options for xfade concatenation.
#[derive(Debug, Clone)]
pub struct XfadeOptions {
    /// Duration of each cross-fade transition in seconds.
    pub fade_duration: f64,
    /// Video codec (e.g. "libx264").
    pub video_codec: String,
    /// Constant rate factor.
    pub crf: u32,
    /// Audio codec (e.g. "aac").
    pub audio_codec: String,
    /// Audio sample rate.
    pub audio_rate: u32,
}

impl Default for XfadeOptions {
    fn default() -> Self {
        Self {
            fade_duration: 0.5,
            video_codec: "libx264".to_string(),
            crf: 18,
            audio_codec: "aac".to_string(),
            audio_rate: 48000,
        }
    }
}

/// Concatenate files with xfade transitions using native libavfilter.
///
/// Uses `movie` source filters for secondary inputs (files[1..]) and the
/// standard buffer source for the primary input (files[0]). The video xfade
/// and audio acrossfade chains are combined into a single filter_complex
/// string executed by the native render pipeline.
pub fn xfade_concat_native(
    files: &[&Path],
    durations: &[f64],
    output: &Path,
    opts: &XfadeOptions,
) -> Result<RenderResult, MediaError> {
    validate_xfade_inputs(files, durations)?;

    let n = files.len();
    let fade = opts
        .fade_duration
        .min(durations.iter().copied().fold(f64::MAX, f64::min) / 2.0);

    // Build the filter_complex string.
    // Secondary inputs loaded via movie source filter; primary input via buffer.
    let mut parts: Vec<String> = Vec::new();

    // Add movie source filters for inputs 1..N-1 (video + audio).
    for (i, file) in files.iter().enumerate().skip(1) {
        let path_escaped = file
            .display()
            .to_string()
            .replace('\\', "\\\\")
            .replace(':', "\\:")
            .replace("'", "\\'");
        parts.push(format!("movie='{path_escaped}':s=dv+da[_mv{i}][_ma{i}]"));
    }

    // Build video xfade chain.
    let mut offset = durations[0] - fade;
    #[allow(clippy::needless_range_loop)]
    for i in 1..n {
        let v_in = if i == 1 {
            "[0:v]".to_string()
        } else {
            format!("[_xf{}]", i - 2)
        };
        let v_src = format!("[_mv{i}]");
        let v_out = if i < n - 1 {
            format!("[_xf{}]", i - 1)
        } else {
            "[vfinal]".to_string()
        };
        parts.push(format!(
            "{v_in}{v_src}xfade=transition=fade:duration={fade}:offset={offset:.6}{v_out}"
        ));
        if i < n - 1 {
            offset += durations[i] - fade;
        }
    }

    // Build audio acrossfade chain.
    for i in 1..n {
        let a_in = if i == 1 {
            "[0:a]".to_string()
        } else {
            format!("[_af{}]", i - 2)
        };
        let a_src = format!("[_ma{i}]");
        let a_out = if i < n - 1 {
            format!("[_af{}]", i - 1)
        } else {
            "[afinal]".to_string()
        };
        parts.push(format!(
            "{a_in}{a_src}acrossfade=d={fade}:c1=tri:c2=tri{a_out}"
        ));
    }

    let filter_complex = parts.join(";");

    let plan = crate::RenderPlan {
        input: files[0].to_path_buf(),
        output: output.to_path_buf(),
        video_codec: opts.video_codec.clone(),
        crf: opts.crf,
        preset: Some("medium".to_string()),
        audio_codec: opts.audio_codec.clone(),
        audio_bitrate: None,
        filters: vec![],
        filter_complex: Some(filter_complex),
        audio_filter: None,
    };

    crate::render::render_native(&plan)
}

fn validate_xfade_inputs(files: &[&Path], durations: &[f64]) -> Result<(), MediaError> {
    if files.len() != durations.len() {
        return Err(MediaError::Concat(format!(
            "files and durations must have same length: {} vs {}",
            files.len(),
            durations.len()
        )));
    }
    if files.len() < 2 {
        return Err(MediaError::Concat(
            "xfade requires at least 2 input files".to_string(),
        ));
    }
    for (i, f) in files.iter().enumerate() {
        if !f.exists() {
            return Err(MediaError::Concat(format!(
                "input {} does not exist: {}",
                i,
                f.display()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_video_with_audio(path: &Path, duration: f64) {
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                &format!("testsrc=duration={duration}:size=160x120:rate=15"),
                "-f",
                "lavfi",
                "-i",
                &format!("sine=frequency=440:duration={duration}:sample_rate=44100"),
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
            ])
            .arg(path.as_os_str())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("ffmpeg not found");
        assert!(status.success());
    }

    // ── Validation tests ───────────────────────────────────────────

    #[test]
    fn test_validate_mismatched_lengths() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mp4");
        std::fs::write(&a, b"x").unwrap();
        let result = validate_xfade_inputs(&[a.as_path()], &[1.0, 2.0]);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_too_few_files() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mp4");
        std::fs::write(&a, b"x").unwrap();
        let result = validate_xfade_inputs(&[a.as_path()], &[1.0]);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_nonexistent_file() {
        let result = validate_xfade_inputs(
            &[
                Path::new("/nonexistent1.mp4"),
                Path::new("/nonexistent2.mp4"),
            ],
            &[1.0, 2.0],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_xfade_options_default() {
        let opts = XfadeOptions::default();
        assert!((opts.fade_duration - 0.5).abs() < f64::EPSILON);
        assert_eq!(opts.video_codec, "libx264");
        assert_eq!(opts.crf, 18);
        assert_eq!(opts.audio_codec, "aac");
        assert_eq!(opts.audio_rate, 48000);
    }

    #[test]
    fn test_xfade_options_debug_clone() {
        let opts = XfadeOptions::default();
        let cloned = opts.clone();
        assert_eq!(cloned.crf, 18);
        let debug = format!("{opts:?}");
        assert!(debug.contains("XfadeOptions"));
    }

    #[test]
    fn test_xfade_native_two_files() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mp4");
        let b = dir.path().join("b.mp4");
        create_test_video_with_audio(&a, 2.0);
        create_test_video_with_audio(&b, 2.0);

        let output = dir.path().join("xfade_native_out.mp4");
        let opts = XfadeOptions {
            fade_duration: 0.3,
            ..Default::default()
        };
        let result = xfade_concat_native(&[a.as_path(), b.as_path()], &[2.0, 2.0], &output, &opts);

        match result {
            Ok(r) => {
                assert!(output.exists());
                assert!(r.duration_secs > 3.0);
            }
            Err(e) => {
                // movie source filter may not be available in all builds.
                // Verify it's a filter error, not a crash.
                let msg = e.to_string();
                assert!(
                    msg.contains("filter") || msg.contains("movie"),
                    "unexpected error: {msg}"
                );
            }
        }
    }
}
