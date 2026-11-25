//! Pure-Rust VP9 WebM -> H.264 MP4 transcoding powered by `vidr 0.1.1` (WhatsApp/Telegram/iOS/Android compatible).

use std::fs::OpenOptions;
use std::path::Path;

use numr15::runtime::cpu::{CpuDevice, CpuRuntime};
use vidr::decode::DecodedFrame;
use vidr::decode::rusty_vp9::RustyVp9Decoder;
use vidr::encode::openh264::Openh264Encoder;
use vidr::encode::{EncodeCodec, EncodeConfig, PixelFormat, RateControl};
use vidr::frame::OutputPlanes;
use vidr::media::{
    Input, InputSource, MuxSink, OutputKind, VideoOutput, plan_copy, read_stream_facts,
};
use vidr::pipeline::{FrameTransform, PipelineError, PumpControl, run};
use vidr::time::{DurationHints, Rational};

struct PassthroughTransform;

impl FrameTransform<CpuRuntime> for PassthroughTransform {
    fn apply(
        &mut self,
        frame: DecodedFrame<CpuRuntime>,
    ) -> Result<OutputPlanes<CpuRuntime>, PipelineError> {
        Ok(OutputPlanes::new(frame.y().clone(), frame.uv().clone()))
    }
}

/// Transcodes a VP9 `.webm` recording into an H.264 Constrained Baseline `.mp4` via `vidr`.
pub fn transcode_webm_to_mp4(input_path: &Path, output_path: &Path) -> Result<u64, String> {
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
