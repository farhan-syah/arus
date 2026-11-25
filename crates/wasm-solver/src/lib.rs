//! Browser WebGPU (`SurfaceSolverF32<WgpuRuntime>`) and WASM CPU (`SurfaceSolverF32<CpuRuntime>`)
//! 2D shallow-water solver built directly on `hydr 0.3.0` + `numr 0.17.0`.
//!
//! Emits the exact `ARUSFRM1` binary stream and `RunGridResponse` JSON expected by ARUS Studio.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod dem;

use hydr::grid::CartesianGrid;
use hydr::model::{CellState, Domain, FlowState, SpatialReference};
use hydr::solver::{
    SurfaceF32Boundaries, SurfaceF32CoastalForcing, SurfaceF32OpenMasks, SurfaceF32SideBoundary,
    SurfaceF32Soil, SurfaceF32SpatialOrder, SurfaceSolverF32, SurfaceSolverF32Config,
    SurfaceSolverF32Ledger,
};
use js_sys::{Object, Reflect, Uint8Array};
use numr::runtime::Runtime;
use numr::runtime::cpu::{CpuDevice, CpuRuntime};
use numr::runtime::wgpu::{WgpuClient, WgpuDevice, WgpuRuntime};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use crate::dem::{
    bilinear_sample_grid, build_channel_prestate, build_synthetic_valley_cells,
    burn_river_channels, compute_d8_flow_accum, compute_wgs84_bounds, condition_hydrological_dem,
};

const FRAME_STREAM_MAGIC: &[u8; 8] = b"ARUSFRM1";
const VELOCITY_DRY_CUTOFF_M: f64 = 1e-4;
const MAX_BROWSER_CELLS: usize = 16_384;
const MAX_BROWSER_STEPS: usize = 20_000;
const MAX_BROWSER_FRAMES: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
enum HyetographPreset {
    Uniform,
    #[default]
    PeakStorm,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum BoundaryChoice {
    Closed,
    #[default]
    Transmissive,
    OpenStage {
        elevation_m: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum LandChoice {
    #[default]
    Impervious,
    ModifiedGreenAmpt {
        suction_m: f64,
        conductivity_m_per_s: f64,
        maximum_deficit: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
enum TideMode {
    #[default]
    Live,
    SpringHigh,
    Off,
}

const fn default_true() -> bool {
    true
}
const fn default_lon() -> f64 {
    101.535
}
const fn default_lat() -> f64 {
    3.031
}
const fn default_cols() -> usize {
    64
}
const fn default_rows() -> usize {
    56
}
const fn default_dx_m() -> f64 {
    30.0
}
const fn default_rainfall() -> f64 {
    280.0
}
const fn default_duration_s() -> f64 {
    360.0
}
const fn default_manning_n() -> f64 {
    0.035
}
const fn default_threshold_m() -> f64 {
    0.15
}
const fn default_gauge_radius() -> usize {
    3
}

#[derive(Debug, Clone, Deserialize)]
struct InterventionCellDelta {
    col: usize,
    row: usize,
    delta_m: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ObservedGaugeSeed {
    pub(crate) lat: f64,
    pub(crate) lon: f64,
    pub(crate) depth_m: f64,
    #[serde(default = "default_gauge_radius")]
    pub(crate) radius_cells: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct SpatialRainGrid {
    nx: usize,
    ny: usize,
    hourly_mm: Vec<Vec<f64>>,
    #[serde(default)]
    et0_mm: Vec<Vec<f64>>,
}

#[derive(Debug, Clone, Deserialize)]
struct WasmSimRequest {
    #[serde(default = "default_lon")]
    center_lon: f64,
    #[serde(default = "default_lat")]
    center_lat: f64,
    #[serde(default = "default_cols")]
    cols: usize,
    #[serde(default = "default_rows")]
    rows: usize,
    #[serde(default = "default_dx_m")]
    dx_m: f64,
    #[serde(default = "default_rainfall")]
    rainfall_mm_per_hr: f64,
    #[serde(default)]
    hyetograph: HyetographPreset,
    #[serde(default = "default_duration_s")]
    duration_s: f64,
    #[serde(default = "default_manning_n")]
    manning_n: f64,
    #[serde(default)]
    land: LandChoice,
    #[serde(default)]
    boundary: BoundaryChoice,
    #[serde(default = "default_threshold_m")]
    threshold_m: f64,
    #[serde(default)]
    interventions: Vec<InterventionCellDelta>,
    #[serde(default)]
    terrain_elevations_m: Option<Vec<Option<f64>>>,
    #[serde(default)]
    terrain_provenance: Option<String>,
    #[serde(default)]
    antecedent_rain_mm: f64,
    #[serde(default)]
    hourly_rain_mm: Vec<f64>,
    #[serde(default)]
    spatial_rain_grid: Option<SpatialRainGrid>,
    #[serde(default)]
    hourly_tide_m: Vec<f64>,
    #[serde(default)]
    drainage_mm_per_hr: f64,
    #[serde(default)]
    evaporation_mm_per_day: f64,
    #[serde(default)]
    observed_gauges: Vec<ObservedGaugeSeed>,
    #[serde(default)]
    wind_x_m_per_s: f64,
    #[serde(default)]
    wind_y_m_per_s: f64,
    #[serde(default)]
    start_dry: bool,
    #[serde(default = "default_true")]
    enable_evaporation: bool,
    #[serde(default = "default_true")]
    enable_drainage: bool,
    #[serde(default)]
    tide_mode: TideMode,
}

#[derive(Debug, Serialize)]
struct WaterBudgetJson {
    initial_storage_m3: f64,
    final_storage_m3: f64,
    rain_m3: f64,
    evaporation_m3: f64,
    withdrawal_m3: f64,
    infiltration_m3: f64,
    boundary_inflow_m3: f64,
    boundary_outflow_m3: f64,
    residual_m3: f64,
    relative_error: f64,
}

#[derive(Debug, Serialize)]
struct BudgetAssessmentJson {
    passed: bool,
    residual_m3: f64,
    relative_error: f64,
    max_abs_tolerance_m3: f64,
    max_relative_tolerance: f64,
    note: String,
}

#[derive(Debug, Serialize)]
struct EarthworkSummaryJson {
    modified_cells: usize,
    cut_volume_m3: f64,
    fill_volume_m3: f64,
    net_volume_m3: f64,
}

#[derive(Debug, Serialize)]
struct GridMetadataJson {
    cols: usize,
    rows: usize,
    dx_m: f64,
    dy_m: f64,
    projected_crs: String,
    vertical_datum: String,
    origin_m: [f64; 2],
    wgs84_bounds: [f64; 4],
    terrain_provenance: String,
    bed_m: Vec<f32>,
    active_mask: Vec<bool>,
}

#[derive(Debug, Serialize)]
struct EnvelopeSummaryJson {
    threshold_m: f64,
    max_depth_m: Vec<f32>,
    max_speed_m_s: Vec<f32>,
    max_hazard_m2_s: Vec<f32>,
    onset_time_s: Vec<f32>,
}

#[derive(Debug, Serialize)]
struct RunGridResponseJson {
    run_id: String,
    classification: &'static str,
    accepted_time_s: f64,
    steps_taken: usize,
    frame_count: usize,
    water_budget: WaterBudgetJson,
    budget_assessment: BudgetAssessmentJson,
    earthwork: EarthworkSummaryJson,
    metadata: GridMetadataJson,
    envelope: EnvelopeSummaryJson,
}

struct PreparedProblem {
    domain: Domain,
    initial: FlowState,
    boundaries: SurfaceF32Boundaries,
    open_masks: SurfaceF32OpenMasks,
    soil: Option<SurfaceF32Soil>,
    coastal: SurfaceF32CoastalForcing,
    intervals: Vec<ForcingInterval>,
    metadata: GridMetadataJson,
    earthwork: EarthworkSummaryJson,
    threshold_m: f64,
    duration_s: f64,
    cols: usize,
    rows: usize,
}

struct ForcingInterval {
    end_s: f64,
    rain: Vec<f32>,
    evap: Vec<f32>,
    withdrawal: Vec<f32>,
}

fn prepare_problem(req: &WasmSimRequest) -> Result<PreparedProblem, String> {
    let cols = req.cols;
    let rows = req.rows;
    let n = cols.saturating_mul(rows);
    if cols < 4 || rows < 4 || n > MAX_BROWSER_CELLS {
        return Err(format!(
            "grid {cols}x{rows} must be at least 4x4 and <= {MAX_BROWSER_CELLS} cells"
        ));
    }
    if !req.dx_m.is_finite() || !(1.0..=2500.0).contains(&req.dx_m) {
        return Err("dx_m must be finite in [1, 2500]".to_owned());
    }
    if !req.duration_s.is_finite() || !(10.0..=3600.0).contains(&req.duration_s) {
        return Err("duration_s must be finite in [10, 3600]".to_owned());
    }

    let bounds = compute_wgs84_bounds(req.center_lon, req.center_lat, cols, rows, req.dx_m);
    let (mut cells, terrain_provenance, vertical_datum) = match &req.terrain_elevations_m {
        Some(sampled) if sampled.len() == n => {
            let prov = req
                .terrain_provenance
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "BrowserTerrariumScreening".to_owned());
            let mut conditioned = sampled.clone();
            if prov == "BrowserTerrariumScreening" {
                condition_hydrological_dem(&mut conditioned, cols, rows);
                burn_river_channels(&mut conditioned, cols, rows);
            }
            (conditioned, prov, "egm96-terrarium-screening".to_owned())
        }
        _ => (
            build_synthetic_valley_cells(cols, rows),
            "SyntheticDemoBasin".to_owned(),
            "local-synthetic-datum".to_owned(),
        ),
    };

    let cell_area = req.dx_m * req.dx_m;
    let mut modified_cells = 0usize;
    let mut cut_volume_m3 = 0.0f64;
    let mut fill_volume_m3 = 0.0f64;
    for iv in &req.interventions {
        if iv.col < cols
            && iv.row < rows
            && iv.delta_m.is_finite()
            && let Some(z) = cells[iv.row * cols + iv.col].as_mut()
        {
            *z += iv.delta_m;
            modified_cells += 1;
            if iv.delta_m >= 0.0 {
                fill_volume_m3 += iv.delta_m * cell_area;
            } else {
                cut_volume_m3 += (-iv.delta_m) * cell_area;
            }
        }
    }
    let earthwork = EarthworkSummaryJson {
        modified_cells,
        cut_volume_m3,
        fill_volume_m3,
        net_volume_m3: fill_volume_m3 - cut_volume_m3,
    };

    let mut bed = Vec::with_capacity(n);
    let mut active = Vec::with_capacity(n);
    let mut bed_f32 = Vec::with_capacity(n);
    for z_opt in &cells {
        match *z_opt {
            Some(z) if z.is_finite() => {
                bed.push(z);
                bed_f32.push(z as f32);
                active.push(true);
            }
            _ => {
                bed.push(0.0);
                bed_f32.push(0.0);
                active.push(false);
            }
        }
    }

    // Per-face open masks on low-lying 45th-percentile perimeter cells (matching server F64).
    let (boundaries, open_masks) = match req.boundary {
        BoundaryChoice::Closed => (
            SurfaceF32Boundaries::impermeable_walls(),
            SurfaceF32OpenMasks::default(),
        ),
        BoundaryChoice::Transmissive | BoundaryChoice::OpenStage { .. } => {
            let mut perim_z: Vec<f64> = Vec::new();
            for r in 0..rows {
                for c in 0..cols {
                    if (r == 0 || r + 1 == rows || c == 0 || c + 1 == cols)
                        && let Some(z) = cells[r * cols + c]
                    {
                        perim_z.push(z);
                    }
                }
            }
            perim_z.sort_by(f64::total_cmp);
            let outlet_max_z = if perim_z.is_empty() {
                f64::INFINITY
            } else {
                perim_z[(perim_z.len() * 45) / 100]
            };
            let west: Vec<bool> = (0..rows)
                .map(|r| cells[r * cols].is_some_and(|z| z <= outlet_max_z))
                .collect();
            let east: Vec<bool> = (0..rows)
                .map(|r| cells[r * cols + (cols - 1)].is_some_and(|z| z <= outlet_max_z))
                .collect();
            let south: Vec<bool> = (0..cols)
                .map(|c| cells[c].is_some_and(|z| z <= outlet_max_z))
                .collect();
            let north: Vec<bool> = (0..cols)
                .map(|c| cells[(rows - 1) * cols + c].is_some_and(|z| z <= outlet_max_z))
                .collect();
            let side = match req.boundary {
                BoundaryChoice::OpenStage { elevation_m } => {
                    SurfaceF32SideBoundary::Stage(elevation_m as f32)
                }
                _ => SurfaceF32SideBoundary::Transmissive,
            };
            (
                SurfaceF32Boundaries {
                    west: side,
                    east: side,
                    south: side,
                    north: side,
                },
                SurfaceF32OpenMasks {
                    west: Some(west),
                    east: Some(east),
                    south: Some(south),
                    north: Some(north),
                },
            )
        }
    };

    let accum = compute_d8_flow_accum(&cells, cols, rows);
    let mut valid_z: Vec<f64> = cells.iter().filter_map(|&z| z).collect();
    valid_z.sort_by(f64::total_cmp);
    let low_z_cutoff = if valid_z.is_empty() {
        10.0
    } else {
        valid_z[(valid_z.len() * 30) / 100]
    };
    let channel_accum_min = ((n as f64).sqrt() * 0.28).max(5.0) as u32;

    let has_spatial = req.spatial_rain_grid.as_ref().is_some_and(|g| {
        g.nx >= 2 && g.ny >= 2 && g.hourly_mm.len() == g.nx * g.ny && !g.hourly_mm[0].is_empty()
    });
    let has_nwp = has_spatial || !req.hourly_rain_mm.is_empty();

    let manning: Vec<f64> = if has_nwp {
        cells
            .iter()
            .enumerate()
            .map(|(idx, z_opt)| match *z_opt {
                Some(_) if accum[idx] >= channel_accum_min => 0.032,
                Some(z) if z <= low_z_cutoff => 0.060,
                Some(_) => 0.110,
                None => req.manning_n,
            })
            .collect()
    } else {
        vec![req.manning_n; n]
    };

    let grid = CartesianGrid::new(cols, rows, req.dx_m, req.dx_m).map_err(|e| e.to_string())?;
    let projected_crs = format!(
        "local-metric-tangent@{:.4},{:.4}",
        req.center_lat, req.center_lon
    );
    let reference =
        SpatialReference::new(projected_crs.clone(), vertical_datum.clone(), [0.0, 0.0])
            .map_err(|e| e.to_string())?;
    let domain =
        Domain::new(grid, bed, manning, active.clone(), reference).map_err(|e| e.to_string())?;

    let has_prestate =
        !req.start_dry && (req.antecedent_rain_mm > 0.0 || !req.observed_gauges.is_empty());
    let init_depths = if has_prestate {
        build_channel_prestate(
            &cells,
            cols,
            rows,
            bounds,
            req.antecedent_rain_mm,
            &req.observed_gauges,
        )
    } else {
        vec![0.0; n]
    };
    let init_cells: Vec<CellState> = init_depths
        .into_iter()
        .zip(active.iter())
        .map(|(h, &a)| CellState {
            h: if a { h } else { 0.0 },
            qx: 0.0,
            qy: 0.0,
        })
        .collect();
    let initial = FlowState::new(&domain, init_cells).map_err(|e| e.to_string())?;

    let soil = match req.land {
        LandChoice::Impervious => None,
        LandChoice::ModifiedGreenAmpt {
            suction_m,
            conductivity_m_per_s,
            maximum_deficit,
        } => Some(SurfaceF32Soil::uniform(
            n,
            conductivity_m_per_s as f32,
            suction_m as f32,
            maximum_deficit as f32,
        )),
    };

    let coastal = SurfaceF32CoastalForcing {
        wind_x_m_per_s: req.wind_x_m_per_s as f32,
        wind_y_m_per_s: req.wind_y_m_per_s as f32,
        ..SurfaceF32CoastalForcing::default()
    };

    let base_drain_m_s = if req.enable_drainage {
        req.drainage_mm_per_hr.max(0.0) / 3_600_000.0
    } else {
        0.0
    };
    let base_evap_m_s = if req.enable_evaporation {
        req.evaporation_mm_per_day.max(0.0) / 86_400_000.0
    } else {
        0.0
    };

    let mut intervals = Vec::new();
    if has_nwp {
        let n_hours = if let Some(sg) = &req.spatial_rain_grid
            && has_spatial
        {
            sg.hourly_mm[0].len()
        } else {
            req.hourly_rain_mm.len()
        };
        let n_bins = n_hours.clamp(1, 24);
        let bin_dur_s = req.duration_s / (n_bins as f64);
        let time_compression = ((n_hours as f64) * 3600.0 / req.duration_s).clamp(1.0, 240.0);

        for b in 0..n_bins {
            let h0 = (b * n_hours) / n_bins;
            let h1 = (((b + 1) * n_hours) / n_bins).max(h0 + 1).min(n_hours);
            let bin_len = (h1 - h0).max(1) as f64;
            let bin_pt_means: Vec<f64> = if let Some(sg) = &req.spatial_rain_grid
                && has_spatial
            {
                sg.hourly_mm
                    .iter()
                    .map(|series| {
                        let end = h1.min(series.len());
                        let start = h0.min(end);
                        series[start..end]
                            .iter()
                            .copied()
                            .filter(|v| v.is_finite() && *v > 0.0)
                            .sum::<f64>()
                            / (end.saturating_sub(start).max(1) as f64)
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let fallback_mean_mm = if !has_spatial {
                req.hourly_rain_mm[h0..h1]
                    .iter()
                    .copied()
                    .filter(|v| v.is_finite() && *v > 0.0)
                    .sum::<f64>()
                    / bin_len
            } else {
                bin_pt_means.iter().sum::<f64>() / (bin_pt_means.len().max(1) as f64)
            };

            let bin_tide_m = match req.tide_mode {
                TideMode::Off => 0.0,
                TideMode::SpringHigh => 2.2,
                TideMode::Live => {
                    if !req.hourly_tide_m.is_empty() {
                        let t_end = h1.min(req.hourly_tide_m.len());
                        let t_start = h0.min(t_end);
                        req.hourly_tide_m[t_start..t_end]
                            .iter()
                            .copied()
                            .fold(f64::NEG_INFINITY, f64::max)
                            .max(0.0)
                    } else {
                        0.0
                    }
                }
            };
            let tide_lock = if bin_tide_m > 1.2 {
                ((bin_tide_m - 1.2) / 1.0).clamp(0.0, 1.0)
            } else {
                0.0
            };

            let has_spatial_et0 = req.enable_evaporation
                && req.spatial_rain_grid.as_ref().is_some_and(|g| {
                    has_spatial && g.et0_mm.len() == g.nx * g.ny && !g.et0_mm[0].is_empty()
                });
            let bin_et0_means: Vec<f64> = if let Some(sg) = &req.spatial_rain_grid
                && has_spatial_et0
            {
                sg.et0_mm
                    .iter()
                    .map(|series| {
                        let end = h1.min(series.len());
                        let start = h0.min(end);
                        series[start..end]
                            .iter()
                            .copied()
                            .filter(|v| v.is_finite() && *v > 0.0)
                            .sum::<f64>()
                            / (end.saturating_sub(start).max(1) as f64)
                    })
                    .collect()
            } else {
                Vec::new()
            };

            let mut rain = vec![0.0f32; n];
            let mut evap = vec![0.0f32; n];
            let mut withdrawal = vec![0.0f32; n];
            for r in 0..rows {
                for c in 0..cols {
                    let idx = r * cols + c;
                    let Some(z) = cells[idx] else { continue };
                    let u = (c as f64 + 0.5) / (cols as f64);
                    let v = (r as f64 + 0.5) / (rows as f64);
                    let local_mm = if let Some(sg) = &req.spatial_rain_grid
                        && has_spatial
                    {
                        bilinear_sample_grid(&bin_pt_means, sg.nx, sg.ny, u, v)
                    } else {
                        fallback_mean_mm
                    };
                    let a = accum[idx];
                    let routing_mult = if a >= channel_accum_min {
                        1.0 + ((a as f64) / (channel_accum_min as f64)).ln_1p().min(1.6) * 0.55
                    } else {
                        0.85
                    };
                    let surge = if z <= 3.5 && tide_lock > 0.0 {
                        (0.45 * tide_lock / 3_600_000.0) * time_compression
                    } else {
                        0.0
                    };
                    rain[idx] =
                        ((local_mm / 3_600_000.0) * time_compression * routing_mult + surge) as f32;
                    let local_evap = if let Some(sg) = &req.spatial_rain_grid
                        && has_spatial_et0
                    {
                        (bilinear_sample_grid(&bin_et0_means, sg.nx, sg.ny, u, v) / 3_600_000.0)
                            * time_compression
                    } else {
                        base_evap_m_s * time_compression
                    };
                    evap[idx] = if local_mm > 0.2 {
                        0.0
                    } else {
                        local_evap as f32
                    };
                    withdrawal[idx] = (base_drain_m_s * time_compression) as f32;
                }
            }
            intervals.push(ForcingInterval {
                end_s: ((b + 1) as f64) * bin_dur_s,
                rain,
                evap,
                withdrawal,
            });
        }
    } else {
        let rain_rate = (req.rainfall_mm_per_hr / 3_600_000.0) as f32;
        let evap_rate = base_evap_m_s as f32;
        let drain_rate = base_drain_m_s as f32;
        match req.hyetograph {
            HyetographPreset::Uniform => {
                intervals.push(ForcingInterval {
                    end_s: req.duration_s,
                    rain: vec![rain_rate; n],
                    evap: vec![evap_rate; n],
                    withdrawal: vec![drain_rate; n],
                });
            }
            HyetographPreset::PeakStorm => {
                let mults = [0.4f32, 1.35, 1.9, 0.9, 0.3, 0.0];
                let seg_s = req.duration_s / (mults.len() as f64);
                for (i, &m) in mults.iter().enumerate() {
                    intervals.push(ForcingInterval {
                        end_s: ((i + 1) as f64) * seg_s,
                        rain: vec![rain_rate * m; n],
                        evap: vec![evap_rate; n],
                        withdrawal: vec![drain_rate; n],
                    });
                }
            }
        }
    }

    let metadata = GridMetadataJson {
        cols,
        rows,
        dx_m: req.dx_m,
        dy_m: req.dx_m,
        projected_crs,
        vertical_datum,
        origin_m: [0.0, 0.0],
        wgs84_bounds: bounds,
        terrain_provenance,
        bed_m: bed_f32,
        active_mask: active,
    };

    Ok(PreparedProblem {
        domain,
        initial,
        boundaries,
        open_masks,
        soil,
        coastal,
        intervals,
        metadata,
        earthwork,
        threshold_m: req.threshold_m,
        duration_s: req.duration_s,
        cols,
        rows,
    })
}

fn attach_options<R: Runtime<DType = numr::dtype::DType>>(
    mut solver: SurfaceSolverF32<R>,
    prob: &PreparedProblem,
) -> Result<SurfaceSolverF32<R>, String>
where
    R::Client: numr::ops::BinaryOps<R>
        + numr::ops::CompareOps<R>
        + numr::ops::ConditionalOps<R>
        + numr::ops::ReduceOps<R>
        + numr::ops::ScalarOps<R>
        + numr::ops::ShapeOps<R>
        + numr::ops::UnaryOps<R>,
{
    solver = solver
        .with_boundaries(prob.boundaries)
        .map_err(|e| e.to_string())?;
    solver = solver
        .with_open_masks(&prob.open_masks)
        .map_err(|e| e.to_string())?;
    if let Some(soil) = &prob.soil {
        solver = solver.with_soil(soil).map_err(|e| e.to_string())?;
    }
    if prob.coastal.is_active() {
        solver = solver
            .with_coastal_forcing(prob.coastal)
            .map_err(|e| e.to_string())?;
    }
    Ok(solver)
}

struct FrameCollector {
    cols: usize,
    rows: usize,
    active: Vec<bool>,
    threshold_m: f64,
    max_depth_m: Vec<f32>,
    max_speed_m_s: Vec<f32>,
    max_hazard_m2_s: Vec<f32>,
    onset_time_s: Vec<f32>,
    frame_count: usize,
    frames_bytes: Vec<u8>,
}

impl FrameCollector {
    fn new(cols: usize, rows: usize, active: Vec<bool>, threshold_m: f64) -> Self {
        let n = cols * rows;
        let mut frames_bytes = Vec::with_capacity(20 + MAX_BROWSER_FRAMES * (4 + n * 12));
        frames_bytes.extend_from_slice(FRAME_STREAM_MAGIC);
        frames_bytes.extend_from_slice(&(cols as u32).to_le_bytes());
        frames_bytes.extend_from_slice(&(rows as u32).to_le_bytes());
        frames_bytes.extend_from_slice(&0u32.to_le_bytes());
        Self {
            cols,
            rows,
            active,
            threshold_m,
            max_depth_m: vec![0.0; n],
            max_speed_m_s: vec![0.0; n],
            max_hazard_m2_s: vec![0.0; n],
            onset_time_s: vec![-1.0; n],
            frame_count: 0,
            frames_bytes,
        }
    }

    fn push_state(&mut self, time_s: f64, cells: &[CellState]) {
        let n = self.cols * self.rows;
        self.frames_bytes
            .extend_from_slice(&(time_s as f32).to_le_bytes());
        let mut vx_buf = Vec::with_capacity(n);
        let mut vy_buf = Vec::with_capacity(n);
        for (idx, (cell, &is_active)) in cells.iter().zip(self.active.iter()).enumerate() {
            if !is_active || !cell.h.is_finite() || cell.h <= 0.0 {
                self.frames_bytes.extend_from_slice(&0.0f32.to_le_bytes());
                vx_buf.push(0.0f32);
                vy_buf.push(0.0f32);
                continue;
            }
            let h = cell.h;
            let h_f32 = h as f32;
            self.frames_bytes.extend_from_slice(&h_f32.to_le_bytes());
            if h_f32 > self.max_depth_m[idx] {
                self.max_depth_m[idx] = h_f32;
            }
            if h >= self.threshold_m && self.onset_time_s[idx] < 0.0 {
                self.onset_time_s[idx] = time_s as f32;
            }
            let (vx, vy, speed) = if h >= VELOCITY_DRY_CUTOFF_M {
                let vx_v = cell.qx / h;
                let vy_v = cell.qy / h;
                let sp = vx_v.hypot(vy_v);
                if vx_v.is_finite() && vy_v.is_finite() && sp.is_finite() {
                    (vx_v as f32, vy_v as f32, sp)
                } else {
                    (0.0, 0.0, 0.0)
                }
            } else {
                (0.0, 0.0, 0.0)
            };
            vx_buf.push(vx);
            vy_buf.push(vy);
            if (speed as f32) > self.max_speed_m_s[idx] {
                self.max_speed_m_s[idx] = speed as f32;
            }
            let hazard = (h * (speed + 0.5)) as f32;
            if hazard > self.max_hazard_m2_s[idx] {
                self.max_hazard_m2_s[idx] = hazard;
            }
        }
        for &v in &vx_buf {
            self.frames_bytes.extend_from_slice(&v.to_le_bytes());
        }
        for &v in &vy_buf {
            self.frames_bytes.extend_from_slice(&v.to_le_bytes());
        }
        self.frame_count += 1;
    }

    fn finish(mut self) -> (EnvelopeSummaryJson, usize, Vec<u8>) {
        let count_bytes = (self.frame_count as u32).to_le_bytes();
        self.frames_bytes[16..20].copy_from_slice(&count_bytes);
        (
            EnvelopeSummaryJson {
                threshold_m: self.threshold_m,
                max_depth_m: self.max_depth_m,
                max_speed_m_s: self.max_speed_m_s,
                max_hazard_m2_s: self.max_hazard_m2_s,
                onset_time_s: self.onset_time_s,
            },
            self.frame_count,
            self.frames_bytes,
        )
    }
}

fn build_js_result(
    engine: &str,
    prob: PreparedProblem,
    accepted_time_s: f64,
    steps_taken: usize,
    ledger: SurfaceSolverF32Ledger,
    collector: FrameCollector,
) -> Result<JsValue, JsValue> {
    let (envelope, frame_count, frames_bytes) = collector.finish();
    let scale = ledger
        .initial_storage_m3
        .abs()
        .max(ledger.final_storage_m3.abs())
        .max(ledger.rain_m3.abs())
        .max(1.0);
    let rel_err = ledger.residual_m3.abs() / scale;
    let passed = rel_err <= 1e-3 || ledger.residual_m3.abs() <= 1.0;

    let response = RunGridResponseJson {
        run_id: format!("local-f32-{}", (accepted_time_s * 10.0) as u64),
        classification: "Exploratory",
        accepted_time_s,
        steps_taken,
        frame_count,
        water_budget: WaterBudgetJson {
            initial_storage_m3: ledger.initial_storage_m3,
            final_storage_m3: ledger.final_storage_m3,
            rain_m3: ledger.rain_m3,
            evaporation_m3: ledger.evaporation_m3,
            withdrawal_m3: ledger.withdrawal_m3,
            infiltration_m3: ledger.infiltration_m3,
            boundary_inflow_m3: ledger.boundary_inflow_m3,
            boundary_outflow_m3: ledger.boundary_outflow_m3,
            residual_m3: ledger.residual_m3,
            relative_error: rel_err,
        },
        budget_assessment: BudgetAssessmentJson {
            passed,
            residual_m3: ledger.residual_m3,
            relative_error: rel_err,
            max_abs_tolerance_m3: 1.0,
            max_relative_tolerance: 1e-3,
            note: format!("Browser {engine} surface solver"),
        },
        earthwork: prob.earthwork,
        metadata: prob.metadata,
        envelope,
    };

    let grid_json =
        serde_json::to_string(&response).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let frames_bin = Uint8Array::from(frames_bytes.as_slice());

    let out = Object::new();
    Reflect::set(
        &out,
        &JsValue::from_str("engine"),
        &JsValue::from_str(engine),
    )?;
    Reflect::set(
        &out,
        &JsValue::from_str("gridJson"),
        &JsValue::from_str(&grid_json),
    )?;
    Reflect::set(&out, &JsValue::from_str("framesBin"), &frames_bin)?;
    Ok(out.into())
}

fn solver_config() -> SurfaceSolverF32Config {
    SurfaceSolverF32Config {
        spatial_order: SurfaceF32SpatialOrder::SecondOrderMuscl,
        ..SurfaceSolverF32Config::default()
    }
}

/// Runs the 2D shallow-water simulation asynchronously on Browser WebGPU (`SurfaceSolverF32<WgpuRuntime>`).
#[wasm_bindgen]
pub async fn run_surface_webgpu(spec_json: String) -> Result<JsValue, JsValue> {
    let req: WasmSimRequest =
        serde_json::from_str(&spec_json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let prob = prepare_problem(&req).map_err(|e| JsValue::from_str(&e))?;

    let client = WgpuClient::new_async(WgpuDevice::new(0))
        .await
        .map_err(|e| JsValue::from_str(&format!("WebGPU init failed: {e}")))?;

    let solver = SurfaceSolverF32::<WgpuRuntime>::new(
        client,
        prob.domain.clone(),
        prob.initial.clone(),
        solver_config(),
    )
    .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let mut solver = attach_options(solver, &prob).map_err(|e| JsValue::from_str(&e))?;

    let mut collector = FrameCollector::new(
        prob.cols,
        prob.rows,
        prob.metadata.active_mask.clone(),
        prob.threshold_m,
    );
    let init_state = solver
        .state_async()
        .await
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    collector.push_state(0.0, &init_state);

    let target_frames = MAX_BROWSER_FRAMES.saturating_sub(1).max(1);
    let frame_dt = prob.duration_s / (target_frames as f64);
    let mut next_frame_s = frame_dt;
    let mut total_steps = 0usize;

    for iv in &prob.intervals {
        while solver.time_s() < iv.end_s - 1e-6 {
            let stop_s = next_frame_s.min(iv.end_s);
            let remaining = MAX_BROWSER_STEPS.saturating_sub(total_steps);
            if remaining == 0 {
                break;
            }
            let taken = solver
                .advance_controlled_async(
                    stop_s,
                    &iv.rain,
                    &iv.evap,
                    &iv.withdrawal,
                    remaining,
                    || false,
                    |_, _, _| {},
                )
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            total_steps += taken;
            if solver.time_s() + 1e-6 >= next_frame_s && collector.frame_count < MAX_BROWSER_FRAMES
            {
                let cells = solver
                    .state_async()
                    .await
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                collector.push_state(solver.time_s(), &cells);
                while next_frame_s <= solver.time_s() + 1e-6 {
                    next_frame_s += frame_dt;
                }
            }
        }
    }

    if collector.frame_count < 2 {
        let cells = solver
            .state_async()
            .await
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        collector.push_state(solver.time_s(), &cells);
    }

    let accepted_time_s = solver.time_s();
    let ledger = solver.ledger();
    build_js_result(
        "WebGPU · F32",
        prob,
        accepted_time_s,
        total_steps,
        ledger,
        collector,
    )
}

/// Runs the 2D shallow-water simulation on Browser WASM CPU (`SurfaceSolverF32<CpuRuntime>`).
#[wasm_bindgen]
pub fn run_surface_wasm_cpu(spec_json: String) -> Result<JsValue, JsValue> {
    let req: WasmSimRequest =
        serde_json::from_str(&spec_json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let prob = prepare_problem(&req).map_err(|e| JsValue::from_str(&e))?;

    let client = CpuRuntime::default_client(&CpuDevice::new());
    let solver = SurfaceSolverF32::<CpuRuntime>::new(
        client,
        prob.domain.clone(),
        prob.initial.clone(),
        solver_config(),
    )
    .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let mut solver = attach_options(solver, &prob).map_err(|e| JsValue::from_str(&e))?;

    let mut collector = FrameCollector::new(
        prob.cols,
        prob.rows,
        prob.metadata.active_mask.clone(),
        prob.threshold_m,
    );
    let init_state = solver
        .state()
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    collector.push_state(0.0, &init_state);

    let target_frames = MAX_BROWSER_FRAMES.saturating_sub(1).max(1);
    let frame_dt = prob.duration_s / (target_frames as f64);
    let mut next_frame_s = frame_dt;
    let mut total_steps = 0usize;

    for iv in &prob.intervals {
        while solver.time_s() < iv.end_s - 1e-6 {
            let stop_s = next_frame_s.min(iv.end_s);
            let remaining = MAX_BROWSER_STEPS.saturating_sub(total_steps);
            if remaining == 0 {
                break;
            }
            let taken = solver
                .advance_to(stop_s, &iv.rain, &iv.evap, &iv.withdrawal, remaining)
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            total_steps += taken;
            if solver.time_s() + 1e-6 >= next_frame_s && collector.frame_count < MAX_BROWSER_FRAMES
            {
                let cells = solver
                    .state()
                    .map_err(|e| JsValue::from_str(&e.to_string()))?;
                collector.push_state(solver.time_s(), &cells);
                while next_frame_s <= solver.time_s() + 1e-6 {
                    next_frame_s += frame_dt;
                }
            }
        }
    }

    if collector.frame_count < 2 {
        let cells = solver
            .state()
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        collector.push_state(solver.time_s(), &cells);
    }

    let accepted_time_s = solver.time_s();
    let ledger = solver.ledger();
    build_js_result(
        "WASM · F32",
        prob,
        accepted_time_s,
        total_steps,
        ledger,
        collector,
    )
}
