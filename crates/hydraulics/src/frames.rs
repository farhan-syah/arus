//! Spatial 2D grid metadata, exact per-step envelopes, and packed little-endian `Float32` frames.
//!
//! Splits static terrain geometry (`GridMetadata`) from time-stepped hydraulic fields (`GridFrame`)
//! so web GPU/Canvas renderers can stream binary frames without duplicating static arrays or losing
//! peak depths across subsampled output intervals.

use arus_jobs::{CancellationToken, StepBudget};
use arus_model_api::{AdvanceSummary, ModelError, Result};
use hydr::model::CellState;
use serde::{Deserialize, Serialize};

use crate::surface::SurfaceEngine;

/// Binary magic header for `ARUSFRM1` little-endian `Float32` frame streams.
pub const FRAME_STREAM_MAGIC: &[u8; 8] = b"ARUSFRM1";

/// Depth cutoff below which velocity is zeroed in visualization frames to prevent `qx/h` spikes.
pub const VELOCITY_DRY_CUTOFF_M: f64 = 1e-4;

/// Static spatial metadata and bare-earth terrain for a 2D simulation domain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridMetadata {
    pub cols: usize,
    pub rows: usize,
    pub dx_m: f64,
    pub dy_m: f64,
    pub projected_crs: String,
    pub vertical_datum: String,
    pub origin_m: [f64; 2],
    /// `[min_lon, min_lat, max_lon, max_lat]` in WGS84 degrees for map placement.
    pub wgs84_bounds: [f64; 4],
    pub terrain_provenance: String,
    pub bed_m: Vec<f32>,
    pub active_mask: Vec<bool>,
}

/// Exact per-cell peak envelopes accumulated across every accepted CFL timestep.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvelopeSummary {
    pub threshold_m: f64,
    pub max_depth_m: Vec<f32>,
    pub max_speed_m_s: Vec<f32>,
    /// Peak DEFRA FD2320 flood hazard rating `h * (v + 0.5)` (debris factor `DF = 0`).
    pub max_hazard_m2_s: Vec<f32>,
    /// First crossing time above `threshold_m` in seconds, or `-1.0` if never crossed.
    pub onset_time_s: Vec<f32>,
}

/// One time-stepped 2D hydraulic snapshot (`h`, `vx`, `vy`) in `f32` for GPU/Canvas animation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridFrame {
    pub time_s: f32,
    pub depth_m: Vec<f32>,
    pub vx_m_s: Vec<f32>,
    pub vy_m_s: Vec<f32>,
}

/// Complete spatial output bundle for one 2D simulation run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameSeries {
    pub metadata: GridMetadata,
    pub envelope: EnvelopeSummary,
    pub frames: Vec<GridFrame>,
}

impl FrameSeries {
    /// Encodes all frames into a compact little-endian `ARUSFRM1` binary stream without `unsafe`.
    ///
    /// Layout:
    /// - `[0..8]`: `b"ARUSFRM1"`
    /// - `[8..12]`: `u32` cols
    /// - `[12..16]`: `u32` rows
    /// - `[16..20]`: `u32` frame_count
    /// - For each frame: `f32` time_s, `[f32; cols*rows]` depth_m, `[f32; cols*rows]` vx, `[f32; cols*rows]` vy.
    pub fn encode_frames_bin(&self) -> Result<Vec<u8>> {
        let cols = u32::try_from(self.metadata.cols)
            .map_err(|_| ModelError::InvalidInput("cols exceeds u32".to_owned()))?;
        let rows = u32::try_from(self.metadata.rows)
            .map_err(|_| ModelError::InvalidInput("rows exceeds u32".to_owned()))?;
        let count = u32::try_from(self.frames.len())
            .map_err(|_| ModelError::InvalidInput("frame count exceeds u32".to_owned()))?;
        let n = self.metadata.cols.saturating_mul(self.metadata.rows);
        let bytes_per_frame = 4 + n * 3 * 4;
        let mut out = Vec::with_capacity(20 + self.frames.len() * bytes_per_frame);
        out.extend_from_slice(FRAME_STREAM_MAGIC);
        out.extend_from_slice(&cols.to_le_bytes());
        out.extend_from_slice(&rows.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        for frame in &self.frames {
            if frame.depth_m.len() != n || frame.vx_m_s.len() != n || frame.vy_m_s.len() != n {
                return Err(ModelError::InvalidInput(
                    "frame array length does not match grid cell count".to_owned(),
                ));
            }
            out.extend_from_slice(&frame.time_s.to_le_bytes());
            for &v in &frame.depth_m {
                out.extend_from_slice(&v.to_le_bytes());
            }
            for &v in &frame.vx_m_s {
                out.extend_from_slice(&v.to_le_bytes());
            }
            for &v in &frame.vy_m_s {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        Ok(out)
    }
}

/// Configuration for sampling spatial frames and computing envelopes during `advance_with_frames`.
#[derive(Debug, Clone)]
pub struct FrameCaptureConfig {
    pub frame_interval_s: f64,
    pub max_frames: usize,
    pub threshold_m: f64,
    pub wgs84_bounds: [f64; 4],
    pub terrain_provenance: String,
}

impl SurfaceEngine {
    /// Advances toward `target_s`, accumulating exact per-step peak envelopes and sampling
    /// `Float32` 2D `(h, vx, vy)` frames at `config.frame_interval_s` (capped at `config.max_frames`).
    pub fn advance_with_frames(
        &mut self,
        target_s: f64,
        budget: StepBudget,
        token: &CancellationToken,
        config: FrameCaptureConfig,
    ) -> Result<(AdvanceSummary, FrameSeries)> {
        self.advance_with_frames_and_progress(target_s, budget, token, config, |_, _| {})
    }

    /// Advances `target_s` while recording exact peak envelopes, sampled frames, and invoking
    /// `on_progress(steps_taken, accepted_time_s)` at each accepted solver step.
    pub fn advance_with_frames_and_progress<F>(
        &mut self,
        target_s: f64,
        budget: StepBudget,
        token: &CancellationToken,
        config: FrameCaptureConfig,
        mut on_progress: F,
    ) -> Result<(AdvanceSummary, FrameSeries)>
    where
        F: FnMut(usize, f64),
    {
        if !config.frame_interval_s.is_finite() || config.frame_interval_s <= 0.0 {
            return Err(ModelError::InvalidInput(format!(
                "frame_interval_s={} must be finite positive",
                config.frame_interval_s
            )));
        }
        if config.max_frames < 2 {
            return Err(ModelError::InvalidInput(
                "max_frames must be at least 2".to_owned(),
            ));
        }

        let domain = self.domain().clone();
        let (cols, rows) = domain.grid().dimensions();
        let (dx_m, dy_m) = domain.grid().spacing();
        let n = cols * rows;
        let active_mask = domain.active().to_vec();
        let bed_m: Vec<f32> = domain.bed().iter().map(|&z| z as f32).collect();

        let metadata = GridMetadata {
            cols,
            rows,
            dx_m,
            dy_m,
            projected_crs: domain.reference().projected_crs().to_owned(),
            vertical_datum: domain.reference().vertical_datum().to_owned(),
            origin_m: domain.reference().origin(),
            wgs84_bounds: config.wgs84_bounds,
            terrain_provenance: config.terrain_provenance,
            bed_m,
            active_mask: active_mask.clone(),
        };

        let mut max_depth_m = vec![0.0_f32; n];
        let mut max_speed_m_s = vec![0.0_f32; n];
        let mut max_hazard_m2_s = vec![0.0_f32; n];
        let mut onset_time_s = vec![-1.0_f32; n];
        let mut frames = Vec::with_capacity(config.max_frames.min(128));

        // Capture initial t = 0 state.
        let initial_cells = self.current_cells()?;
        let initial_time = self.time_s();
        update_envelope(
            &initial_cells,
            &active_mask,
            initial_time,
            config.threshold_m,
            &mut max_depth_m,
            &mut max_speed_m_s,
            &mut max_hazard_m2_s,
            &mut onset_time_s,
        );
        frames.push(build_frame(initial_time, &initial_cells, &active_mask));

        let mut next_capture_s = initial_time + config.frame_interval_s;
        let mut last_cells: Option<(f64, Vec<CellState>)> = None;
        let mut step_count = 0_usize;

        let summary = self.advance_raw_steps(target_s, budget, token, |end_s, cells| {
            step_count += 1;
            on_progress(step_count, end_s);
            update_envelope(
                cells,
                &active_mask,
                end_s,
                config.threshold_m,
                &mut max_depth_m,
                &mut max_speed_m_s,
                &mut max_hazard_m2_s,
                &mut onset_time_s,
            );
            if end_s + 1e-9 >= next_capture_s && frames.len() + 1 < config.max_frames {
                frames.push(build_frame(end_s, cells, &active_mask));
                while next_capture_s <= end_s + 1e-9 {
                    next_capture_s += config.frame_interval_s;
                }
                last_cells = None;
            } else {
                last_cells = Some((end_s, cells.to_vec()));
            }
        })?;

        if let Some((end_s, cells)) = last_cells
            && frames.len() < config.max_frames
        {
            frames.push(build_frame(end_s, &cells, &active_mask));
        }

        let envelope = EnvelopeSummary {
            threshold_m: config.threshold_m,
            max_depth_m,
            max_speed_m_s,
            max_hazard_m2_s,
            onset_time_s,
        };
        Ok((
            summary,
            FrameSeries {
                metadata,
                envelope,
                frames,
            },
        ))
    }
}

fn build_frame(time_s: f64, cells: &[CellState], active: &[bool]) -> GridFrame {
    let n = cells.len();
    let mut depth_m = Vec::with_capacity(n);
    let mut vx_m_s = Vec::with_capacity(n);
    let mut vy_m_s = Vec::with_capacity(n);
    for (cell, &is_active) in cells.iter().zip(active.iter()) {
        if !is_active || !cell.h.is_finite() || cell.h <= 0.0 {
            depth_m.push(0.0);
            vx_m_s.push(0.0);
            vy_m_s.push(0.0);
        } else {
            depth_m.push(cell.h as f32);
            if cell.h >= VELOCITY_DRY_CUTOFF_M {
                let vx = cell.qx / cell.h;
                let vy = cell.qy / cell.h;
                vx_m_s.push(if vx.is_finite() { vx as f32 } else { 0.0 });
                vy_m_s.push(if vy.is_finite() { vy as f32 } else { 0.0 });
            } else {
                vx_m_s.push(0.0);
                vy_m_s.push(0.0);
            }
        }
    }
    GridFrame {
        time_s: time_s as f32,
        depth_m,
        vx_m_s,
        vy_m_s,
    }
}

#[allow(clippy::too_many_arguments)]
fn update_envelope(
    cells: &[CellState],
    active: &[bool],
    time_s: f64,
    threshold_m: f64,
    max_depth_m: &mut [f32],
    max_speed_m_s: &mut [f32],
    max_hazard_m2_s: &mut [f32],
    onset_time_s: &mut [f32],
) {
    for (idx, (cell, &is_active)) in cells.iter().zip(active.iter()).enumerate() {
        if !is_active || !cell.h.is_finite() || cell.h <= 0.0 {
            continue;
        }
        let h = cell.h;
        let h_f32 = h as f32;
        if h_f32 > max_depth_m[idx] {
            max_depth_m[idx] = h_f32;
        }
        if h >= threshold_m && onset_time_s[idx] < 0.0 {
            onset_time_s[idx] = time_s as f32;
        }
        let speed = if h >= VELOCITY_DRY_CUTOFF_M {
            let vx = cell.qx / h;
            let vy = cell.qy / h;
            let sp = vx.hypot(vy);
            if sp.is_finite() { sp } else { 0.0 }
        } else {
            0.0
        };
        let speed_f32 = speed as f32;
        if speed_f32 > max_speed_m_s[idx] {
            max_speed_m_s[idx] = speed_f32;
        }
        // DEFRA FD2320 flood hazard rating: HR = h * (v + 0.5) (debris factor DF = 0).
        let hazard_f32 = (h * (speed + 0.5)) as f32;
        if hazard_f32 > max_hazard_m2_s[idx] {
            max_hazard_m2_s[idx] = hazard_f32;
        }
    }
}
