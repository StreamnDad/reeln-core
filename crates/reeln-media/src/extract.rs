use std::path::Path;

use ffmpeg_next::{codec, format, frame, media, software};

use crate::MediaError;

/// Extract a single frame from a video at the given timestamp and write it as PNG.
///
/// Seeks to the nearest keyframe before `timestamp`, then decodes forward to the
/// target frame. Converts to RGB24 via swscale and writes PNG via the `image` crate.
pub fn extract_frame(input: &Path, timestamp: f64, output: &Path) -> Result<(), MediaError> {
    if !input.exists() {
        return Err(MediaError::Render(format!(
            "input does not exist: {}",
            input.display()
        )));
    }

    let mut ictx = format::input(input).map_err(|e| {
        MediaError::Render(format!("failed to open input: {}: {e}", input.display()))
    })?;

    let video_stream = ictx
        .streams()
        .best(media::Type::Video)
        .ok_or_else(|| MediaError::NoStreams("no video stream found".to_string()))?;

    let video_idx = video_stream.index();
    let time_base = video_stream.time_base();

    let dec = codec::context::Context::from_parameters(video_stream.parameters())
        .map_err(|e| MediaError::Render(format!("decoder context: {e}")))?
        .decoder()
        .video()
        .map_err(|e| MediaError::Render(format!("video decoder: {e}")))?;

    let width = dec.width();
    let height = dec.height();

    // Seek to the target timestamp.
    let target_ts =
        (timestamp * f64::from(time_base.denominator()) / f64::from(time_base.numerator())) as i64;
    ictx.seek(target_ts, ..target_ts)
        .map_err(|e| MediaError::Render(format!("seek failed: {e}")))?;

    // Re-create decoder after seek (the old one was consumed for stream info).
    let video_stream = ictx.stream(video_idx).unwrap();
    let mut dec = codec::context::Context::from_parameters(video_stream.parameters())
        .map_err(|e| MediaError::Render(format!("decoder context after seek: {e}")))?
        .decoder()
        .video()
        .map_err(|e| MediaError::Render(format!("video decoder after seek: {e}")))?;

    // Decode frames until we reach or pass the target timestamp.
    let mut best_frame: Option<frame::Video> = None;
    let target_pts = target_ts;

    for (stream, packet) in ictx.packets() {
        if stream.index() != video_idx {
            continue;
        }

        let _ = dec.send_packet(&packet);
        let mut vframe = frame::Video::empty();
        while dec.receive_frame(&mut vframe).is_ok() {
            let frame_pts = vframe.timestamp().unwrap_or(0);
            best_frame = Some(vframe.clone());
            if frame_pts >= target_pts {
                // We've reached or passed the target — use this frame.
                return write_frame_as_png(best_frame.as_ref().unwrap(), width, height, output);
            }
        }
    }

    // Flush decoder.
    let _ = dec.send_eof();
    let mut vframe = frame::Video::empty();
    while dec.receive_frame(&mut vframe).is_ok() {
        best_frame = Some(vframe.clone());
    }

    // Use the last decoded frame if we never reached the target.
    match best_frame {
        Some(ref f) => write_frame_as_png(f, width, height, output),
        None => Err(MediaError::Render(
            "no frames decoded from input".to_string(),
        )),
    }
}

/// Convert a video frame to RGB24 and write as PNG.
fn write_frame_as_png(
    frame: &frame::Video,
    width: u32,
    height: u32,
    output: &Path,
) -> Result<(), MediaError> {
    // Convert pixel format to RGB24 using swscale.
    let mut scaler = software::scaling::Context::get(
        frame.format(),
        width,
        height,
        format::Pixel::RGB24,
        width,
        height,
        software::scaling::Flags::BILINEAR,
    )
    .map_err(|e| MediaError::Render(format!("scaler: {e}")))?;

    let mut rgb_frame = frame::Video::empty();
    scaler
        .run(frame, &mut rgb_frame)
        .map_err(|e| MediaError::Render(format!("scale to RGB24: {e}")))?;

    // Extract RGB data from the frame.
    let data = rgb_frame.data(0);
    let stride = rgb_frame.stride(0);

    // Copy row-by-row to handle stride padding.
    let mut pixels = Vec::with_capacity((width * height * 3) as usize);
    for row in 0..height as usize {
        let start = row * stride;
        let end = start + (width as usize * 3);
        pixels.extend_from_slice(&data[start..end]);
    }

    // Write PNG using image crate.
    image::save_buffer(output, &pixels, width, height, image::ColorType::Rgb8).map_err(|e| {
        MediaError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            e.to_string(),
        ))
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn create_test_video(path: &Path) {
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=2:size=160x120:rate=15",
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
    fn test_extract_frame_nonexistent_input() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("frame.png");
        let result = extract_frame(Path::new("/nonexistent.mp4"), 0.5, &output);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_frame_at_beginning() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("video.mp4");
        create_test_video(&input);

        let output = dir.path().join("frame_start.png");
        let result = extract_frame(&input, 0.0, &output);
        assert!(result.is_ok(), "extract_frame failed: {result:?}");
        assert!(output.exists());
        assert!(output.metadata().unwrap().len() > 100); // PNG has content
    }

    #[test]
    fn test_extract_frame_at_middle() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("video.mp4");
        create_test_video(&input);

        let output = dir.path().join("frame_mid.png");
        let result = extract_frame(&input, 1.0, &output);
        assert!(result.is_ok(), "extract_frame failed: {result:?}");
        assert!(output.exists());

        // Verify it's a valid PNG by reading with image crate.
        let img = image::open(&output).unwrap();
        assert_eq!(img.width(), 160);
        assert_eq!(img.height(), 120);
    }

    #[test]
    fn test_extract_frame_at_end() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("video.mp4");
        create_test_video(&input);

        let output = dir.path().join("frame_end.png");
        let result = extract_frame(&input, 1.9, &output);
        assert!(result.is_ok(), "extract_frame failed: {result:?}");
        assert!(output.exists());
    }

    #[test]
    fn test_extract_frame_past_end() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("video.mp4");
        create_test_video(&input);

        let output = dir.path().join("frame_past.png");
        // Seek past the end — should still return the last frame.
        let result = extract_frame(&input, 100.0, &output);
        assert!(result.is_ok(), "extract_frame past end failed: {result:?}");
        assert!(output.exists());
    }

    #[test]
    fn test_extract_frame_invalid_input() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.mp4");
        std::fs::write(&bad, b"not a video").unwrap();

        let output = dir.path().join("frame.png");
        let result = extract_frame(&bad, 0.5, &output);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_frame_produces_different_frames() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("video.mp4");
        create_test_video(&input);

        let out1 = dir.path().join("frame1.png");
        let out2 = dir.path().join("frame2.png");
        extract_frame(&input, 0.1, &out1).unwrap();
        extract_frame(&input, 1.5, &out2).unwrap();

        // Different timestamps should produce different file content
        // (test pattern changes over time).
        let data1 = std::fs::read(&out1).unwrap();
        let data2 = std::fs::read(&out2).unwrap();
        assert_ne!(data1, data2, "frames at different times should differ");
    }
}
