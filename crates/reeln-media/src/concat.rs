use std::io::Write;
use std::path::Path;
use std::process::Command;

use ffmpeg_next::{
    ChannelLayout, Dictionary, Packet, Rational, Rescale, codec, decoder, encoder, format, frame,
    media, picture, software,
};

use crate::{ConcatOptions, MediaError};

/// Concatenate media segments into a single output file using native libav* APIs.
///
/// In copy mode, packets are remuxed directly (no re-encode).
/// In re-encode mode, falls back to subprocess ffmpeg (full native re-encode
/// concat is a future enhancement).
pub fn concat_native(
    segments: &[&Path],
    output: &Path,
    opts: &ConcatOptions,
) -> Result<(), MediaError> {
    validate_segments(segments)?;

    if opts.copy {
        concat_copy(segments, output)
    } else {
        concat_reencode(segments, output, opts)
    }
}

/// Concatenate media segments using the ffmpeg subprocess (Phase 1a fallback).
pub fn concat_subprocess(
    segments: &[&Path],
    output: &Path,
    opts: &ConcatOptions,
) -> Result<(), MediaError> {
    validate_segments(segments)?;

    let concat_list = build_concat_file(segments)?;
    let args = build_concat_args(&concat_list, output, opts);

    run_ffmpeg(&args).map_err(|e| MediaError::Concat(format!("ffmpeg concat failed: {e}")))?;

    let _ = std::fs::remove_file(&concat_list);
    Ok(())
}

// ── Validation ──────────────────────────────────────────────────────

fn validate_segments(segments: &[&Path]) -> Result<(), MediaError> {
    if segments.is_empty() {
        return Err(MediaError::Concat("no segments provided".to_string()));
    }

    for (i, seg) in segments.iter().enumerate() {
        if !seg.exists() {
            return Err(MediaError::Concat(format!(
                "segment {i} does not exist: {}",
                seg.display()
            )));
        }
    }
    Ok(())
}

// ── Native copy-mode concat ─────────────────────────────────────────

/// Packet-level remux of multiple segments into a single output (no re-encode).
fn concat_copy(segments: &[&Path], output: &Path) -> Result<(), MediaError> {
    // Open the first segment to discover stream layout.
    let first_ictx = format::input(segments[0]).map_err(|e| {
        MediaError::Concat(format!(
            "failed to open segment 0: {}: {e}",
            segments[0].display()
        ))
    })?;

    let mut octx = format::output(output).map_err(|e| {
        MediaError::Concat(format!(
            "failed to create output: {}: {e}",
            output.display()
        ))
    })?;

    // Map input streams → output streams.
    let mut stream_mapping: Vec<i32> = vec![-1; first_ictx.nb_streams() as usize];
    let mut ist_time_bases: Vec<Rational> = vec![Rational(0, 1); first_ictx.nb_streams() as usize];
    let mut ost_index: i32 = 0;

    for (ist_index, ist) in first_ictx.streams().enumerate() {
        let medium = ist.parameters().medium();
        if medium != media::Type::Audio
            && medium != media::Type::Video
            && medium != media::Type::Subtitle
        {
            continue;
        }

        stream_mapping[ist_index] = ost_index;
        ist_time_bases[ist_index] = ist.time_base();
        ost_index += 1;

        let mut ost = octx
            .add_stream(encoder::find(codec::Id::None))
            .map_err(|e| MediaError::Concat(format!("failed to add output stream: {e}")))?;
        ost.set_parameters(ist.parameters());
        // Clear codec_tag for container format compatibility.
        unsafe {
            (*ost.parameters().as_mut_ptr()).codec_tag = 0;
        }
    }

    octx.write_header()
        .map_err(|e| MediaError::Concat(format!("failed to write output header: {e}")))?;

    // Collect output time bases after header is written.
    let ost_time_bases: Vec<Rational> = (0..octx.nb_streams() as usize)
        .map(|i| octx.stream(i).unwrap().time_base())
        .collect();

    // Process each segment.
    let num_ost = ost_index as usize;
    let mut pts_offsets: Vec<i64> = vec![0; num_ost];
    let mut last_pts: Vec<i64> = vec![0; num_ost];
    let mut last_duration: Vec<i64> = vec![0; num_ost];

    // We need to re-open even the first segment since we consumed `first_ictx`
    // for stream layout discovery only. Drop it before the loop.
    drop(first_ictx);

    for (seg_idx, seg_path) in segments.iter().enumerate() {
        // Update offsets from previous segment.
        if seg_idx > 0 {
            for ost_i in 0..num_ost {
                pts_offsets[ost_i] = last_pts[ost_i] + last_duration[ost_i];
            }
        }

        let mut ictx = format::input(seg_path).map_err(|e| {
            MediaError::Concat(format!(
                "failed to open segment {seg_idx}: {}: {e}",
                seg_path.display()
            ))
        })?;

        // Build stream mapping for this segment (may differ from first).
        let seg_mapping = build_segment_mapping(&ictx, &stream_mapping, &ist_time_bases);

        for (stream, mut packet) in ictx.packets() {
            let ist_idx = stream.index();
            if ist_idx >= seg_mapping.len() {
                continue;
            }
            let ost_idx = seg_mapping[ist_idx];
            if ost_idx < 0 {
                continue;
            }
            let ost_i = ost_idx as usize;
            let ost_tb = ost_time_bases[ost_i];

            packet.rescale_ts(stream.time_base(), ost_tb);

            // Apply PTS offset for segment concatenation.
            if let Some(pts) = packet.pts() {
                let new_pts = pts + pts_offsets[ost_i];
                packet.set_pts(Some(new_pts));
                last_pts[ost_i] = new_pts;
            }
            if let Some(dts) = packet.dts() {
                packet.set_dts(Some(dts + pts_offsets[ost_i]));
            }
            last_duration[ost_i] = packet.duration();

            packet.set_position(-1);
            packet.set_stream(ost_idx as _);
            packet
                .write_interleaved(&mut octx)
                .map_err(|e| MediaError::Concat(format!("failed to write packet: {e}")))?;
        }

        // Break the borrow — ictx is consumed by packets() iterator above
        // (it's moved into the for loop), so nothing to do here.
    }

    octx.write_trailer()
        .map_err(|e| MediaError::Concat(format!("failed to write output trailer: {e}")))?;

    Ok(())
}

/// Build a stream index mapping for a segment based on the reference (first segment) mapping.
fn build_segment_mapping(
    ictx: &format::context::Input,
    ref_mapping: &[i32],
    _ref_time_bases: &[Rational],
) -> Vec<i32> {
    let mut mapping = vec![-1i32; ictx.nb_streams() as usize];
    for (ist_index, ist) in ictx.streams().enumerate() {
        let medium = ist.parameters().medium();
        if medium != media::Type::Audio
            && medium != media::Type::Video
            && medium != media::Type::Subtitle
        {
            continue;
        }
        // Try to find matching output stream by index.
        if ist_index < ref_mapping.len() {
            mapping[ist_index] = ref_mapping[ist_index];
        }
    }
    mapping
}

// ── Native re-encode concat ─────────────────────────────────────────

/// Decode → re-encode concat of multiple segments into a single output.
///
/// Each segment is decoded, frames are PTS-offset to form a continuous
/// timeline, then re-encoded into the output container.
fn concat_reencode(
    segments: &[&Path],
    output: &Path,
    opts: &ConcatOptions,
) -> Result<(), MediaError> {
    // Open first segment to discover stream layout.
    let first_ictx = format::input(segments[0]).map_err(|e| {
        MediaError::Concat(format!(
            "failed to open segment 0: {}: {e}",
            segments[0].display()
        ))
    })?;

    let mut octx = format::output(output).map_err(|e| {
        MediaError::Concat(format!(
            "failed to create output: {}: {e}",
            output.display()
        ))
    })?;

    let global_header = octx.format().flags().contains(format::Flags::GLOBAL_HEADER);

    // Discover streams and set up encoders.
    let best_video_idx = first_ictx
        .streams()
        .best(media::Type::Video)
        .map(|s| s.index());
    let best_audio_idx = first_ictx
        .streams()
        .best(media::Type::Audio)
        .map(|s| s.index());

    let mut stream_mapping: Vec<i32> = vec![-1; first_ictx.nb_streams() as usize];
    let mut stream_configs: Vec<StreamConfig> = Vec::new();
    let mut ost_index: i32 = 0;

    for (ist_index, ist) in first_ictx.streams().enumerate() {
        let medium = ist.parameters().medium();

        if medium == media::Type::Video && Some(ist_index) == best_video_idx {
            let dec = codec::context::Context::from_parameters(ist.parameters())
                .map_err(|e| MediaError::Concat(format!("video decoder context: {e}")))?
                .decoder()
                .video()
                .map_err(|e| MediaError::Concat(format!("video decoder: {e}")))?;

            let enc_codec = encoder::find_by_name(&opts.video_codec).ok_or_else(|| {
                MediaError::Concat(format!("video encoder not found: {}", opts.video_codec))
            })?;

            let mut ost = octx
                .add_stream(enc_codec)
                .map_err(|e| MediaError::Concat(format!("add video stream: {e}")))?;

            let mut enc = codec::context::Context::new_with_codec(enc_codec)
                .encoder()
                .video()
                .map_err(|e| MediaError::Concat(format!("video encoder: {e}")))?;

            ost.set_parameters(&enc);
            enc.set_height(dec.height());
            enc.set_width(dec.width());
            enc.set_aspect_ratio(dec.aspect_ratio());
            enc.set_format(dec.format());
            enc.set_frame_rate(dec.frame_rate());
            enc.set_time_base(ist.time_base());

            if global_header {
                enc.set_flags(codec::Flags::GLOBAL_HEADER);
            }

            let mut enc_opts = Dictionary::new();
            enc_opts.set("crf", &opts.crf.to_string());
            enc_opts.set("preset", "medium");

            let opened = enc
                .open_with(enc_opts)
                .map_err(|e| MediaError::Concat(format!("open video encoder: {e}")))?;
            ost.set_parameters(&opened);

            stream_mapping[ist_index] = ost_index;
            stream_configs.push(StreamConfig {
                kind: StreamKind::Video,
                ist_time_base: ist.time_base(),
            });
            ost_index += 1;
        } else if medium == media::Type::Audio && Some(ist_index) == best_audio_idx {
            let dec = codec::context::Context::from_parameters(ist.parameters())
                .map_err(|e| MediaError::Concat(format!("audio decoder context: {e}")))?
                .decoder()
                .audio()
                .map_err(|e| MediaError::Concat(format!("audio decoder: {e}")))?;

            let enc_codec = encoder::find_by_name(&opts.audio_codec).ok_or_else(|| {
                MediaError::Concat(format!("audio encoder not found: {}", opts.audio_codec))
            })?;

            let mut ost = octx
                .add_stream(enc_codec)
                .map_err(|e| MediaError::Concat(format!("add audio stream: {e}")))?;

            let mut enc = codec::context::Context::new_with_codec(enc_codec)
                .encoder()
                .audio()
                .map_err(|e| MediaError::Concat(format!("audio encoder: {e}")))?;

            enc.set_rate(opts.audio_rate as i32);
            enc.set_channel_layout(dec.channel_layout());
            enc.set_format(dec.format());
            enc.set_time_base(Rational(1, opts.audio_rate as i32));

            if global_header {
                enc.set_flags(codec::Flags::GLOBAL_HEADER);
            }

            let opened = enc
                .open_with(Dictionary::new())
                .map_err(|e| MediaError::Concat(format!("open audio encoder: {e}")))?;
            ost.set_parameters(&opened);

            stream_mapping[ist_index] = ost_index;
            stream_configs.push(StreamConfig {
                kind: StreamKind::Audio,
                ist_time_base: ist.time_base(),
            });
            ost_index += 1;
        }
    }

    octx.set_metadata(first_ictx.metadata().to_owned());
    drop(first_ictx);

    octx.write_header()
        .map_err(|e| MediaError::Concat(format!("failed to write header: {e}")))?;

    let ost_time_bases: Vec<Rational> = (0..octx.nb_streams() as usize)
        .map(|i| octx.stream(i).unwrap().time_base())
        .collect();

    // Track PTS offsets per output stream across segments.
    let num_ost = ost_index as usize;
    let mut pts_offsets: Vec<i64> = vec![0; num_ost];
    let mut last_pts: Vec<i64> = vec![0; num_ost];
    let mut last_duration: Vec<i64> = vec![0; num_ost];

    for (seg_idx, seg_path) in segments.iter().enumerate() {
        if seg_idx > 0 {
            for ost_i in 0..num_ost {
                pts_offsets[ost_i] = last_pts[ost_i] + last_duration[ost_i];
            }
        }

        let mut ictx = format::input(seg_path).map_err(|e| {
            MediaError::Concat(format!(
                "failed to open segment {seg_idx}: {}: {e}",
                seg_path.display()
            ))
        })?;

        // Create decoders for this segment.
        let mut video_decoder: Option<(usize, decoder::Video)> = None;
        let mut audio_decoder: Option<(usize, decoder::Audio)> = None;

        for (ist_index, ist) in ictx.streams().enumerate() {
            if ist_index >= stream_mapping.len() || stream_mapping[ist_index] < 0 {
                continue;
            }
            let ost_idx = stream_mapping[ist_index] as usize;
            match stream_configs[ost_idx].kind {
                StreamKind::Video => {
                    let dec = codec::context::Context::from_parameters(ist.parameters())
                        .map_err(|e| {
                            MediaError::Concat(format!("segment {seg_idx} video decoder: {e}"))
                        })?
                        .decoder()
                        .video()
                        .map_err(|e| {
                            MediaError::Concat(format!("segment {seg_idx} video decoder: {e}"))
                        })?;
                    video_decoder = Some((ist_index, dec));
                }
                StreamKind::Audio => {
                    let dec = codec::context::Context::from_parameters(ist.parameters())
                        .map_err(|e| {
                            MediaError::Concat(format!("segment {seg_idx} audio decoder: {e}"))
                        })?
                        .decoder()
                        .audio()
                        .map_err(|e| {
                            MediaError::Concat(format!("segment {seg_idx} audio decoder: {e}"))
                        })?;
                    audio_decoder = Some((ist_index, dec));
                }
            }
        }

        // Create per-segment encoders and process packets.

        // Set up video encoder for this segment.
        let mut vid_enc: Option<(usize, encoder::Video)> = None;
        if let Some((vid_ist_idx, ref vid_dec)) = video_decoder {
            let ost_idx = stream_mapping[vid_ist_idx] as usize;
            let ist_tb = stream_configs[ost_idx].ist_time_base;

            let enc_codec = encoder::find_by_name(&opts.video_codec).unwrap();
            let mut enc = codec::context::Context::new_with_codec(enc_codec)
                .encoder()
                .video()
                .map_err(|e| MediaError::Concat(format!("segment video encoder: {e}")))?;

            enc.set_height(vid_dec.height());
            enc.set_width(vid_dec.width());
            enc.set_aspect_ratio(vid_dec.aspect_ratio());
            enc.set_format(vid_dec.format());
            enc.set_frame_rate(vid_dec.frame_rate());
            enc.set_time_base(ist_tb);

            if global_header {
                enc.set_flags(codec::Flags::GLOBAL_HEADER);
            }

            let mut enc_opts = Dictionary::new();
            enc_opts.set("crf", &opts.crf.to_string());
            enc_opts.set("preset", "medium");

            let opened = enc
                .open_with(enc_opts)
                .map_err(|e| MediaError::Concat(format!("open segment video encoder: {e}")))?;
            vid_enc = Some((ost_idx, opened));
        }

        // Set up audio encoder + resampler for this segment.
        let mut aud_enc: Option<(usize, encoder::Audio, Option<software::resampling::Context>)> =
            None;
        if let Some((aud_ist_idx, ref aud_dec)) = audio_decoder {
            let ost_idx = stream_mapping[aud_ist_idx] as usize;

            let enc_codec = encoder::find_by_name(&opts.audio_codec).ok_or_else(|| {
                MediaError::Concat(format!("audio encoder not found: {}", opts.audio_codec))
            })?;
            let mut enc = codec::context::Context::new_with_codec(enc_codec)
                .encoder()
                .audio()
                .map_err(|e| MediaError::Concat(format!("segment audio encoder: {e}")))?;

            let channel_layout = if aud_dec.channel_layout() != ChannelLayout::default(0) {
                aud_dec.channel_layout()
            } else {
                ChannelLayout::STEREO
            };

            enc.set_rate(opts.audio_rate as i32);
            enc.set_channel_layout(channel_layout);
            enc.set_format(
                enc_codec
                    .audio()
                    .unwrap()
                    .formats()
                    .unwrap()
                    .next()
                    .unwrap(),
            );
            enc.set_time_base(Rational(1, opts.audio_rate as i32));

            if global_header {
                enc.set_flags(codec::Flags::GLOBAL_HEADER);
            }

            let opened = enc
                .open_with(Dictionary::new())
                .map_err(|e| MediaError::Concat(format!("open segment audio encoder: {e}")))?;

            // Set up resampler if formats differ.
            let resampler = if aud_dec.format() != opened.format()
                || aud_dec.rate() != opened.rate()
                || aud_dec.channel_layout() != opened.channel_layout()
            {
                let r = software::resampling::Context::get(
                    aud_dec.format(),
                    aud_dec.channel_layout(),
                    aud_dec.rate(),
                    opened.format(),
                    opened.channel_layout(),
                    opened.rate(),
                )
                .map_err(|e| MediaError::Concat(format!("audio resampler: {e}")))?;
                Some(r)
            } else {
                None
            };

            aud_enc = Some((ost_idx, opened, resampler));
        }

        // Process packets for this segment (both video and audio).
        for (stream, packet) in ictx.packets() {
            let ist_idx = stream.index();
            if ist_idx >= stream_mapping.len() || stream_mapping[ist_idx] < 0 {
                continue;
            }

            // Video packets.
            if let Some((vid_ist_idx, ref mut vid_dec)) = video_decoder
                && ist_idx == vid_ist_idx
            {
                if let Some((v_ost_idx, ref mut v_enc)) = vid_enc {
                    let ost_tb = ost_time_bases[v_ost_idx];
                    let ist_tb = stream_configs[v_ost_idx].ist_time_base;

                    let _ = vid_dec.send_packet(&packet);
                    let mut vframe = frame::Video::empty();
                    while vid_dec.receive_frame(&mut vframe).is_ok() {
                        let ts = vframe.timestamp();
                        vframe.set_pts(ts);
                        vframe.set_kind(picture::Type::None);

                        if let Some(pts) = vframe.pts() {
                            let rescaled = pts.rescale(ist_tb, ost_tb);
                            let offset_pts = rescaled + pts_offsets[v_ost_idx];
                            vframe.set_pts(Some(offset_pts));
                            last_pts[v_ost_idx] = offset_pts;
                        }
                        last_duration[v_ost_idx] = 1;

                        let _ = v_enc.send_frame(&vframe);
                        drain_video_encoder(v_enc, v_ost_idx, &mut octx);
                    }
                }
                continue;
            }

            // Audio packets.
            if let Some((aud_ist_idx, ref mut aud_dec)) = audio_decoder
                && ist_idx == aud_ist_idx
                && let Some((a_ost_idx, ref mut a_enc, ref mut resampler)) = aud_enc
            {
                let ost_tb = ost_time_bases[a_ost_idx];
                let ist_tb = stream_configs[a_ost_idx].ist_time_base;

                let _ = aud_dec.send_packet(&packet);
                let mut aframe = frame::Audio::empty();
                while aud_dec.receive_frame(&mut aframe).is_ok() {
                    let ts = aframe.timestamp();
                    aframe.set_pts(ts);

                    // Resample if needed, then encode.
                    let frame_to_encode = if let Some(r) = resampler {
                        let mut resampled = frame::Audio::empty();
                        if r.run(&aframe, &mut resampled).is_ok() {
                            resampled.set_pts(aframe.pts());
                            resampled
                        } else {
                            continue;
                        }
                    } else {
                        aframe.clone()
                    };

                    // Apply PTS offset.
                    if let Some(pts) = frame_to_encode.pts() {
                        let rescaled = pts.rescale(ist_tb, ost_tb);
                        let offset_pts = rescaled + pts_offsets[a_ost_idx];
                        // frame_to_encode is already consumed, set on encoder frame
                        let mut offset_frame = frame_to_encode;
                        offset_frame.set_pts(Some(offset_pts));
                        last_pts[a_ost_idx] = offset_pts;
                        last_duration[a_ost_idx] = 1;
                        let _ = a_enc.send_frame(&offset_frame);
                    } else {
                        let _ = a_enc.send_frame(&frame_to_encode);
                    }
                    drain_audio_encoder(a_enc, a_ost_idx, &mut octx);
                }
            }
        }

        // Flush video decoder/encoder for this segment.
        if let Some((_vid_ist_idx, ref mut vid_dec)) = video_decoder
            && let Some((v_ost_idx, ref mut v_enc)) = vid_enc
        {
            let ost_tb = ost_time_bases[v_ost_idx];
            let ist_tb = stream_configs[v_ost_idx].ist_time_base;

            let _ = vid_dec.send_eof();
            let mut vframe = frame::Video::empty();
            while vid_dec.receive_frame(&mut vframe).is_ok() {
                let ts = vframe.timestamp();
                vframe.set_pts(ts);
                vframe.set_kind(picture::Type::None);
                if let Some(pts) = vframe.pts() {
                    let rescaled = pts.rescale(ist_tb, ost_tb);
                    let offset_pts = rescaled + pts_offsets[v_ost_idx];
                    vframe.set_pts(Some(offset_pts));
                    last_pts[v_ost_idx] = offset_pts;
                }
                last_duration[v_ost_idx] = 1;
                let _ = v_enc.send_frame(&vframe);
                drain_video_encoder(v_enc, v_ost_idx, &mut octx);
            }
            let _ = v_enc.send_eof();
            drain_video_encoder(v_enc, v_ost_idx, &mut octx);
        }

        // Flush audio decoder/encoder for this segment.
        if let Some((_aud_ist_idx, ref mut aud_dec)) = audio_decoder
            && let Some((a_ost_idx, ref mut a_enc, ref mut resampler)) = aud_enc
        {
            let ost_tb = ost_time_bases[a_ost_idx];
            let ist_tb = stream_configs[a_ost_idx].ist_time_base;

            let _ = aud_dec.send_eof();
            let mut aframe = frame::Audio::empty();
            while aud_dec.receive_frame(&mut aframe).is_ok() {
                let ts = aframe.timestamp();
                aframe.set_pts(ts);

                let frame_to_encode = if let Some(r) = resampler {
                    let mut resampled = frame::Audio::empty();
                    if r.run(&aframe, &mut resampled).is_ok() {
                        resampled.set_pts(aframe.pts());
                        resampled
                    } else {
                        continue;
                    }
                } else {
                    aframe.clone()
                };

                if let Some(pts) = frame_to_encode.pts() {
                    let rescaled = pts.rescale(ist_tb, ost_tb);
                    let offset_pts = rescaled + pts_offsets[a_ost_idx];
                    let mut offset_frame = frame_to_encode;
                    offset_frame.set_pts(Some(offset_pts));
                    last_pts[a_ost_idx] = offset_pts;
                    last_duration[a_ost_idx] = 1;
                    let _ = a_enc.send_frame(&offset_frame);
                } else {
                    let _ = a_enc.send_frame(&frame_to_encode);
                }
                drain_audio_encoder(a_enc, a_ost_idx, &mut octx);
            }

            // Flush resampler delay.
            if let Some(r) = resampler {
                let mut delay = frame::Audio::empty();
                while r.flush(&mut delay).is_ok() && delay.samples() > 0 {
                    let _ = a_enc.send_frame(&delay);
                    drain_audio_encoder(a_enc, a_ost_idx, &mut octx);
                }
            }

            let _ = a_enc.send_eof();
            drain_audio_encoder(a_enc, a_ost_idx, &mut octx);
        }
    }

    octx.write_trailer()
        .map_err(|e| MediaError::Concat(format!("failed to write trailer: {e}")))?;

    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum StreamKind {
    Video,
    Audio,
}

#[derive(Debug, Clone)]
struct StreamConfig {
    kind: StreamKind,
    ist_time_base: Rational,
}

/// Drain encoded packets from a video encoder and write to output.
fn drain_video_encoder(
    encoder: &mut encoder::Video,
    ost_index: usize,
    octx: &mut format::context::Output,
) {
    let mut encoded = Packet::empty();
    while encoder.receive_packet(&mut encoded).is_ok() {
        encoded.set_stream(ost_index);
        let _ = encoded.write_interleaved(octx);
    }
}

/// Drain encoded packets from an audio encoder and write to output.
fn drain_audio_encoder(
    encoder: &mut encoder::Audio,
    ost_index: usize,
    octx: &mut format::context::Output,
) {
    let mut encoded = Packet::empty();
    while encoder.receive_packet(&mut encoded).is_ok() {
        encoded.set_stream(ost_index);
        let _ = encoded.write_interleaved(octx);
    }
}

// ── Subprocess helpers (kept for SubprocessBackend) ─────────────────

/// Build a concat demuxer list file and return its path.
fn build_concat_file(segments: &[&Path]) -> Result<String, MediaError> {
    let mut tmp = tempfile::NamedTempFile::new()
        .map_err(|e| MediaError::Concat(format!("failed to create temp file: {e}")))?;

    for seg in segments {
        writeln!(tmp, "file '{}'", seg.display())
            .map_err(|e| MediaError::Concat(format!("failed to write concat list: {e}")))?;
    }

    let path = tmp.into_temp_path();
    let path_str = path.to_str().unwrap_or("").to_string();
    path.keep()
        .map_err(|e| MediaError::Concat(format!("failed to persist temp file: {e}")))?;

    Ok(path_str)
}

/// Build ffmpeg CLI arguments for the concat operation.
pub(crate) fn build_concat_args(
    concat_file: &str,
    output: &Path,
    opts: &ConcatOptions,
) -> Vec<String> {
    let mut args = vec![
        "-y".to_string(),
        "-f".to_string(),
        "concat".to_string(),
        "-safe".to_string(),
        "0".to_string(),
        "-i".to_string(),
        concat_file.to_string(),
    ];

    if opts.copy {
        args.extend(["-c".to_string(), "copy".to_string()]);
    } else {
        args.extend([
            "-c:v".to_string(),
            opts.video_codec.clone(),
            "-crf".to_string(),
            opts.crf.to_string(),
            "-c:a".to_string(),
            opts.audio_codec.clone(),
            "-ar".to_string(),
            opts.audio_rate.to_string(),
        ]);
    }

    args.push(output.display().to_string());
    args
}

/// Run ffmpeg with the given arguments.
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

    fn default_copy_opts() -> ConcatOptions {
        ConcatOptions {
            copy: true,
            video_codec: "libx264".to_string(),
            crf: 23,
            audio_codec: "aac".to_string(),
            audio_rate: 48000,
        }
    }

    fn default_reencode_opts() -> ConcatOptions {
        ConcatOptions {
            copy: false,
            video_codec: "libx264".to_string(),
            crf: 18,
            audio_codec: "aac".to_string(),
            audio_rate: 44100,
        }
    }

    /// Create a small test video file using the system ffmpeg binary.
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

    // ── Validation tests ────────────────────────────────────────────

    #[test]
    fn test_validate_segments_empty() {
        let result = validate_segments(&[]);
        assert!(result.is_err());
        match result.unwrap_err() {
            MediaError::Concat(msg) => assert!(msg.contains("no segments")),
            other => panic!("expected Concat error, got {other:?}"),
        }
    }

    #[test]
    fn test_validate_segments_nonexistent() {
        let bad = Path::new("/tmp/nonexistent_segment_12345.mp4");
        let result = validate_segments(&[bad]);
        assert!(result.is_err());
        match result.unwrap_err() {
            MediaError::Concat(msg) => assert!(msg.contains("does not exist")),
            other => panic!("expected Concat error, got {other:?}"),
        }
    }

    #[test]
    fn test_validate_segments_ok() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("ok.mp4");
        std::fs::write(&f, b"dummy").unwrap();
        assert!(validate_segments(&[f.as_path()]).is_ok());
    }

    // ── Subprocess tests (kept for SubprocessBackend) ───────────────

    #[test]
    fn test_build_concat_args_copy_mode() {
        let opts = default_copy_opts();
        let args = build_concat_args("/tmp/list.txt", Path::new("/tmp/out.mp4"), &opts);

        assert!(args.contains(&"-y".to_string()));
        assert!(args.contains(&"concat".to_string()));
        assert!(args.contains(&"copy".to_string()));
        assert!(!args.contains(&"-crf".to_string()));
    }

    #[test]
    fn test_build_concat_args_reencode_mode() {
        let opts = default_reencode_opts();
        let args = build_concat_args("/tmp/list.txt", Path::new("/tmp/out.mp4"), &opts);

        assert!(args.contains(&"libx264".to_string()));
        assert!(args.contains(&"18".to_string()));
        assert!(args.contains(&"aac".to_string()));
        assert!(args.contains(&"44100".to_string()));
        assert!(!args.contains(&"copy".to_string()));
    }

    #[test]
    fn test_build_concat_args_output_path() {
        let opts = default_copy_opts();
        let args = build_concat_args("/tmp/list.txt", Path::new("/output/video.mp4"), &opts);
        assert_eq!(args.last().unwrap(), "/output/video.mp4");
    }

    #[test]
    fn test_build_concat_args_safe_flag() {
        let opts = default_copy_opts();
        let args = build_concat_args("/tmp/list.txt", Path::new("/tmp/out.mp4"), &opts);
        let safe_idx = args.iter().position(|a| a == "-safe").unwrap();
        assert_eq!(args[safe_idx + 1], "0");
    }

    #[test]
    fn test_build_concat_file() {
        let dir = tempfile::tempdir().unwrap();
        let seg1 = dir.path().join("a.mp4");
        let seg2 = dir.path().join("b.mp4");
        let segments: Vec<&Path> = vec![seg1.as_path(), seg2.as_path()];

        let path = build_concat_file(&segments).unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("file '"));
        assert!(contents.contains("a.mp4"));
        assert!(contents.contains("b.mp4"));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_run_ffmpeg_failure() {
        let result = run_ffmpeg(&[
            "-i".to_string(),
            "/nonexistent/file.mp4".to_string(),
            "/tmp/impossible_output.mp4".to_string(),
        ]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("ffmpeg exited with"));
    }

    // ── Subprocess integration tests ────────────────────────────────

    #[test]
    fn test_concat_subprocess_empty_segments() {
        let opts = default_copy_opts();
        let result = concat_subprocess(&[], Path::new("/tmp/out.mp4"), &opts);
        assert!(result.is_err());
    }

    #[test]
    fn test_concat_subprocess_copy_integration() {
        let dir = tempfile::tempdir().unwrap();
        let seg1 = dir.path().join("seg1.mp4");
        let seg2 = dir.path().join("seg2.mp4");
        create_test_video(&seg1);
        create_test_video(&seg2);

        let output = dir.path().join("out.mp4");
        let opts = default_copy_opts();
        let result = concat_subprocess(&[seg1.as_path(), seg2.as_path()], &output, &opts);
        assert!(result.is_ok(), "concat failed: {result:?}");
        assert!(output.exists());
        assert!(output.metadata().unwrap().len() > 0);
    }

    #[test]
    fn test_concat_subprocess_reencode_integration() {
        let dir = tempfile::tempdir().unwrap();
        let seg1 = dir.path().join("seg1.mp4");
        create_test_video(&seg1);

        let output = dir.path().join("out_reencode.mp4");
        let opts = default_reencode_opts();
        let result = concat_subprocess(&[seg1.as_path()], &output, &opts);
        assert!(result.is_ok(), "concat reencode failed: {result:?}");
        assert!(output.exists());
    }

    #[test]
    fn test_concat_subprocess_invalid_content() {
        let dir = tempfile::tempdir().unwrap();
        let bad_seg = dir.path().join("bad.mp4");
        std::fs::write(&bad_seg, b"not a video").unwrap();

        let output = dir.path().join("out.mp4");
        let opts = default_copy_opts();
        let result = concat_subprocess(&[bad_seg.as_path()], &output, &opts);
        assert!(result.is_err());
        match result.unwrap_err() {
            MediaError::Concat(msg) => assert!(msg.contains("ffmpeg concat failed")),
            other => panic!("expected Concat error, got {other:?}"),
        }
    }

    // ── Native concat tests ─────────────────────────────────────────

    #[test]
    fn test_concat_native_empty_segments() {
        let opts = default_copy_opts();
        let result = concat_native(&[], Path::new("/tmp/out.mp4"), &opts);
        assert!(result.is_err());
    }

    #[test]
    fn test_concat_native_nonexistent_segment() {
        let opts = default_copy_opts();
        let bad = Path::new("/tmp/nonexistent_segment_native.mp4");
        let result = concat_native(&[bad], Path::new("/tmp/out.mp4"), &opts);
        assert!(result.is_err());
    }

    #[test]
    fn test_concat_native_copy_single_segment() {
        let dir = tempfile::tempdir().unwrap();
        let seg1 = dir.path().join("seg1.mp4");
        create_test_video(&seg1);

        let output = dir.path().join("native_single.mp4");
        let opts = default_copy_opts();
        let result = concat_native(&[seg1.as_path()], &output, &opts);
        assert!(result.is_ok(), "native concat failed: {result:?}");
        assert!(output.exists());
        assert!(output.metadata().unwrap().len() > 0);
    }

    #[test]
    fn test_concat_native_copy_two_segments() {
        let dir = tempfile::tempdir().unwrap();
        let seg1 = dir.path().join("seg1.mp4");
        let seg2 = dir.path().join("seg2.mp4");
        create_test_video(&seg1);
        create_test_video(&seg2);

        let output = dir.path().join("native_two.mp4");
        let opts = default_copy_opts();
        let result = concat_native(&[seg1.as_path(), seg2.as_path()], &output, &opts);
        assert!(result.is_ok(), "native concat failed: {result:?}");
        assert!(output.exists());

        // Verify duration is roughly 2x a single segment.
        let info = crate::probe::probe(&output).unwrap();
        let dur = info.duration_secs.unwrap_or(0.0);
        assert!(dur > 1.5, "expected ~2s duration, got {dur}");
    }

    #[test]
    fn test_concat_native_reencode_single_segment() {
        let dir = tempfile::tempdir().unwrap();
        let seg1 = dir.path().join("seg1.mp4");
        create_test_video(&seg1);

        let output = dir.path().join("native_reencode.mp4");
        let opts = default_reencode_opts();
        let result = concat_native(&[seg1.as_path()], &output, &opts);
        assert!(result.is_ok(), "concat reencode failed: {result:?}");
        assert!(output.exists());

        let info = crate::probe::probe(&output).unwrap();
        assert!(info.duration_secs.unwrap_or(0.0) > 0.5);
    }

    #[test]
    fn test_concat_native_reencode_two_segments() {
        let dir = tempfile::tempdir().unwrap();
        let seg1 = dir.path().join("seg1.mp4");
        let seg2 = dir.path().join("seg2.mp4");
        create_test_video(&seg1);
        create_test_video(&seg2);

        let output = dir.path().join("native_reencode_two.mp4");
        let opts = default_reencode_opts();
        let result = concat_native(&[seg1.as_path(), seg2.as_path()], &output, &opts);
        assert!(result.is_ok(), "concat reencode failed: {result:?}");
        assert!(output.exists());

        // Verify duration is roughly 2x a single segment.
        let info = crate::probe::probe(&output).unwrap();
        let dur = info.duration_secs.unwrap_or(0.0);
        assert!(dur > 1.5, "expected ~2s duration, got {dur}");
    }

    #[test]
    fn test_concat_native_invalid_content() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.mp4");
        std::fs::write(&bad, b"not a video").unwrap();

        let output = dir.path().join("out.mp4");
        let opts = default_copy_opts();
        let result = concat_native(&[bad.as_path()], &output, &opts);
        assert!(result.is_err());
    }

    // ── ConcatOptions tests ─────────────────────────────────────────

    #[test]
    fn test_concat_options_clone_and_debug() {
        let opts = default_copy_opts();
        let cloned = opts.clone();
        assert!(cloned.copy);
        assert_eq!(cloned.video_codec, "libx264");

        let debug = format!("{opts:?}");
        assert!(debug.contains("ConcatOptions"));
    }

    // ── Audio transcoding concat tests ─────────────────────────────

    /// Create a test video WITH audio (sine wave + test pattern).
    fn create_test_video_with_audio(path: &Path) {
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=160x120:rate=15",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
            ])
            .arg(path.as_os_str())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("ffmpeg not found");
        assert!(status.success());
    }

    #[test]
    fn test_concat_native_reencode_with_audio() {
        let dir = tempfile::tempdir().unwrap();
        let seg1 = dir.path().join("av_seg1.mp4");
        create_test_video_with_audio(&seg1);

        let output = dir.path().join("av_reencode_out.mp4");
        let opts = default_reencode_opts();
        let result = concat_native(&[seg1.as_path()], &output, &opts);
        assert!(
            result.is_ok(),
            "concat reencode with audio failed: {result:?}"
        );
        assert!(output.exists());

        let info = crate::probe::probe(&output).unwrap();
        assert!(info.duration_secs.unwrap_or(0.0) > 0.5);
    }

    #[test]
    fn test_concat_native_reencode_two_segments_with_audio() {
        let dir = tempfile::tempdir().unwrap();
        let seg1 = dir.path().join("av_seg1.mp4");
        let seg2 = dir.path().join("av_seg2.mp4");
        create_test_video_with_audio(&seg1);
        create_test_video_with_audio(&seg2);

        let output = dir.path().join("av_reencode_two.mp4");
        let opts = default_reencode_opts();
        let result = concat_native(&[seg1.as_path(), seg2.as_path()], &output, &opts);
        assert!(
            result.is_ok(),
            "concat reencode 2 segments with audio failed: {result:?}"
        );
        assert!(output.exists());

        let info = crate::probe::probe(&output).unwrap();
        let dur = info.duration_secs.unwrap_or(0.0);
        assert!(dur > 1.5, "expected ~2s duration, got {dur}");
    }
}
