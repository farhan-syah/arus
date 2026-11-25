//! Pure-Rust VP9 WebM -> H.264 MP4 transcoding powered by `vidr 0.1.1` (WhatsApp/Telegram/iOS/Android compatible).

use std::fs::OpenOptions;
use std::io::Cursor;
use std::path::Path;

use containr::TrackKind;
use containr::mp4::{self, MuxOptions, PacketIn, SampleEntrySpec, TrackSpec, VisualCodec};
use numr15::runtime::cpu::{CpuDevice, CpuRuntime};
use numr15::tensor::Tensor;
use vidr::decode::DecodedFrame;
use vidr::decode::rusty_vp9::RustyVp9Decoder;
use vidr::encode::openh264::Openh264Encoder;
use vidr::encode::{
    EncodeCodec, EncodeConfig, EncodeFrame, EncodedPacket, Encoder, PixelFormat, RateControl,
};
use vidr::frame::OutputPlanes;
use vidr::media::{
    Input, InputSource, MuxSink, OutputKind, VideoOutput, plan_copy, read_stream_facts,
};
use vidr::pipeline::{FrameTransform, PipelineError, PumpControl, run};
use vidr::time::{DurationHints, EditEntry, EditTimeline, Rational};

struct PassthroughTransform;

impl FrameTransform<CpuRuntime> for PassthroughTransform {
    fn apply(
        &mut self,
        frame: DecodedFrame<CpuRuntime>,
    ) -> Result<OutputPlanes<CpuRuntime>, PipelineError> {
        Ok(OutputPlanes::new(frame.y().clone(), frame.uv().clone()))
    }
}

/// Transcodes a VP9 `.webm` recording into an H.264 Constrained Baseline `.mp4` via `vidr`,
/// optionally trimming leading loading frames before `trim_start_s` using `vidr`'s native edit window.
pub fn transcode_webm_to_mp4(
    input_path: &Path,
    output_path: &Path,
    trim_start_s: f64,
) -> Result<u64, String> {
    let mut input = Input::open(input_path)
        .map_err(|e| format!("open {} failed: {e}", input_path.display()))?;
    let facts = read_stream_facts(&input).map_err(|e| format!("read_stream_facts failed: {e}"))?;
    let width = facts.geometry.coded.width();
    let height = facts.geometry.coded.height();
    let fps = Rational::new(25, 1).map_err(|e| e.to_string())?;

    let kind = OutputKind::from_path(output_path)
        .map_err(|e| format!("output format check failed: {e}"))?;
    let copy_plan = plan_copy(&mut input, kind).map_err(|e| format!("plan_copy failed: {e}"))?;

    let source = InputSource::new(input).map_err(|e| format!("InputSource failed: {e}"))?;
    let mut timing = source.timing();
    timing.hints = DurationHints {
        average_frame_rate: Some(fps),
        ..DurationHints::default()
    };
    if trim_start_s > 0.0 {
        let ticks_per_sec = timing.time_base.den() as f64 / timing.time_base.num().max(1) as f64;
        let skip_ticks = (trim_start_s * ticks_per_sec).round() as i64;
        if skip_ticks > 0 {
            let entry = EditEntry {
                segment_duration: 0,
                media_time: skip_ticks,
                rate: Rational::ONE,
            };
            timing.timeline = Some(
                EditTimeline::resolve(&[entry], timing.time_base, timing.time_base)
                    .map_err(|e| format!("edit timeline failed: {e}"))?,
            );
        }
    }

    let mut config = EncodeConfig::new(
        EncodeCodec::H264,
        width,
        height,
        PixelFormat::Nv12,
        fps,
        timing.time_base,
    );
    config.rate_control = RateControl::VbrQuality { cq: 24 };
    config.gop_length = 60;

    let device = CpuDevice::new();
    let decoder = RustyVp9Decoder::<CpuRuntime>::new(&device);
    let encoder = Openh264Encoder::<CpuRuntime>::new(config)
        .map_err(|e| format!("OpenH264 encoder init failed: {e}"))?;

    let out_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(output_path)
        .map_err(|e| format!("create {} failed: {e}", output_path.display()))?;
    let video_out = VideoOutput::new(
        EncodeCodec::H264,
        Vec::new(),
        width,
        height,
        timing.time_base,
    );
    let sink = MuxSink::new(out_file, copy_plan, video_out);

    let control = PumpControl::new();
    let report = run::<CpuRuntime, _, _, _, _, _>(
        source,
        decoder,
        PassthroughTransform,
        encoder,
        sink,
        timing,
        &control,
    )
    .map_err(|e| format!("vidr transcode pipeline failed: {e}"))?;

    Ok(report.frames_out)
}

/// Encodes a sequence of RGB/RGBA PNG frames directly into a faststart H.264 `.mp4` via `vidr` + `containr`.
pub fn encode_png_frames_to_mp4(
    frames_png: &[Vec<u8>],
    fps_num: u32,
    output_path: &Path,
) -> Result<u64, String> {
    if frames_png.is_empty() {
        return Err("no PNG frames captured to encode".to_owned());
    }
    let (raw_w, raw_h, _) = decode_png_rgba(&frames_png[0])?;
    let width = raw_w & !1;
    let height = raw_h & !1;
    if width == 0 || height == 0 {
        return Err(format!("invalid frame dimensions {raw_w}x{raw_h}"));
    }

    let fps_val = i64::from(fps_num.max(1));
    let timescale = 90_000_u32;
    let frame_dur_ticks = i64::from(timescale) / fps_val;
    let fps = Rational::new(fps_val, 1).map_err(|e| e.to_string())?;
    let time_base = Rational::new(1, u64::from(timescale)).map_err(|e| e.to_string())?;

    let mut config = EncodeConfig::new(
        EncodeCodec::H264,
        width,
        height,
        PixelFormat::Nv12,
        fps,
        time_base,
    );
    config.rate_control = RateControl::VbrQuality { cq: 22 };
    config.gop_length = fps_num.max(1) * 2;

    let device = CpuDevice::new();
    let mut encoder = Openh264Encoder::<CpuRuntime>::new(config)
        .map_err(|e| format!("OpenH264 encoder init failed: {e}"))?;

    let mut packets: Vec<EncodedPacket> = Vec::with_capacity(frames_png.len());
    for (idx, png_bytes) in frames_png.iter().enumerate() {
        let (fw, fh, rgba) = decode_png_rgba(png_bytes)?;
        if (fw & !1) != width || (fh & !1) != height {
            return Err(format!(
                "frame {idx} size {fw}x{fh} mismatches expected {width}x{height}"
            ));
        }
        let (y_bytes, uv_bytes) = rgba_to_nv12(&rgba, fw as usize, width as usize, height as usize);
        let y_tensor =
            Tensor::<CpuRuntime>::from_slice(&y_bytes, &[height as usize, width as usize], &device)
                .map_err(|e| format!("y tensor init failed: {e}"))?;
        let uv_tensor = Tensor::<CpuRuntime>::from_slice(
            &uv_bytes,
            &[(height as usize) / 2, width as usize],
            &device,
        )
        .map_err(|e| format!("uv tensor init failed: {e}"))?;
        let encode_frame = EncodeFrame::new(&y_tensor, &uv_tensor);
        let pts = (idx as i64) * frame_dur_ticks;
        encoder
            .send(&encode_frame, pts, frame_dur_ticks)
            .map_err(|e| format!("OpenH264 send frame {idx} failed: {e}"))?;
        while let Some(pkt) = encoder.receive() {
            packets.push(pkt);
        }
    }
    encoder
        .flush()
        .map_err(|e| format!("OpenH264 flush failed: {e}"))?;
    while let Some(pkt) = encoder.receive() {
        packets.push(pkt);
    }

    let config_record = encoder
        .config_record()
        .ok_or_else(|| "OpenH264 produced no avcC config record".to_owned())?
        .to_vec();

    let out_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(output_path)
        .map_err(|e| format!("create {} failed: {e}", output_path.display()))?;

    let options = MuxOptions {
        movie_timescale: timescale,
        faststart: true,
        ..MuxOptions::default()
    };
    let mut muxer =
        mp4::Muxer::new(out_file, options).map_err(|e| format!("mp4 muxer init failed: {e}"))?;

    let w16 = u16::try_from(width).map_err(|_| "width overflow".to_owned())?;
    let h16 = u16::try_from(height).map_err(|_| "height overflow".to_owned())?;
    let mut spec = TrackSpec::new(
        TrackKind::Video,
        timescale,
        SampleEntrySpec::Visual {
            codec: VisualCodec::Avc,
            config: config_record,
            width: w16,
            height: h16,
        },
    );
    spec.display_width = u32::from(w16) << 16;
    spec.display_height = u32::from(h16) << 16;
    let track_id = muxer
        .add_track(spec)
        .map_err(|e| format!("mp4 add_track failed: {e}"))?;

    let pkt_count = packets.len();
    for (i, pkt) in packets.iter().enumerate() {
        let is_last = i + 1 == pkt_count;
        let dur = if is_last {
            Some(frame_dur_ticks.max(1) as u64)
        } else {
            None
        };
        muxer
            .write_packet(
                track_id,
                &PacketIn {
                    data: &pkt.data,
                    pts: pkt.pts,
                    dts: pkt.dts,
                    duration: dur,
                    sync: pkt.keyframe,
                },
            )
            .map_err(|e| format!("mp4 write_packet {i} failed: {e}"))?;
    }
    muxer
        .finish()
        .map_err(|e| format!("mp4 finish failed: {e}"))?;

    Ok(pkt_count as u64)
}

fn decode_png_rgba(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("png header decode failed: {e}"))?;
    let buf_len = reader
        .output_buffer_size()
        .ok_or_else(|| "png output buffer size overflow".to_owned())?;
    let mut buf = vec![0u8; buf_len];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("png frame decode failed: {e}"))?;
    let raw = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => raw.to_vec(),
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity((info.width * info.height * 4) as usize);
            for px in raw.as_chunks::<3>().0 {
                out.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            out
        }
        other => return Err(format!("unsupported png color type {other:?}")),
    };
    Ok((info.width, info.height, rgba))
}

fn rgba_to_nv12(rgba: &[u8], stride_px: usize, w: usize, h: usize) -> (Vec<u8>, Vec<u8>) {
    let mut y_plane = vec![0u8; w * h];
    let mut uv_plane = vec![128u8; (h / 2) * w];

    for y in 0..h {
        let row_off = y * stride_px * 4;
        let y_row = y * w;
        for x in 0..w {
            let p = row_off + x * 4;
            let r = i32::from(rgba[p]);
            let g = i32::from(rgba[p + 1]);
            let b = i32::from(rgba[p + 2]);
            let luma = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
            y_plane[y_row + x] = luma.clamp(16, 235) as u8;
        }
    }

    for y in (0..h).step_by(2) {
        let uv_row = (y / 2) * w;
        for x in (0..w).step_by(2) {
            let mut r_sum = 0i32;
            let mut g_sum = 0i32;
            let mut b_sum = 0i32;
            for dy in 0..2 {
                let row_off = (y + dy) * stride_px * 4;
                for dx in 0..2 {
                    let p = row_off + (x + dx) * 4;
                    r_sum += i32::from(rgba[p]);
                    g_sum += i32::from(rgba[p + 1]);
                    b_sum += i32::from(rgba[p + 2]);
                }
            }
            let r = r_sum >> 2;
            let g = g_sum >> 2;
            let b = b_sum >> 2;
            let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
            let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
            uv_plane[uv_row + x] = u.clamp(16, 240) as u8;
            uv_plane[uv_row + x + 1] = v.clamp(16, 240) as u8;
        }
    }

    (y_plane, uv_plane)
}
