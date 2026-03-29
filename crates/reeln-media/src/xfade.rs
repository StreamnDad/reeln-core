//! Xfade concatenation: join multiple video files with cross-fade transitions.
//!
//! Uses the `xfade` video filter and `acrossfade` audio filter to create
//! smooth fade transitions between adjacent clips.

use std::path::Path;
use std::process::Command;

use crate::{MediaError, RenderPlan, RenderResult, probe};

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

    let plan = RenderPlan {
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

/// Concatenate files with xfade transitions using the ffmpeg subprocess.
pub fn xfade_concat_subprocess(
    files: &[&Path],
    durations: &[f64],
    output: &Path,
    opts: &XfadeOptions,
) -> Result<RenderResult, MediaError> {
    validate_xfade_inputs(files, durations)?;

    let args = build_xfade_args(files, durations, output, opts);
    run_ffmpeg(&args).map_err(|e| MediaError::Concat(format!("ffmpeg xfade failed: {e}")))?;

    let info = probe::probe(output)?;
    let duration_secs = info.duration_secs.unwrap_or(0.0);

    Ok(RenderResult {
        output: output.to_path_buf(),
        duration_secs,
    })
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

/// Build ffmpeg CLI arguments for xfade concatenation.
pub(crate) fn build_xfade_args(
    files: &[&Path],
    durations: &[f64],
    output: &Path,
    opts: &XfadeOptions,
) -> Vec<String> {
    let n = files.len();
    let fade = opts
        .fade_duration
        .min(durations.iter().copied().fold(f64::MAX, f64::min) / 2.0);

    let mut args = vec!["-y".to_string()];
    for f in files {
        args.extend(["-i".to_string(), f.display().to_string()]);
    }

    let mut v_parts: Vec<String> = Vec::new();
    let mut a_parts: Vec<String> = Vec::new();

    let mut offset = durations[0] - fade;
    #[allow(clippy::needless_range_loop)]
    for i in 1..n {
        let v_in = if i == 1 {
            format!("[{}:v]", i - 1)
        } else {
            format!("[xf{}]", i - 2)
        };
        let v_out = if i < n - 1 {
            format!("[xf{}]", i - 1)
        } else {
            "[vout]".to_string()
        };
        v_parts.push(format!(
            "{v_in}[{i}:v]xfade=transition=fade:duration={fade}:offset={offset:.6}{v_out}"
        ));

        let a_in = if i == 1 {
            format!("[{}:a]", i - 1)
        } else {
            format!("[af{}]", i - 2)
        };
        let a_out = if i < n - 1 {
            format!("[af{}]", i - 1)
        } else {
            "[aout]".to_string()
        };
        a_parts.push(format!(
            "{a_in}[{i}:a]acrossfade=d={fade}:c1=tri:c2=tri{a_out}"
        ));

        if i < n - 1 {
            offset += durations[i] - fade;
        }
    }

    let filter_complex = [v_parts, a_parts].concat().join(";");

    args.extend([
        "-filter_complex".to_string(),
        filter_complex,
        "-map".to_string(),
        "[vout]".to_string(),
        "-map".to_string(),
        "[aout]".to_string(),
        "-c:v".to_string(),
        opts.video_codec.clone(),
        "-crf".to_string(),
        opts.crf.to_string(),
        "-c:a".to_string(),
        opts.audio_codec.clone(),
        "-ar".to_string(),
        opts.audio_rate.to_string(),
        output.display().to_string(),
    ]);

    args
}

fn run_ffmpeg(args: &[String]) -> Result<(), String> {
    let output = Command::new("ffmpeg")
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("failed to spawn ffmpeg: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "ffmpeg exited with {}: {}",
            output.status,
            stderr.lines().last().unwrap_or("unknown error")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;

    fn create_test_video_with_audio(path: &Path, duration: f64) {
        let status = StdCommand::new("ffmpeg")
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

    // ── build_xfade_args tests ─────────────────────────────────────

    #[test]
    fn test_build_xfade_args_two_files() {
        let opts = XfadeOptions::default();
        let args = build_xfade_args(
            &[Path::new("/a.mp4"), Path::new("/b.mp4")],
            &[10.0, 8.0],
            Path::new("/out.mp4"),
            &opts,
        );

        assert!(args.contains(&"-filter_complex".to_string()));
        let fc_idx = args.iter().position(|a| a == "-filter_complex").unwrap();
        let fc = &args[fc_idx + 1];
        assert!(fc.contains("xfade=transition=fade"));
        assert!(fc.contains("acrossfade"));
        assert!(fc.contains("[vout]"));
        assert!(fc.contains("[aout]"));
    }

    #[test]
    fn test_build_xfade_args_three_files() {
        let opts = XfadeOptions::default();
        let args = build_xfade_args(
            &[
                Path::new("/a.mp4"),
                Path::new("/b.mp4"),
                Path::new("/c.mp4"),
            ],
            &[10.0, 8.0, 6.0],
            Path::new("/out.mp4"),
            &opts,
        );

        let fc_idx = args.iter().position(|a| a == "-filter_complex").unwrap();
        let fc = &args[fc_idx + 1];
        assert_eq!(fc.matches("xfade=").count(), 2);
        assert_eq!(fc.matches("acrossfade=").count(), 2);
        assert!(fc.contains("[xf0]"));
        assert!(fc.contains("[af0]"));
    }

    #[test]
    fn test_build_xfade_args_fade_clamped() {
        let opts = XfadeOptions {
            fade_duration: 5.0,
            ..Default::default()
        };
        let args = build_xfade_args(
            &[Path::new("/a.mp4"), Path::new("/b.mp4")],
            &[10.0, 0.6],
            Path::new("/out.mp4"),
            &opts,
        );

        let fc_idx = args.iter().position(|a| a == "-filter_complex").unwrap();
        let fc = &args[fc_idx + 1];
        // fade clamped to 0.3 (half of shortest clip 0.6)
        assert!(fc.contains("duration=0.3"));
    }

    #[test]
    fn test_build_xfade_args_offset_calculation() {
        let opts = XfadeOptions {
            fade_duration: 0.5,
            ..Default::default()
        };
        let args = build_xfade_args(
            &[Path::new("/a.mp4"), Path::new("/b.mp4")],
            &[10.0, 8.0],
            Path::new("/out.mp4"),
            &opts,
        );

        let fc_idx = args.iter().position(|a| a == "-filter_complex").unwrap();
        let fc = &args[fc_idx + 1];
        // offset = 10.0 - 0.5 = 9.5
        assert!(fc.contains("offset=9.5"));
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

    // ── Integration tests ──────────────────────────────────────────

    #[test]
    fn test_xfade_subprocess_two_files() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mp4");
        let b = dir.path().join("b.mp4");
        create_test_video_with_audio(&a, 2.0);
        create_test_video_with_audio(&b, 2.0);

        let output = dir.path().join("xfade_out.mp4");
        let opts = XfadeOptions {
            fade_duration: 0.3,
            ..Default::default()
        };
        let result =
            xfade_concat_subprocess(&[a.as_path(), b.as_path()], &[2.0, 2.0], &output, &opts)
                .unwrap();

        assert!(output.exists());
        // 2 clips of 2s with 0.3s overlap → ~3.7s
        assert!(result.duration_secs > 3.0);
        assert!(result.duration_secs < 4.5);
    }

    #[test]
    fn test_xfade_subprocess_three_files() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mp4");
        let b = dir.path().join("b.mp4");
        let c = dir.path().join("c.mp4");
        create_test_video_with_audio(&a, 2.0);
        create_test_video_with_audio(&b, 2.0);
        create_test_video_with_audio(&c, 2.0);

        let output = dir.path().join("xfade3_out.mp4");
        let opts = XfadeOptions {
            fade_duration: 0.3,
            ..Default::default()
        };
        let result = xfade_concat_subprocess(
            &[a.as_path(), b.as_path(), c.as_path()],
            &[2.0, 2.0, 2.0],
            &output,
            &opts,
        )
        .unwrap();

        assert!(output.exists());
        // 3 clips of 2s with 0.3s overlaps → ~5.4s
        assert!(result.duration_secs > 4.5);
        assert!(result.duration_secs < 6.5);
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
