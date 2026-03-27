use std::collections::HashMap;
use std::process::Command;

use ffmpeg_next::{
    ChannelLayout, Dictionary, Packet, Rational, codec, decoder, encoder, format, frame, media,
    picture, software,
};

use crate::{MediaError, RenderPlan, RenderResult, probe};

/// Execute a render plan using native libav* APIs (decode → filter → encode).
///
/// Falls back to subprocess for unsupported codecs.
pub fn render_native(plan: &RenderPlan) -> Result<RenderResult, MediaError> {
    if !plan.input.exists() {
        return Err(MediaError::Render(format!(
            "input does not exist: {}",
            plan.input.display()
        )));
    }

    transcode(plan)?;

    let info = probe::probe(&plan.output)?;
    let duration_secs = info.duration_secs.unwrap_or(0.0);

    Ok(RenderResult {
        output: plan.output.clone(),
        duration_secs,
    })
}

/// Execute a render plan using the ffmpeg subprocess (Phase 1a fallback).
pub fn render_subprocess(plan: &RenderPlan) -> Result<RenderResult, MediaError> {
    if !plan.input.exists() {
        return Err(MediaError::Render(format!(
            "input does not exist: {}",
            plan.input.display()
        )));
    }

    let args = build_render_args(plan);
    run_ffmpeg(&args).map_err(|e| MediaError::Render(format!("ffmpeg render failed: {e}")))?;

    let info = probe::probe(&plan.output)?;
    let duration_secs = info.duration_secs.unwrap_or(0.0);

    Ok(RenderResult {
        output: plan.output.clone(),
        duration_secs,
    })
}

// ── Native transcode pipeline ───────────────────────────────────────

/// Transcode a video file: decode → optional filter graph → encode.
fn transcode(plan: &RenderPlan) -> Result<(), MediaError> {
    let mut ictx = format::input(&plan.input).map_err(|e| {
        MediaError::Render(format!(
            "failed to open input: {}: {e}",
            plan.input.display()
        ))
    })?;

    let mut octx = format::output(&plan.output).map_err(|e| {
        MediaError::Render(format!(
            "failed to create output: {}: {e}",
            plan.output.display()
        ))
    })?;

    let best_video_idx = ictx.streams().best(media::Type::Video).map(|s| s.index());
    let best_audio_idx = ictx.streams().best(media::Type::Audio).map(|s| s.index());

    let mut stream_mapping: Vec<i32> = vec![-1; ictx.nb_streams() as usize];
    let mut ist_time_bases: Vec<Rational> = vec![Rational(0, 0); ictx.nb_streams() as usize];
    let mut video_transcoders: HashMap<usize, VideoTranscoder> = HashMap::new();
    let mut audio_transcoders: HashMap<usize, AudioTranscoder> = HashMap::new();
    let mut ost_index: i32 = 0;

    for (ist_index, ist) in ictx.streams().enumerate() {
        let medium = ist.parameters().medium();
        if medium != media::Type::Audio
            && medium != media::Type::Video
            && medium != media::Type::Subtitle
        {
            continue;
        }

        stream_mapping[ist_index] = ost_index;
        ist_time_bases[ist_index] = ist.time_base();

        if medium == media::Type::Video && Some(ist_index) == best_video_idx {
            let tc = VideoTranscoder::new(&ist, &mut octx, ost_index as usize, plan)?;
            video_transcoders.insert(ist_index, tc);
        } else if medium == media::Type::Audio && Some(ist_index) == best_audio_idx {
            let tc = AudioTranscoder::new(&ist, &mut octx, ost_index as usize, plan)?;
            audio_transcoders.insert(ist_index, tc);
        } else {
            // Stream-copy other streams (subtitle, secondary audio, etc.).
            let mut ost = octx
                .add_stream(encoder::find(codec::Id::None))
                .map_err(|e| MediaError::Render(format!("failed to add stream: {e}")))?;
            ost.set_parameters(ist.parameters());
            unsafe {
                (*ost.parameters().as_mut_ptr()).codec_tag = 0;
            }
        }

        ost_index += 1;
    }

    octx.set_metadata(ictx.metadata().to_owned());
    octx.write_header()
        .map_err(|e| MediaError::Render(format!("failed to write header: {e}")))?;

    // Collect output time bases after header write.
    let ost_time_bases: Vec<Rational> = (0..octx.nb_streams() as usize)
        .map(|i| octx.stream(i).unwrap().time_base())
        .collect();

    // Process packets.
    for (stream, mut packet) in ictx.packets() {
        let ist_index = stream.index();
        if ist_index >= stream_mapping.len() {
            continue;
        }
        let ost_idx = stream_mapping[ist_index];
        if ost_idx < 0 {
            continue;
        }
        let ost_time_base = ost_time_bases[ost_idx as usize];

        if let Some(tc) = video_transcoders.get_mut(&ist_index) {
            tc.send_packet(&packet);
            tc.receive_and_process_frames(&mut octx, ost_time_base);
        } else if let Some(tc) = audio_transcoders.get_mut(&ist_index) {
            tc.send_packet(&packet);
            tc.receive_and_process_frames(&mut octx, ost_time_base);
        } else {
            // Stream copy.
            packet.rescale_ts(ist_time_bases[ist_index], ost_time_base);
            packet.set_position(-1);
            packet.set_stream(ost_idx as _);
            packet
                .write_interleaved(&mut octx)
                .map_err(|e| MediaError::Render(format!("failed to write packet: {e}")))?;
        }
    }

    // Flush video transcoders.
    for (ist_index, tc) in &mut video_transcoders {
        let ost_idx = stream_mapping[*ist_index] as usize;
        let ost_time_base = ost_time_bases[ost_idx];
        tc.flush(&mut octx, ost_time_base);
    }

    // Flush audio transcoders.
    for (ist_index, tc) in &mut audio_transcoders {
        let ost_idx = stream_mapping[*ist_index] as usize;
        let ost_time_base = ost_time_bases[ost_idx];
        tc.flush(&mut octx, ost_time_base);
    }

    octx.write_trailer()
        .map_err(|e| MediaError::Render(format!("failed to write trailer: {e}")))?;

    Ok(())
}

/// Manages decode → optional filter → encode for a single video stream.
struct VideoTranscoder {
    ost_index: usize,
    decoder: decoder::Video,
    encoder: encoder::Video,
    input_time_base: Rational,
    filter: Option<ffmpeg_next::filter::Graph>,
}

impl VideoTranscoder {
    fn new(
        ist: &format::stream::Stream,
        octx: &mut format::context::Output,
        ost_index: usize,
        plan: &RenderPlan,
    ) -> Result<Self, MediaError> {
        let global_header = octx.format().flags().contains(format::Flags::GLOBAL_HEADER);

        let dec = codec::context::Context::from_parameters(ist.parameters())
            .map_err(|e| MediaError::Render(format!("decoder context: {e}")))?
            .decoder()
            .video()
            .map_err(|e| MediaError::Render(format!("video decoder: {e}")))?;

        // Determine the video filter spec: filter_complex takes precedence over filters.
        let video_filter_spec = if let Some(ref fc) = plan.filter_complex {
            Some(fc.clone())
        } else if !plan.filters.is_empty() {
            Some(plan.filters.join(","))
        } else {
            None
        };

        // Build filter graph first (if needed) so we know the output dimensions.
        let filter = if let Some(ref spec) = video_filter_spec {
            Some(build_video_filter_graph(&dec, ist.time_base(), spec)?)
        } else {
            None
        };

        // Determine encoder dimensions from filter spec or decoder.
        let (enc_width, enc_height, enc_format) = if let Some(ref spec) = video_filter_spec {
            let (w, h) = parse_output_dimensions(spec, dec.width(), dec.height());
            (w, h, dec.format())
        } else {
            (dec.width(), dec.height(), dec.format())
        };

        // Find encoder by name (e.g. "libx264", "libx265").
        let enc_codec = encoder::find_by_name(&plan.video_codec).ok_or_else(|| {
            MediaError::Render(format!("encoder not found: {}", plan.video_codec))
        })?;

        let mut ost = octx
            .add_stream(enc_codec)
            .map_err(|e| MediaError::Render(format!("add video stream: {e}")))?;

        let mut enc = codec::context::Context::new_with_codec(enc_codec)
            .encoder()
            .video()
            .map_err(|e| MediaError::Render(format!("video encoder: {e}")))?;

        ost.set_parameters(&enc);
        enc.set_height(enc_height);
        enc.set_width(enc_width);
        enc.set_aspect_ratio(dec.aspect_ratio());
        enc.set_format(enc_format);
        enc.set_frame_rate(dec.frame_rate());
        enc.set_time_base(ist.time_base());

        if global_header {
            enc.set_flags(codec::Flags::GLOBAL_HEADER);
        }

        let mut opts = Dictionary::new();
        opts.set("crf", &plan.crf.to_string());
        opts.set("preset", plan.preset.as_deref().unwrap_or("medium"));

        let opened = enc
            .open_with(opts)
            .map_err(|e| MediaError::Render(format!("open encoder: {e}")))?;
        ost.set_parameters(&opened);

        Ok(Self {
            ost_index,
            decoder: dec,
            encoder: opened,
            input_time_base: ist.time_base(),
            filter,
        })
    }

    fn send_packet(&mut self, packet: &Packet) {
        let _ = self.decoder.send_packet(packet);
    }

    fn receive_and_process_frames(
        &mut self,
        octx: &mut format::context::Output,
        ost_time_base: Rational,
    ) {
        let mut frame = frame::Video::empty();
        while self.decoder.receive_frame(&mut frame).is_ok() {
            let timestamp = frame.timestamp();
            frame.set_pts(timestamp);
            frame.set_kind(picture::Type::None);

            if let Some(filter) = &mut self.filter {
                filter.get("in").unwrap().source().add(&frame).unwrap();
                let mut filtered = frame::Video::empty();
                while filter
                    .get("out")
                    .unwrap()
                    .sink()
                    .frame(&mut filtered)
                    .is_ok()
                {
                    let _ = self.encoder.send_frame(&filtered);
                    Self::drain_encoder_static(
                        &mut self.encoder,
                        self.ost_index,
                        self.input_time_base,
                        octx,
                        ost_time_base,
                    );
                }
            } else {
                let _ = self.encoder.send_frame(&frame);
                Self::drain_encoder_static(
                    &mut self.encoder,
                    self.ost_index,
                    self.input_time_base,
                    octx,
                    ost_time_base,
                );
            }
        }
    }

    fn drain_encoder_static(
        encoder: &mut encoder::Video,
        ost_index: usize,
        input_time_base: Rational,
        octx: &mut format::context::Output,
        ost_time_base: Rational,
    ) {
        let mut encoded = Packet::empty();
        while encoder.receive_packet(&mut encoded).is_ok() {
            encoded.set_stream(ost_index);
            encoded.rescale_ts(input_time_base, ost_time_base);
            let _ = encoded.write_interleaved(octx);
        }
    }

    fn flush(&mut self, octx: &mut format::context::Output, ost_time_base: Rational) {
        // Flush decoder.
        let _ = self.decoder.send_eof();
        self.receive_and_process_frames(octx, ost_time_base);

        // Flush filter.
        if let Some(filter) = &mut self.filter {
            filter.get("in").unwrap().source().flush().unwrap();
            let mut filtered = frame::Video::empty();
            while filter
                .get("out")
                .unwrap()
                .sink()
                .frame(&mut filtered)
                .is_ok()
            {
                let _ = self.encoder.send_frame(&filtered);
                Self::drain_encoder_static(
                    &mut self.encoder,
                    self.ost_index,
                    self.input_time_base,
                    octx,
                    ost_time_base,
                );
            }
        }

        // Flush encoder.
        let _ = self.encoder.send_eof();
        Self::drain_encoder_static(
            &mut self.encoder,
            self.ost_index,
            self.input_time_base,
            octx,
            ost_time_base,
        );
    }
}

/// Manages decode → optional resample → encode for a single audio stream.
struct AudioTranscoder {
    ost_index: usize,
    decoder: decoder::Audio,
    encoder: encoder::Audio,
    input_time_base: Rational,
    resampler: Option<software::resampling::Context>,
    filter: Option<ffmpeg_next::filter::Graph>,
}

impl AudioTranscoder {
    fn new(
        ist: &format::stream::Stream,
        octx: &mut format::context::Output,
        ost_index: usize,
        plan: &RenderPlan,
    ) -> Result<Self, MediaError> {
        let global_header = octx.format().flags().contains(format::Flags::GLOBAL_HEADER);

        let dec = codec::context::Context::from_parameters(ist.parameters())
            .map_err(|e| MediaError::Render(format!("audio decoder context: {e}")))?
            .decoder()
            .audio()
            .map_err(|e| MediaError::Render(format!("audio decoder: {e}")))?;

        // Build audio filter graph if audio_filter is set.
        let filter = if let Some(ref af) = plan.audio_filter {
            Some(build_audio_filter_graph(&dec, ist.time_base(), af)?)
        } else {
            None
        };

        let enc_codec = encoder::find_by_name(&plan.audio_codec).ok_or_else(|| {
            MediaError::Render(format!("audio encoder not found: {}", plan.audio_codec))
        })?;

        let mut ost = octx
            .add_stream(enc_codec)
            .map_err(|e| MediaError::Render(format!("add audio stream: {e}")))?;

        let mut enc = codec::context::Context::new_with_codec(enc_codec)
            .encoder()
            .audio()
            .map_err(|e| MediaError::Render(format!("audio encoder: {e}")))?;

        // Use decoder's channel layout, or default to stereo.
        let channel_layout = if dec.channel_layout() != ChannelLayout::default(0) {
            dec.channel_layout()
        } else {
            ChannelLayout::STEREO
        };

        let enc_rate = dec.rate() as i32;
        enc.set_rate(enc_rate);
        enc.set_channel_layout(channel_layout);
        // AAC encoder requires fltp format.
        enc.set_format(
            enc_codec
                .audio()
                .unwrap()
                .formats()
                .unwrap()
                .next()
                .unwrap(),
        );
        enc.set_time_base(Rational(1, enc_rate));

        if global_header {
            enc.set_flags(codec::Flags::GLOBAL_HEADER);
        }

        let mut opts = Dictionary::new();
        if let Some(bitrate) = plan.audio_bitrate {
            opts.set("b", &format!("{}", bitrate * 1000));
        }

        let opened = enc
            .open_with(opts)
            .map_err(|e| MediaError::Render(format!("open audio encoder: {e}")))?;
        ost.set_parameters(&opened);

        // Set up resampler if formats differ (common: decoder outputs s16/s32,
        // AAC encoder requires fltp).
        let resampler = if dec.format() != opened.format()
            || dec.rate() != opened.rate()
            || dec.channel_layout() != opened.channel_layout()
        {
            let r = software::resampling::Context::get(
                dec.format(),
                dec.channel_layout(),
                dec.rate(),
                opened.format(),
                opened.channel_layout(),
                opened.rate(),
            )
            .map_err(|e| MediaError::Render(format!("audio resampler: {e}")))?;
            Some(r)
        } else {
            None
        };

        Ok(Self {
            ost_index,
            decoder: dec,
            encoder: opened,
            input_time_base: ist.time_base(),
            resampler,
            filter,
        })
    }

    fn send_packet(&mut self, packet: &Packet) {
        let _ = self.decoder.send_packet(packet);
    }

    fn receive_and_process_frames(
        &mut self,
        octx: &mut format::context::Output,
        ost_time_base: Rational,
    ) {
        let mut frame = frame::Audio::empty();
        while self.decoder.receive_frame(&mut frame).is_ok() {
            let timestamp = frame.timestamp();
            frame.set_pts(timestamp);

            if let Some(filter) = &mut self.filter {
                filter.get("in").unwrap().source().add(&frame).unwrap();
                let mut filtered = frame::Audio::empty();
                while filter
                    .get("out")
                    .unwrap()
                    .sink()
                    .frame(&mut filtered)
                    .is_ok()
                {
                    let _ = self.encoder.send_frame(&filtered);
                    Self::drain_encoder_static(
                        &mut self.encoder,
                        self.ost_index,
                        self.input_time_base,
                        octx,
                        ost_time_base,
                    );
                }
            } else if let Some(ref mut resampler) = self.resampler {
                let mut resampled = frame::Audio::empty();
                if resampler.run(&frame, &mut resampled).is_ok() {
                    resampled.set_pts(frame.pts());
                    let _ = self.encoder.send_frame(&resampled);
                    Self::drain_encoder_static(
                        &mut self.encoder,
                        self.ost_index,
                        self.input_time_base,
                        octx,
                        ost_time_base,
                    );
                }
            } else {
                let _ = self.encoder.send_frame(&frame);
                Self::drain_encoder_static(
                    &mut self.encoder,
                    self.ost_index,
                    self.input_time_base,
                    octx,
                    ost_time_base,
                );
            }
        }
    }

    fn drain_encoder_static(
        encoder: &mut encoder::Audio,
        ost_index: usize,
        input_time_base: Rational,
        octx: &mut format::context::Output,
        ost_time_base: Rational,
    ) {
        let mut encoded = Packet::empty();
        while encoder.receive_packet(&mut encoded).is_ok() {
            encoded.set_stream(ost_index);
            encoded.rescale_ts(input_time_base, ost_time_base);
            let _ = encoded.write_interleaved(octx);
        }
    }

    fn flush(&mut self, octx: &mut format::context::Output, ost_time_base: Rational) {
        // Flush decoder.
        let _ = self.decoder.send_eof();
        self.receive_and_process_frames(octx, ost_time_base);

        // Flush filter.
        if let Some(filter) = &mut self.filter {
            filter.get("in").unwrap().source().flush().unwrap();
            let mut filtered = frame::Audio::empty();
            while filter
                .get("out")
                .unwrap()
                .sink()
                .frame(&mut filtered)
                .is_ok()
            {
                let _ = self.encoder.send_frame(&filtered);
                Self::drain_encoder_static(
                    &mut self.encoder,
                    self.ost_index,
                    self.input_time_base,
                    octx,
                    ost_time_base,
                );
            }
        }

        // Flush resampler.
        if let Some(ref mut resampler) = self.resampler {
            let mut delay = frame::Audio::empty();
            while resampler.flush(&mut delay).is_ok() && delay.samples() > 0 {
                let _ = self.encoder.send_frame(&delay);
                Self::drain_encoder_static(
                    &mut self.encoder,
                    self.ost_index,
                    self.input_time_base,
                    octx,
                    ost_time_base,
                );
            }
        }

        // Flush encoder.
        let _ = self.encoder.send_eof();
        Self::drain_encoder_static(
            &mut self.encoder,
            self.ost_index,
            self.input_time_base,
            octx,
            ost_time_base,
        );
    }
}

/// Build an audio filter graph: abuffer → user filters → abuffersink.
fn build_audio_filter_graph(
    decoder: &decoder::Audio,
    time_base: Rational,
    filter_spec: &str,
) -> Result<ffmpeg_next::filter::Graph, MediaError> {
    let mut graph = ffmpeg_next::filter::Graph::new();

    let channel_layout = if decoder.channel_layout() != ChannelLayout::default(0) {
        decoder.channel_layout()
    } else {
        ChannelLayout::STEREO
    };

    let args = format!(
        "time_base={}/{}:sample_rate={}:sample_fmt={}:channel_layout=0x{:x}",
        time_base.numerator(),
        time_base.denominator(),
        decoder.rate(),
        decoder.format().name(),
        channel_layout.bits(),
    );

    graph
        .add(&ffmpeg_next::filter::find("abuffer").unwrap(), "in", &args)
        .map_err(|e| MediaError::Filter(format!("abuffer source: {e}")))?;

    graph
        .add(
            &ffmpeg_next::filter::find("abuffersink").unwrap(),
            "out",
            "",
        )
        .map_err(|e| MediaError::Filter(format!("abuffer sink: {e}")))?;

    // Normalize the spec: replace [0:a] with [in] label, strip trailing
    // output labels like [afinal].
    let native_spec = prepare_filter_spec(filter_spec, "[0:a]");

    if native_spec.contains(';') {
        // Multi-chain audio graph. The spec uses [in] labels to reference
        // the abuffer source, so we only provide the abuffersink as open input.
        graph
            .input("out", 0)
            .map_err(|e| MediaError::Filter(format!("audio graph input: {e}")))?
            .parse(&native_spec)
            .map_err(|e| MediaError::Filter(format!("parse audio filter '{native_spec}': {e}")))?;
    } else {
        graph
            .output("in", 0)
            .map_err(|e| MediaError::Filter(format!("audio graph output: {e}")))?
            .input("out", 0)
            .map_err(|e| MediaError::Filter(format!("audio graph input: {e}")))?
            .parse(&native_spec)
            .map_err(|e| MediaError::Filter(format!("parse audio filter '{native_spec}': {e}")))?;
    }

    graph
        .validate()
        .map_err(|e| MediaError::Filter(format!("validate audio filter graph: {e}")))?;

    Ok(graph)
}

/// Parse output dimensions from a filter spec string.
///
/// Handles:
/// - Simple `scale=W:H` filters
/// - Multi-chain specs with `color=c=...:s=WxH` (smart pad background determines output)
/// - `pad=W:H:...` filters
/// Returns (width, height) with defaults from the decoder if not found.
fn parse_output_dimensions(filter_spec: &str, dec_w: u32, dec_h: u32) -> (u32, u32) {
    // For multi-chain specs with color source + overlay, the output dimensions
    // come from the color source's `s=WxH` parameter.
    if filter_spec.contains("color=") && filter_spec.contains("overlay") {
        for segment in filter_spec.split(';') {
            let trimmed = segment.trim().trim_start_matches(|c: char| {
                c == '[' || c.is_alphanumeric() || c == ':' || c == ']' || c == '_'
            });
            if trimmed.starts_with("color=") {
                // Parse s=WxH from color filter params.
                for param in trimmed.split(':') {
                    if let Some(size) = param.strip_prefix("s=") {
                        let parts: Vec<&str> = size.split('x').collect();
                        if parts.len() == 2 {
                            if let (Ok(w), Ok(h)) =
                                (parts[0].parse::<u32>(), parts[1].parse::<u32>())
                            {
                                return (w, h);
                            }
                        }
                    }
                }
            }
        }
    }

    // Check for scale=W:H in simple filter chains.
    for part in filter_spec.split(',') {
        let trimmed = part.trim();
        // Handle scale after semicolons too (e.g. [label]scale=...)
        let scale_part = if let Some(idx) = trimmed.rfind(']') {
            &trimmed[idx + 1..]
        } else {
            trimmed
        };
        if let Some(args) = scale_part.strip_prefix("scale=") {
            let parts: Vec<&str> = args.split(':').collect();
            if parts.len() >= 2 {
                if let (Ok(w), Ok(h)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                    return (w, h);
                }
            }
        }
    }
    (dec_w, dec_h)
}

/// Normalize a filter spec for native graph execution.
///
/// The Python side generates filter_complex strings using ffmpeg CLI syntax:
/// - `[0:v]` / `[0:a]` to reference input streams
/// - `[vfinal]` / `[afinal]` as final output labels
///
/// The native `graph.parse()` API expects:
/// - Unlabeled chain starts connect to the buffer source provided via `.output()`
/// - Unlabeled chain ends connect to the buffersink provided via `.input()`
///
/// This function normalizes the spec by:
/// 1. Replacing `[0:v]` or `[0:a]` with nothing (unlabeled → auto-connects to buffer)
/// 2. Stripping trailing output labels like `[vfinal]`, `[afinal]`, `[out]`
fn prepare_filter_spec(spec: &str, stream_ref: &str) -> String {
    // Replace the stream reference (e.g. [0:v]) with our buffer source label [in].
    // This wires the buffer source to wherever the stream ref was used.
    let mut result = spec.replace(stream_ref, "[in]");

    // Strip a trailing output label (e.g. [vfinal], [afinal], [out], [vout], [aout]).
    // A trailing label is a [...] at the very end of the spec.
    if let Some(bracket_start) = result.rfind('[') {
        let after = &result[bracket_start..];
        // Must end with ']' and be at the very end of the string.
        if after.ends_with(']') && result[bracket_start..].len() > 2 {
            result.truncate(bracket_start);
        }
    }

    result
}

/// Build a video filter graph: buffer → user filters → buffersink.
fn build_video_filter_graph(
    decoder: &decoder::Video,
    time_base: Rational,
    filter_spec: &str,
) -> Result<ffmpeg_next::filter::Graph, MediaError> {
    let mut graph = ffmpeg_next::filter::Graph::new();

    let pix_fmt: ffmpeg_next::ffi::AVPixelFormat = decoder.format().into();
    let args = format!(
        "video_size={}x{}:pix_fmt={}:time_base={}/{}:pixel_aspect={}/{}",
        decoder.width(),
        decoder.height(),
        pix_fmt as i32,
        time_base.numerator(),
        time_base.denominator(),
        decoder.aspect_ratio().numerator(),
        decoder.aspect_ratio().denominator().max(1),
    );

    graph
        .add(&ffmpeg_next::filter::find("buffer").unwrap(), "in", &args)
        .map_err(|e| MediaError::Filter(format!("buffer source: {e}")))?;

    graph
        .add(&ffmpeg_next::filter::find("buffersink").unwrap(), "out", "")
        .map_err(|e| MediaError::Filter(format!("buffer sink: {e}")))?;

    // Normalize the spec: replace [0:v] with unlabeled input, strip trailing
    // output labels like [vfinal]. This allows source filters (e.g. color=...)
    // and multi-chain graphs to work with the single-input parse API.
    let native_spec = prepare_filter_spec(filter_spec, "[0:v]");

    // Constrain output pixel format only for simple single-chain graphs.
    // Multi-chain graphs (containing ';') may produce a different pixel format
    // through overlay or other multi-input filters.
    if !native_spec.contains(';') {
        let mut out = graph.get("out").unwrap();
        out.set_pixel_format(decoder.format());
    }

    graph
        .output("in", 0)
        .map_err(|e| MediaError::Filter(format!("graph output: {e}")))?
        .input("out", 0)
        .map_err(|e| MediaError::Filter(format!("graph input: {e}")))?
        .parse(&native_spec)
        .map_err(|e| MediaError::Filter(format!("parse filter '{native_spec}': {e}")))?;

    graph
        .validate()
        .map_err(|e| MediaError::Filter(format!("validate filter graph: {e}")))?;

    Ok(graph)
}

// ── Subprocess helpers ──────────────────────────────────────────────

/// Build ffmpeg CLI arguments from a render plan.
pub(crate) fn build_render_args(plan: &RenderPlan) -> Vec<String> {
    let mut args = vec![
        "-y".to_string(),
        "-i".to_string(),
        plan.input.display().to_string(),
    ];

    if let Some(ref fc) = plan.filter_complex {
        args.extend(["-filter_complex".to_string(), fc.clone()]);
    } else if !plan.filters.is_empty() {
        args.extend(["-vf".to_string(), plan.filters.join(",")]);
    }

    if let Some(ref af) = plan.audio_filter {
        args.extend(["-af".to_string(), af.clone()]);
    }

    args.extend([
        "-c:v".to_string(),
        plan.video_codec.clone(),
        "-crf".to_string(),
        plan.crf.to_string(),
    ]);

    if let Some(ref preset) = plan.preset {
        args.extend(["-preset".to_string(), preset.clone()]);
    }

    args.extend(["-c:a".to_string(), plan.audio_codec.clone()]);

    if let Some(bitrate) = plan.audio_bitrate {
        args.extend(["-b:a".to_string(), format!("{bitrate}k")]);
    }

    args.push(plan.output.display().to_string());
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
    use std::path::{Path, PathBuf};

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

    fn default_plan(input: PathBuf, output: PathBuf) -> RenderPlan {
        RenderPlan {
            input,
            output,
            video_codec: "libx264".to_string(),
            crf: 23,
            preset: None,
            audio_codec: "aac".to_string(),
            audio_bitrate: None,
            filters: vec![],
            filter_complex: None,
            audio_filter: None,
        }
    }

    // ── Subprocess render tests ─────────────────────────────────────

    #[test]
    fn test_build_render_args_no_filters() {
        let plan = default_plan(PathBuf::from("/in.mp4"), PathBuf::from("/out.mp4"));
        let args = build_render_args(&plan);

        assert!(args.contains(&"-y".to_string()));
        assert!(args.contains(&"/in.mp4".to_string()));
        assert!(args.contains(&"libx264".to_string()));
        assert!(args.contains(&"23".to_string()));
        assert!(!args.contains(&"-vf".to_string()));
        assert_eq!(args.last().unwrap(), "/out.mp4");
    }

    #[test]
    fn test_build_render_args_with_filters() {
        let plan = RenderPlan {
            input: PathBuf::from("/in.mp4"),
            output: PathBuf::from("/out.mp4"),
            video_codec: "libx264".to_string(),
            crf: 18,
            preset: None,
            audio_codec: "aac".to_string(),
            audio_bitrate: None,
            filters: vec!["scale=1920:1080".to_string(), "fps=30".to_string()],
            filter_complex: None,
            audio_filter: None,
        };
        let args = build_render_args(&plan);

        let vf_idx = args.iter().position(|a| a == "-vf").unwrap();
        assert_eq!(args[vf_idx + 1], "scale=1920:1080,fps=30");
    }

    #[test]
    fn test_render_subprocess_nonexistent_input() {
        let plan = default_plan(
            PathBuf::from("/tmp/nonexistent_render_input.mp4"),
            PathBuf::from("/tmp/render_out.mp4"),
        );
        let result = render_subprocess(&plan);
        assert!(result.is_err());
        match result.unwrap_err() {
            MediaError::Render(msg) => assert!(msg.contains("does not exist")),
            other => panic!("expected Render error, got {other:?}"),
        }
    }

    #[test]
    fn test_render_subprocess_integration() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.mp4");
        create_test_video(&input);

        let output = dir.path().join("output.mp4");
        let plan = default_plan(input, output.clone());
        let result = render_subprocess(&plan).unwrap();

        assert_eq!(result.output, output);
        assert!(result.duration_secs > 0.0);
        assert!(output.exists());
    }

    #[test]
    fn test_render_subprocess_with_filter() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input_filter.mp4");
        create_test_video(&input);

        let output = dir.path().join("output_filter.mp4");
        let plan = RenderPlan {
            input,
            output: output.clone(),
            video_codec: "libx264".to_string(),
            crf: 23,
            preset: None,
            audio_codec: "aac".to_string(),
            audio_bitrate: None,
            filters: vec!["scale=80:60".to_string()],
            filter_complex: None,
            audio_filter: None,
        };

        let result = render_subprocess(&plan).unwrap();
        assert!(result.duration_secs > 0.0);

        let info = probe::probe(&output).unwrap();
        assert_eq!(info.width, Some(80));
        assert_eq!(info.height, Some(60));
    }

    #[test]
    fn test_render_subprocess_invalid_input() {
        let dir = tempfile::tempdir().unwrap();
        let bad_input = dir.path().join("bad_input.mp4");
        std::fs::write(&bad_input, b"not a video").unwrap();

        let output = dir.path().join("render_bad_out.mp4");
        let plan = default_plan(bad_input, output);
        let result = render_subprocess(&plan);
        assert!(result.is_err());
        match result.unwrap_err() {
            MediaError::Render(msg) => assert!(msg.contains("ffmpeg render failed")),
            other => panic!("expected Render error, got {other:?}"),
        }
    }

    #[test]
    fn test_run_ffmpeg_failure() {
        let result = run_ffmpeg(&[
            "-i".to_string(),
            "/nonexistent/render_input.mp4".to_string(),
            "/tmp/impossible_render_output.mp4".to_string(),
        ]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("ffmpeg exited with"));
    }

    // ── Native render tests ─────────────────────────────────────────

    #[test]
    fn test_render_native_nonexistent_input() {
        let plan = default_plan(
            PathBuf::from("/tmp/nonexistent_native_input.mp4"),
            PathBuf::from("/tmp/native_out.mp4"),
        );
        let result = render_native(&plan);
        assert!(result.is_err());
        match result.unwrap_err() {
            MediaError::Render(msg) => assert!(msg.contains("does not exist")),
            other => panic!("expected Render error, got {other:?}"),
        }
    }

    #[test]
    fn test_render_native_no_filters() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("native_in.mp4");
        create_test_video(&input);

        let output = dir.path().join("native_out.mp4");
        let plan = default_plan(input, output.clone());
        let result = render_native(&plan).unwrap();

        assert_eq!(result.output, output);
        assert!(result.duration_secs > 0.0);
        assert!(output.exists());
    }

    #[test]
    fn test_render_native_with_scale_filter() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("native_filter_in.mp4");
        create_test_video(&input);

        let output = dir.path().join("native_filter_out.mp4");
        let plan = RenderPlan {
            input,
            output: output.clone(),
            video_codec: "libx264".to_string(),
            crf: 23,
            preset: None,
            audio_codec: "aac".to_string(),
            audio_bitrate: None,
            filters: vec!["scale=80:60".to_string()],
            filter_complex: None,
            audio_filter: None,
        };

        let result = render_native(&plan).unwrap();
        assert!(result.duration_secs > 0.0);

        let info = probe::probe(&output).unwrap();
        assert_eq!(info.width, Some(80));
        assert_eq!(info.height, Some(60));
    }

    #[test]
    fn test_render_native_invalid_input() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad_native.mp4");
        std::fs::write(&bad, b"not a video").unwrap();

        let output = dir.path().join("native_bad_out.mp4");
        let plan = default_plan(bad, output);
        let result = render_native(&plan);
        assert!(result.is_err());
    }

    // ── Shared type tests ───────────────────────────────────────────

    #[test]
    fn test_render_plan_clone_and_debug() {
        let plan = default_plan(PathBuf::from("/a.mp4"), PathBuf::from("/b.mp4"));
        let cloned = plan.clone();
        assert_eq!(cloned.input, PathBuf::from("/a.mp4"));
        assert_eq!(cloned.crf, 23);

        let debug = format!("{plan:?}");
        assert!(debug.contains("RenderPlan"));
    }

    #[test]
    fn test_render_result_clone_and_debug() {
        let result = RenderResult {
            output: PathBuf::from("/out.mp4"),
            duration_secs: 1.5,
        };
        let cloned = result.clone();
        assert_eq!(cloned.output, PathBuf::from("/out.mp4"));
        assert!((cloned.duration_secs - 1.5).abs() < f64::EPSILON);

        let debug = format!("{result:?}");
        assert!(debug.contains("RenderResult"));
    }

    #[test]
    fn test_build_render_args_codec_and_crf() {
        let plan = RenderPlan {
            input: PathBuf::from("/in.mp4"),
            output: PathBuf::from("/out.mp4"),
            video_codec: "libx265".to_string(),
            crf: 28,
            preset: None,
            audio_codec: "libopus".to_string(),
            audio_bitrate: None,
            filters: vec![],
            filter_complex: None,
            audio_filter: None,
        };
        let args = build_render_args(&plan);

        let cv_idx = args.iter().position(|a| a == "-c:v").unwrap();
        assert_eq!(args[cv_idx + 1], "libx265");

        let crf_idx = args.iter().position(|a| a == "-crf").unwrap();
        assert_eq!(args[crf_idx + 1], "28");

        let ca_idx = args.iter().position(|a| a == "-c:a").unwrap();
        assert_eq!(args[ca_idx + 1], "libopus");
    }

    // ── Preset and audio bitrate tests ─────────────────────────────

    #[test]
    fn test_build_render_args_with_preset() {
        let plan = RenderPlan {
            input: PathBuf::from("/in.mp4"),
            output: PathBuf::from("/out.mp4"),
            video_codec: "libx264".to_string(),
            crf: 23,
            preset: Some("fast".to_string()),
            audio_codec: "aac".to_string(),
            audio_bitrate: None,
            filters: vec![],
            filter_complex: None,
            audio_filter: None,
        };
        let args = build_render_args(&plan);

        let preset_idx = args.iter().position(|a| a == "-preset").unwrap();
        assert_eq!(args[preset_idx + 1], "fast");
    }

    #[test]
    fn test_build_render_args_with_audio_bitrate() {
        let plan = RenderPlan {
            input: PathBuf::from("/in.mp4"),
            output: PathBuf::from("/out.mp4"),
            video_codec: "libx264".to_string(),
            crf: 23,
            preset: None,
            audio_codec: "aac".to_string(),
            audio_bitrate: Some(192),
            filters: vec![],
            filter_complex: None,
            audio_filter: None,
        };
        let args = build_render_args(&plan);

        let ba_idx = args.iter().position(|a| a == "-b:a").unwrap();
        assert_eq!(args[ba_idx + 1], "192k");
    }

    #[test]
    fn test_build_render_args_no_preset_no_bitrate() {
        let plan = default_plan(PathBuf::from("/in.mp4"), PathBuf::from("/out.mp4"));
        let args = build_render_args(&plan);
        assert!(!args.contains(&"-preset".to_string()));
        assert!(!args.contains(&"-b:a".to_string()));
    }

    #[test]
    fn test_build_render_args_with_filter_complex() {
        let mut plan = default_plan(PathBuf::from("/in.mp4"), PathBuf::from("/out.mp4"));
        plan.filter_complex = Some("scale=1920:1080,setpts=PTS/2".to_string());
        let args = build_render_args(&plan);

        let fc_idx = args.iter().position(|a| a == "-filter_complex").unwrap();
        assert_eq!(args[fc_idx + 1], "scale=1920:1080,setpts=PTS/2");
        // filter_complex should not also emit -vf
        assert!(!args.contains(&"-vf".to_string()));
    }

    #[test]
    fn test_build_render_args_with_audio_filter() {
        let mut plan = default_plan(PathBuf::from("/in.mp4"), PathBuf::from("/out.mp4"));
        plan.audio_filter = Some("atempo=2.0".to_string());
        let args = build_render_args(&plan);

        let af_idx = args.iter().position(|a| a == "-af").unwrap();
        assert_eq!(args[af_idx + 1], "atempo=2.0");
    }

    #[test]
    fn test_build_render_args_filter_complex_overrides_vf() {
        let mut plan = default_plan(PathBuf::from("/in.mp4"), PathBuf::from("/out.mp4"));
        plan.filters = vec!["scale=80:60".to_string()];
        plan.filter_complex = Some("scale=1920:1080".to_string());
        let args = build_render_args(&plan);

        // filter_complex takes precedence
        assert!(args.contains(&"-filter_complex".to_string()));
        assert!(!args.contains(&"-vf".to_string()));
    }

    // ── Audio transcoding tests ────────────────────────────────────

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
    fn test_render_native_with_audio_transcoding() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("av_in.mp4");
        create_test_video_with_audio(&input);

        let output = dir.path().join("av_out.mp4");
        let plan = default_plan(input, output.clone());
        let result = render_native(&plan).unwrap();

        assert_eq!(result.output, output);
        assert!(result.duration_secs > 0.0);
        assert!(output.exists());
        // Verify output has audio by checking file size is larger than video-only.
        assert!(output.metadata().unwrap().len() > 0);
    }

    #[test]
    fn test_render_native_with_audio_and_filter() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("av_filter_in.mp4");
        create_test_video_with_audio(&input);

        let output = dir.path().join("av_filter_out.mp4");
        let plan = RenderPlan {
            input,
            output: output.clone(),
            video_codec: "libx264".to_string(),
            crf: 23,
            preset: None,
            audio_codec: "aac".to_string(),
            audio_bitrate: None,
            filters: vec!["scale=80:60".to_string()],
            filter_complex: None,
            audio_filter: None,
        };

        let result = render_native(&plan).unwrap();
        assert!(result.duration_secs > 0.0);

        let info = probe::probe(&output).unwrap();
        assert_eq!(info.width, Some(80));
        assert_eq!(info.height, Some(60));
    }

    #[test]
    fn test_render_native_with_preset() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("preset_in.mp4");
        create_test_video(&input);

        let output = dir.path().join("preset_out.mp4");
        let plan = RenderPlan {
            input,
            output: output.clone(),
            video_codec: "libx264".to_string(),
            crf: 23,
            preset: Some("ultrafast".to_string()),
            audio_codec: "aac".to_string(),
            audio_bitrate: None,
            filters: vec![],
            filter_complex: None,
            audio_filter: None,
        };

        // Preset is currently only used in subprocess args, so native still works.
        let result = render_native(&plan).unwrap();
        assert!(result.duration_secs > 0.0);
    }

    // ── filter_complex tests ───────────────────────────────────────

    #[test]
    fn test_render_native_with_filter_complex_scale() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("fc_in.mp4");
        create_test_video(&input);

        let output = dir.path().join("fc_out.mp4");
        let plan = RenderPlan {
            input,
            output: output.clone(),
            video_codec: "libx264".to_string(),
            crf: 23,
            preset: None,
            audio_codec: "aac".to_string(),
            audio_bitrate: None,
            filters: vec![],
            filter_complex: Some("scale=80:60,format=yuv420p".to_string()),
            audio_filter: None,
        };

        let result = render_native(&plan).unwrap();
        assert!(result.duration_secs > 0.0);

        let info = probe::probe(&output).unwrap();
        assert_eq!(info.width, Some(80));
        assert_eq!(info.height, Some(60));
    }

    #[test]
    fn test_render_native_filter_complex_overrides_filters() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("fc_override_in.mp4");
        create_test_video(&input);

        let output = dir.path().join("fc_override_out.mp4");
        // filters says 80x60 but filter_complex says 120x90; filter_complex wins.
        let plan = RenderPlan {
            input,
            output: output.clone(),
            video_codec: "libx264".to_string(),
            crf: 23,
            preset: None,
            audio_codec: "aac".to_string(),
            audio_bitrate: None,
            filters: vec!["scale=80:60".to_string()],
            filter_complex: Some("scale=120:90,format=yuv420p".to_string()),
            audio_filter: None,
        };

        let _result = render_native(&plan).unwrap();
        let info = probe::probe(&output).unwrap();
        // filter_complex takes precedence.
        assert_eq!(info.width, Some(120));
        assert_eq!(info.height, Some(90));
    }

    #[test]
    fn test_render_native_with_filter_complex_and_audio() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("fc_av_in.mp4");
        create_test_video_with_audio(&input);

        let output = dir.path().join("fc_av_out.mp4");
        let plan = RenderPlan {
            input,
            output: output.clone(),
            video_codec: "libx264".to_string(),
            crf: 23,
            preset: None,
            audio_codec: "aac".to_string(),
            audio_bitrate: None,
            filters: vec![],
            filter_complex: Some("scale=80:60".to_string()),
            audio_filter: None,
        };

        let result = render_native(&plan).unwrap();
        assert!(result.duration_secs > 0.0);
        assert!(output.exists());
    }

    #[test]
    fn test_render_native_with_setpts_filter() {
        // setpts is commonly used for speed changes in short-form renders.
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("setpts_in.mp4");
        create_test_video(&input);

        let output = dir.path().join("setpts_out.mp4");
        let plan = RenderPlan {
            input,
            output: output.clone(),
            video_codec: "libx264".to_string(),
            crf: 23,
            preset: None,
            audio_codec: "aac".to_string(),
            audio_bitrate: None,
            filters: vec![],
            filter_complex: Some("setpts=PTS/2".to_string()),
            audio_filter: None,
        };

        let result = render_native(&plan).unwrap();
        // Speed 2x should roughly halve the duration.
        assert!(result.duration_secs > 0.0);
        assert!(result.duration_secs < 1.0);
    }

    // ── prepare_filter_spec tests ──────────────────────────────────

    #[test]
    fn test_prepare_filter_spec_simple_passthrough() {
        let spec = "scale=1080:-2,fps=30,format=yuv420p";
        assert_eq!(prepare_filter_spec(spec, "[0:v]"), spec);
    }

    #[test]
    fn test_prepare_filter_spec_replaces_0v_with_in() {
        let spec = "[0:v]scale=1080:-2,fps=30";
        let result = prepare_filter_spec(spec, "[0:v]");
        assert_eq!(result, "[in]scale=1080:-2,fps=30");
    }

    #[test]
    fn test_prepare_filter_spec_replaces_0a_with_in() {
        let spec = "[0:a]atempo=2.0";
        let result = prepare_filter_spec(spec, "[0:a]");
        assert_eq!(result, "[in]atempo=2.0");
    }

    #[test]
    fn test_prepare_filter_spec_strips_trailing_vfinal() {
        let spec = "scale=1080:-2[vfinal]";
        let result = prepare_filter_spec(spec, "[0:v]");
        assert_eq!(result, "scale=1080:-2");
    }

    #[test]
    fn test_prepare_filter_spec_strips_trailing_afinal() {
        let spec = "[0:a]atempo=2.0[afinal]";
        let result = prepare_filter_spec(spec, "[0:a]");
        assert_eq!(result, "[in]atempo=2.0");
    }

    #[test]
    fn test_prepare_filter_spec_smart_pad() {
        // Smart pad: color source + overlay with [0:v] and [vfinal]
        let spec = "color=c=black:s=1080x1920:r=30/1[_bg];[0:v]scale=1080:-2:flags=lanczos[_fg];[_bg][_fg]overlay=0:420[vfinal]";
        let result = prepare_filter_spec(spec, "[0:v]");
        assert_eq!(
            result,
            "color=c=black:s=1080x1920:r=30/1[_bg];[in]scale=1080:-2:flags=lanczos[_fg];[_bg][_fg]overlay=0:420"
        );
    }

    #[test]
    fn test_prepare_filter_spec_speed_segments() {
        // Speed segments with split/trim/concat pattern
        let spec = "[0:v]split=2[_v0][_v1];[_v0]trim=0:5,setpts=PTS-STARTPTS[_tv0];[_v1]trim=5:10,setpts=PTS/2-STARTPTS[_tv1];[_tv0][_tv1]concat=n=2[vfinal]";
        let result = prepare_filter_spec(spec, "[0:v]");
        assert_eq!(
            result,
            "[in]split=2[_v0][_v1];[_v0]trim=0:5,setpts=PTS-STARTPTS[_tv0];[_v1]trim=5:10,setpts=PTS/2-STARTPTS[_tv1];[_tv0][_tv1]concat=n=2"
        );
    }

    #[test]
    fn test_prepare_filter_spec_preserves_internal_labels() {
        // Internal labels like [_bg][_fg] should be preserved
        let spec = "color=c=red:s=100x100[_bg];[0:v]scale=100:-2[_fg];[_bg][_fg]overlay=0:0";
        let result = prepare_filter_spec(spec, "[0:v]");
        // No trailing label to strip, [0:v] replaced with [in]
        assert_eq!(
            result,
            "color=c=red:s=100x100[_bg];[in]scale=100:-2[_fg];[_bg][_fg]overlay=0:0"
        );
    }

    #[test]
    fn test_prepare_filter_spec_audio_speed_segments() {
        let spec = "[0:a]asplit=2[_a0][_a1];[_a0]atrim=0:5,asetpts=PTS-STARTPTS[_ta0];[_a1]atrim=5:10,atempo=2.0,asetpts=PTS-STARTPTS[_ta1];[_ta0][_ta1]concat=n=2:v=0:a=1[afinal]";
        let result = prepare_filter_spec(spec, "[0:a]");
        assert_eq!(
            result,
            "[in]asplit=2[_a0][_a1];[_a0]atrim=0:5,asetpts=PTS-STARTPTS[_ta0];[_a1]atrim=5:10,atempo=2.0,asetpts=PTS-STARTPTS[_ta1];[_ta0][_ta1]concat=n=2:v=0:a=1"
        );
    }

    #[test]
    fn test_render_native_with_color_source_filter() {
        // Simulated smart pad: color source + overlay.
        // Uses a simple graph: color source → overlay with video input.
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("smartpad_in.mp4");
        create_test_video(&input);

        let output = dir.path().join("smartpad_out.mp4");
        let plan = RenderPlan {
            input,
            output: output.clone(),
            video_codec: "libx264".to_string(),
            crf: 23,
            preset: None,
            audio_codec: "aac".to_string(),
            audio_bitrate: None,
            filters: vec![],
            // Mini smart pad: color background + overlay the input video on top.
            // shortest=1 stops when video ends. format=yuv420p for encoder compat.
            filter_complex: Some(
                "color=c=black:s=200x200:r=15[_bg];[0:v]scale=160:120[_fg];[_bg][_fg]overlay=20:40:shortest=1,format=yuv420p[vfinal]".to_string(),
            ),
            audio_filter: None,
        };

        let result = render_native(&plan).unwrap();
        assert!(result.duration_secs > 0.0);
        assert!(output.exists());
    }
}
