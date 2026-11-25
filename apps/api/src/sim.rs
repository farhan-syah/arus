//! Bounded interactive `hydr 0.1.0` simulation execution, binary frame cache, paired diff, and exports.

use std::cmp::Ordering as CmpOrdering;
use std::collections::{BTreeMap, BinaryHeap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use arus_domain::{GridSpec, RunClassification, SpatialReference};
use arus_evaluation::{BudgetAssessment, assess_water_budget};
use arus_geo::{
    EarthworkSummary, TerrainEdit, TerrainGrid, parse_esri_ascii_bounded, write_esri_ascii,
};
use arus_hydraulics::{
    BoundaryAxisSpec, BoundaryFaceSpec, BoundaryForcingSpec, CoastalForcingSpec, EnvelopeSummary,
    FrameCaptureConfig, FrameSeries, GridMetadata, LandSpec, ProbeSpec, RainIntervalSpec,
    SurfaceRunSpec,
};
use arus_jobs::{CancellationToken, StepBudget};
use arus_model_api::WaterBudget;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

/// Maximum request payload size accepted by interactive simulation endpoints (4 MiB).
pub const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
/// Maximum number of 2D grid cells permitted in an interactive API run.
pub const MAX_INTERACTIVE_CELLS: usize = 16_384;
/// Maximum number of solver steps permitted in an interactive API run.
pub const MAX_INTERACTIVE_STEPS: usize = 20_000;
/// Maximum number of exported animation frames per interactive run.
pub const MAX_INTERACTIVE_FRAMES: usize = 120;
/// Maximum concurrent interactive CPU simulations per API process.
pub const MAX_CONCURRENT_SIMS: usize = 2;
/// Maximum wall-clock duration per interactive simulation before cancellation.
pub const SIM_TIMEOUT_SECS: u64 = 20;
/// Maximum interactive runs kept in the bounded in-memory LRU cache.
pub const MAX_CACHED_RUNS: usize = 16;

/// Rainfall hyetograph shape for interactive simulations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum HyetographPreset {
    Uniform,
    #[default]
    PeakStorm,
}

/// Boundary condition mode for interactive simulations.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum BoundaryChoice {
    Closed,
    #[default]
    Transmissive,
    OpenStage {
        elevation_m: f64,
    },
}

/// Land infiltration choice for interactive simulations.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum LandChoice {
    #[default]
    Impervious,
    ModifiedGreenAmpt {
        suction_m: f64,
        conductivity_m_per_s: f64,
        maximum_deficit: f64,
    },
}

/// Precision profile requested by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PrecisionChoice {
    #[default]
    F64,
    F32,
}

/// Relative elevation edit applied to one grid cell (positive = raised bund, negative = detention pond).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InterventionCellDelta {
    pub col: usize,
    pub row: usize,
    pub delta_m: f64,
    pub reason: String,
}

/// Optional Bring-Your-Own-Data ESRI ASCII `.asc` terrain upload with explicit CRS/bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UploadedAsciiDem {
    pub ascii_content: String,
    pub projected_crs: String,
    pub vertical_datum: String,
    pub wgs84_bounds: [f64; 4],
}

/// Optional observed live gauge / flooded hotspot depth assimilated into valley cells at `t = 0`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservedGaugeSeed {
    pub lat: f64,
    pub lon: f64,
    pub depth_m: f64,
    #[serde(default = "default_gauge_radius")]
    pub radius_cells: usize,
}

fn default_gauge_radius() -> usize {
    3
}

/// Optional coarse spatial NWP rainfall grid (`nx` lon x `ny` lat points, row-major south-to-north)
/// bilinearly interpolated onto the 2D hydraulic mesh per interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialRainGrid {
    pub nx: usize,
    pub ny: usize,
    pub hourly_mm: Vec<Vec<f64>>,
}

/// Request body for `POST /api/v1/simulations` and `POST /api/v1/compare`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateSimulationRequest {
    #[serde(default = "default_lon")]
    pub center_lon: f64,
    #[serde(default = "default_lat")]
    pub center_lat: f64,
    #[serde(default = "default_cols")]
    pub cols: usize,
    #[serde(default = "default_rows")]
    pub rows: usize,
    #[serde(default = "default_dx_m")]
    pub dx_m: f64,
    #[serde(default = "default_rainfall")]
    pub rainfall_mm_per_hr: f64,
    #[serde(default)]
    pub hyetograph: HyetographPreset,
    #[serde(default = "default_duration_s")]
    pub duration_s: f64,
    #[serde(default = "default_manning_n")]
    pub manning_n: f64,
    #[serde(default)]
    pub land: LandChoice,
    #[serde(default)]
    pub boundary: BoundaryChoice,
    #[serde(default = "default_threshold_m")]
    pub threshold_m: f64,
    #[serde(default)]
    pub precision: PrecisionChoice,
    #[serde(default)]
    pub interventions: Vec<InterventionCellDelta>,
    #[serde(default)]
    pub ascii_dem: Option<UploadedAsciiDem>,
    /// Optional row-major bare-earth elevations (`m`) sampled from the 3D map DEM (e.g. Terrarium).
    #[serde(default)]
    pub terrain_elevations_m: Option<Vec<Option<f64>>>,
    /// Optional provenance label for `terrain_elevations_m` (e.g. `BrowserTerrariumScreening`).
    #[serde(default)]
    pub terrain_provenance: Option<String>,
    /// Optional uniform 10 m coastal wind forcing x-component (`m/s`, `hydr 0.2.0`).
    /// Optional antecedent rainfall total (mm) used to seed valley/channel pre-state before `t = 0`.
    #[serde(default)]
    pub antecedent_rain_mm: f64,
    /// Optional real hourly NWP rainfall forecast series (mm/hr per hour) driving multi-band
    /// build-up and dry-window recession.
    #[serde(default)]
    pub hourly_rain_mm: Vec<f64>,
    /// Optional spatially varying NWP rainfall grid (`nx x ny` points across the domain bbox).
    #[serde(default)]
    pub spatial_rain_grid: Option<SpatialRainGrid>,
    /// Optional hourly coastal tide level (m MSL) applied as a tide-lock `Stage` boundary on
    /// low-lying coastal outlet faces when above `1.2 m`.
    #[serde(default)]
    pub hourly_tide_m: Vec<f64>,
    /// Continuous subsurface / river / storm-drainage loss rate (mm/hr) during simulation.
    #[serde(default)]
    pub drainage_mm_per_hr: f64,
    /// Daily daytime potential evaporation (mm/day), applied diurnally.
    #[serde(default)]
    pub evaporation_mm_per_day: f64,
    /// Optional live observed flooded gauge seeds assimilated into local valley cells at `t = 0`.
    #[serde(default)]
    pub observed_gauges: Vec<ObservedGaugeSeed>,
    #[serde(default)]
    pub wind_x_m_per_s: f64,
    /// Optional uniform 10 m coastal wind forcing y-component (`m/s`, `hydr 0.2.0`).
    #[serde(default)]
    pub wind_y_m_per_s: f64,
}

const fn default_lon() -> f64 {
    101.535
}
const fn default_lat() -> f64 {
    3.031
}
const fn default_cols() -> usize {
    20
}
const fn default_rows() -> usize {
    18
}
const fn default_dx_m() -> f64 {
    75.0
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

/// Summary returned by `POST /api/v1/simulations`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationSummaryResponse {
    pub run_id: String,
    pub classification: RunClassification,
    pub terrain_provenance: String,
    pub accepted_time_s: f64,
    pub steps_taken: usize,
    pub frame_count: usize,
    pub water_budget: WaterBudget,
    pub budget_assessment: BudgetAssessment,
    pub earthwork: EarthworkSummary,
    pub wgs84_bounds: [f64; 4],
}

/// JSON metadata + exact per-step peak envelope returned by `GET /api/v1/runs/{run_id}/grid`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunGridResponse {
    pub run_id: String,
    pub classification: RunClassification,
    pub accepted_time_s: f64,
    pub steps_taken: usize,
    pub frame_count: usize,
    pub water_budget: WaterBudget,
    pub budget_assessment: BudgetAssessment,
    pub earthwork: EarthworkSummary,
    pub metadata: GridMetadata,
    pub envelope: EnvelopeSummary,
}

/// Paired baseline vs intervention response returned by `POST /api/v1/compare`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareSimulationsResponse {
    pub baseline_run_id: String,
    pub intervention_run_id: String,
    pub conservation_valid: bool,
    pub earthwork: EarthworkSummary,
    pub improved_cells: usize,
    pub worsened_cells: usize,
    pub max_reduction_m: f32,
    pub max_worsening_m: f32,
    /// Per-cell `intervention_max_depth_m - baseline_max_depth_m` (`< 0` improved, `> 0` worsened).
    pub delta_max_depth_m: Vec<f32>,
}

/// Cached interactive run held in memory.
#[derive(Debug, Clone)]
pub struct CachedSimulation {
    pub summary: SimulationSummaryResponse,
    pub series: FrameSeries,
    pub frames_bin: Vec<u8>,
}

/// Bounded in-memory LRU cache + concurrency limiter for interactive simulations.
#[derive(Debug)]
pub struct InteractiveStore {
    next_id: AtomicU64,
    semaphore: Arc<Semaphore>,
    cache: RwLock<LruRuns>,
}

#[derive(Debug, Default)]
struct LruRuns {
    order: VecDeque<String>,
    entries: BTreeMap<String, Arc<CachedSimulation>>,
}

impl Default for InteractiveStore {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_SIMS)),
            cache: RwLock::new(LruRuns::default()),
        }
    }
}

impl InteractiveStore {
    /// Lists cached interactive run IDs in most-recent-last order.
    pub fn list_ids(&self) -> Vec<String> {
        self.cache
            .read()
            .map(|guard| guard.order.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Retrieves a cached interactive run by ID.
    pub fn get(&self, run_id: &str) -> Option<Arc<CachedSimulation>> {
        self.cache
            .read()
            .ok()
            .and_then(|guard| guard.entries.get(run_id).cloned())
    }

    /// Inserts a run into the bounded LRU cache, evicting the oldest entry if full.
    pub fn insert(&self, run_id: String, sim: CachedSimulation) {
        if let Ok(mut guard) = self.cache.write() {
            if guard.entries.contains_key(&run_id) {
                guard.order.retain(|id| id != &run_id);
            } else if guard.order.len() >= MAX_CACHED_RUNS
                && let Some(oldest) = guard.order.pop_front()
            {
                guard.entries.remove(&oldest);
            }
            guard.order.push_back(run_id.clone());
            guard.entries.insert(run_id, Arc::new(sim));
        }
    }

    fn allocate_run_id(&self) -> String {
        let seq = self.next_id.fetch_add(1, Ordering::Relaxed);
        format!("sim-{seq:04}")
    }

    /// Executes one bounded interactive simulation on a blocking thread with timeout & semaphore.
    pub async fn run_simulation(
        &self,
        request: CreateSimulationRequest,
        fixed_id: Option<String>,
    ) -> Result<Arc<CachedSimulation>, (u16, String)> {
        validate_request(&request)?;
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| (503, "simulation worker pool unavailable".to_owned()))?;

        let run_id = fixed_id.unwrap_or_else(|| self.allocate_run_id());
        let token = CancellationToken::new();
        let cancel_handle = token.clone();
        let run_id_clone = run_id.clone();

        let join = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            execute_blocking(run_id_clone, &request, &token)
        });

        let result = tokio::time::timeout(Duration::from_secs(SIM_TIMEOUT_SECS), join).await;
        let sim = match result {
            Ok(Ok(Ok(sim))) => sim,
            Ok(Ok(Err(err))) => return Err(err),
            Ok(Err(join_err)) => return Err((500, format!("worker join error: {join_err}"))),
            Err(_) => {
                cancel_handle.cancel();
                return Err((
                    408,
                    format!("simulation exceeded {SIM_TIMEOUT_SECS}s wall-clock limit"),
                ));
            }
        };

        self.insert(run_id.clone(), sim);
        self.get(&run_id)
            .ok_or_else(|| (500, "failed to read newly cached simulation".to_owned()))
    }
}

fn validate_request(req: &CreateSimulationRequest) -> Result<(), (u16, String)> {
    if req.precision == PrecisionChoice::F32
        && (req.land != LandChoice::Impervious || !matches!(req.boundary, BoundaryChoice::Closed))
    {
        return Err((
            422,
            "hydr 0.1.0 F32 profile admits Closed + Impervious only; use F64 for infiltration or open boundaries".to_owned(),
        ));
    }
    if req.precision == PrecisionChoice::F32 {
        return Err((
            422,
            "interactive per-frame binary streaming requires F64 reference solver in hydr 0.1.0"
                .to_owned(),
        ));
    }
    if !req.center_lon.is_finite()
        || !(-180.0..=180.0).contains(&req.center_lon)
        || !req.center_lat.is_finite()
        || !(-85.0..=85.0).contains(&req.center_lat)
    {
        return Err((
            422,
            "center_lon must be in [-180, 180] and center_lat in [-85, 85]".to_owned(),
        ));
    }
    if !req.rainfall_mm_per_hr.is_finite() || !(0.0..=1000.0).contains(&req.rainfall_mm_per_hr) {
        return Err((
            422,
            "rainfall_mm_per_hr must be finite in [0, 1000]".to_owned(),
        ));
    }
    if !req.antecedent_rain_mm.is_finite() || !(0.0..=500.0).contains(&req.antecedent_rain_mm) {
        return Err((
            422,
            "antecedent_rain_mm must be finite in [0, 500]".to_owned(),
        ));
    }
    if !req.duration_s.is_finite() || !(10.0..=3600.0).contains(&req.duration_s) {
        return Err((422, "duration_s must be finite in [10, 3600]".to_owned()));
    }
    if !req.manning_n.is_finite() || !(0.005..=0.5).contains(&req.manning_n) {
        return Err((422, "manning_n must be finite in [0.005, 0.5]".to_owned()));
    }
    if !req.threshold_m.is_finite() || req.threshold_m <= 0.0 {
        return Err((422, "threshold_m must be finite positive".to_owned()));
    }
    if req.ascii_dem.is_none() {
        let cells = req.cols.saturating_mul(req.rows);
        if req.cols < 4 || req.rows < 4 || cells > MAX_INTERACTIVE_CELLS {
            return Err((
                422,
                format!("grid must be at least 4x4 and <= {MAX_INTERACTIVE_CELLS} cells"),
            ));
        }
        if !req.dx_m.is_finite() || !(1.0..=2500.0).contains(&req.dx_m) {
            return Err((422, "dx_m must be finite in [1, 2500] metres".to_owned()));
        }
    }
    Ok(())
}

fn execute_blocking(
    run_id: String,
    req: &CreateSimulationRequest,
    token: &CancellationToken,
) -> Result<CachedSimulation, (u16, String)> {
    let (base_terrain, wgs84_bounds, terrain_provenance) = match &req.ascii_dem {
        Some(upload) => {
            if upload.projected_crs.to_ascii_uppercase().contains("4326") {
                return Err((
                    422,
                    "EPSG:4326 degree grids are not metric; supply a projected metric CRS (e.g. EPSG:3375 / UTM)".to_owned(),
                ));
            }
            let grid = parse_esri_ascii_bounded(
                &upload.ascii_content,
                &upload.projected_crs,
                &upload.vertical_datum,
                "UploadedAsciiDem",
                MAX_INTERACTIVE_CELLS,
            )
            .map_err(|e| (422, e.to_string()))?;
            if grid.grid().dx_m < 0.1 {
                return Err((
                    422,
                    format!(
                        "cellsize {} is smaller than 0.1 m; check that the DEM is in projected metres, not degrees",
                        grid.grid().dx_m
                    ),
                ));
            }
            (grid, upload.wgs84_bounds, "UploadedAsciiDem".to_owned())
        }
        None => {
            if let Some(sampled) = &req.terrain_elevations_m {
                let expected = req.cols.saturating_mul(req.rows);
                if sampled.len() != expected {
                    return Err((
                        422,
                        format!(
                            "terrain_elevations_m length {} does not match {}x{} ({expected})",
                            sampled.len(),
                            req.cols,
                            req.rows
                        ),
                    ));
                }
                let prov = req
                    .terrain_provenance
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| "BrowserTerrariumScreening".to_owned());
                let bounds = compute_wgs84_bounds(
                    req.center_lon,
                    req.center_lat,
                    req.cols,
                    req.rows,
                    req.dx_m,
                );
                let mut conditioned = sampled.clone();
                if prov == "BrowserTerrariumScreening" {
                    condition_hydrological_dem(&mut conditioned, req.cols, req.rows);
                }
                let grid = TerrainGrid::new(
                    GridSpec {
                        columns: req.cols,
                        rows: req.rows,
                        dx_m: req.dx_m,
                        dy_m: req.dx_m,
                    },
                    SpatialReference {
                        projected_crs: format!(
                            "local-metric-tangent@{:.4},{:.4}",
                            req.center_lat, req.center_lon
                        ),
                        vertical_datum: "egm96-terrarium-screening".to_owned(),
                        origin_m: [0.0, 0.0],
                    },
                    prov.clone(),
                    conditioned,
                )
                .map_err(|e| (422, e.to_string()))?;
                (grid, bounds, prov)
            } else {
                let (grid, bounds) = build_synthetic_valley_terrain(
                    req.center_lon,
                    req.center_lat,
                    req.cols,
                    req.rows,
                    req.dx_m,
                )
                .map_err(|e| (422, e))?;
                (grid, bounds, "SyntheticDemoBasin".to_owned())
            }
        }
    };

    let (cols, rows) = (base_terrain.grid().columns, base_terrain.grid().rows);
    let mut edits = Vec::with_capacity(req.interventions.len());
    for delta in &req.interventions {
        if delta.col >= cols || delta.row >= rows {
            return Err((
                422,
                format!(
                    "intervention ({}, {}) out of bounds ({cols}x{rows})",
                    delta.col, delta.row
                ),
            ));
        }
        let idx = delta.row * cols + delta.col;
        if let Some(prev) = base_terrain.cells()[idx] {
            edits.push(TerrainEdit::CellElevationOverride {
                cell: idx,
                new_elevation_m: prev + delta.delta_m,
                reason: if delta.reason.trim().is_empty() {
                    "interactive map intervention".to_owned()
                } else {
                    delta.reason.clone()
                },
            });
        }
    }
    let (terrain, earthwork) = base_terrain
        .apply_edits(edits)
        .map_err(|e| (422, e.to_string()))?;

    let rain_m_per_s = req.rainfall_mm_per_hr / 3_600_000.0;
    let open_faces: Vec<BoundaryFaceSpec> = match req.boundary {
        BoundaryChoice::Closed => Vec::new(),
        BoundaryChoice::Transmissive | BoundaryChoice::OpenStage { .. } => {
            // Open perimeter faces on low-lying coastal/valley outlets (never steep mountain cliffs)
            let mut perim_z: Vec<f64> = Vec::new();
            for r in 0..rows {
                for c in 0..cols {
                    if (r == 0 || r + 1 == rows || c == 0 || c + 1 == cols)
                        && let Some(z) = terrain.cells()[r * cols + c]
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
            let mut faces = Vec::new();
            for row in 0..rows {
                if terrain.cells()[row * cols].is_some_and(|z| z <= outlet_max_z) {
                    faces.push(BoundaryFaceSpec {
                        axis: BoundaryAxisSpec::X,
                        column: 0,
                        row,
                    });
                }
                if terrain.cells()[row * cols + (cols - 1)].is_some_and(|z| z <= outlet_max_z) {
                    faces.push(BoundaryFaceSpec {
                        axis: BoundaryAxisSpec::X,
                        column: cols,
                        row,
                    });
                }
            }
            for col in 0..cols {
                if terrain.cells()[col].is_some_and(|z| z <= outlet_max_z) {
                    faces.push(BoundaryFaceSpec {
                        axis: BoundaryAxisSpec::Y,
                        column: col,
                        row: 0,
                    });
                }
                if terrain.cells()[(rows - 1) * cols + col].is_some_and(|z| z <= outlet_max_z) {
                    faces.push(BoundaryFaceSpec {
                        axis: BoundaryAxisSpec::Y,
                        column: col,
                        row: rows,
                    });
                }
            }
            faces
        }
    };
    let boundary_forcings: Vec<BoundaryForcingSpec> = match req.boundary {
        BoundaryChoice::Closed => Vec::new(),
        BoundaryChoice::Transmissive => open_faces
            .iter()
            .copied()
            .map(|face| BoundaryForcingSpec::ExteriorDry { face })
            .collect(),
        BoundaryChoice::OpenStage { elevation_m } => open_faces
            .iter()
            .copied()
            .map(|face| BoundaryForcingSpec::Stage {
                face,
                elevation_m,
                inflow_tangential_velocity_m_s: None,
            })
            .collect(),
    };

    // Stage 1: Channel-only initial water depth (hills = 0.0 m; rivers/valleys + observed gauges wet)
    let has_prestate = req.antecedent_rain_mm > 0.0 || !req.observed_gauges.is_empty();
    let per_cell_initial: Option<Vec<f64>> = if has_prestate {
        Some(build_channel_prestate(
            terrain.cells(),
            cols,
            rows,
            wgs84_bounds,
            req.antecedent_rain_mm,
            &req.observed_gauges,
        ))
    } else {
        None
    };

    let warmup_s = 0.0;
    let base_drain_m_per_s = req.drainage_mm_per_hr.max(0.0) / 3_600_000.0;
    let base_evap_mean_m_per_s = req.evaporation_mm_per_day.max(0.0) / 86_400_000.0;

    let mut forcing = Vec::new();
    let mut interval_losses: Vec<(f64, f64)> = Vec::new();
    let mut interval_rain_grids: Vec<Vec<f64>> = Vec::new();
    let mut interval_loss_grids: Vec<Vec<f64>> = Vec::new();
    let mut use_spatial_grids = false;

    let has_spatial = req.spatial_rain_grid.as_ref().is_some_and(|g| {
        g.nx >= 2 && g.ny >= 2 && g.hourly_mm.len() == g.nx * g.ny && !g.hourly_mm[0].is_empty()
    });

    if has_spatial || !req.hourly_rain_mm.is_empty() {
        // Stage 2-3: Real multi-hour NWP forcing (spatial 2D grid + D8 channel concentration +
        // coastal tide-lock + diurnal evaporation + slow lowland/depression drainage for stagnant ponding).
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

        let accum = compute_d8_flow_accum(terrain.cells(), cols, rows);
        let depression_mask = detect_real_depressions(terrain.cells(), cols, rows);
        let mut valid_z: Vec<f64> = terrain.cells().iter().filter_map(|&z| z).collect();
        valid_z.sort_by(f64::total_cmp);
        let low_z_cutoff = if valid_z.is_empty() {
            10.0
        } else {
            valid_z[(valid_z.len() * 30) / 100]
        };
        let channel_accum_min = ((cols * rows) as f64).sqrt().mul_add(0.28, 0.0).max(5.0) as u32;
        use_spatial_grids = true;

        for b in 0..n_bins {
            let h0 = (b * n_hours) / n_bins;
            let h1 = (((b + 1) * n_hours) / n_bins).max(h0 + 1).min(n_hours);
            let bin_len = (h1 - h0).max(1) as f64;

            // Diurnal solar evaporation: peaks around midday (local hour 9..17)
            let mid_h = ((h0 + h1) / 2 + 8) % 24;
            let solar_weight = if (7..=18).contains(&mid_h) {
                let phase = ((mid_h as f64 - 7.0) / 11.0) * std::f64::consts::PI;
                phase.sin().max(0.0) * 2.2
            } else {
                0.05
            };
            let eff_evap_m_s = base_evap_mean_m_per_s * solar_weight * time_compression;
            let eff_drain_m_s = base_drain_m_per_s * time_compression;

            // Build per-cell rainfall grid (bilinear spatial NWP + D8 upstream channel routing)
            let mut rain_grid = vec![0.0f64; cols * rows];
            let mut loss_grid = vec![0.0f64; cols * rows];

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
            let fallback_mean_mm: f64 = if !has_spatial {
                req.hourly_rain_mm[h0..h1]
                    .iter()
                    .copied()
                    .filter(|v| v.is_finite() && *v > 0.0)
                    .sum::<f64>()
                    / bin_len
            } else {
                bin_pt_means.iter().sum::<f64>() / (bin_pt_means.len().max(1) as f64)
            };

            let bin_tide_m = if !req.hourly_tide_m.is_empty() {
                let t_end = h1.min(req.hourly_tide_m.len());
                let t_start = h0.min(t_end);
                req.hourly_tide_m[t_start..t_end]
                    .iter()
                    .copied()
                    .fold(f64::NEG_INFINITY, f64::max)
            } else {
                0.0
            };
            let tide_lock = if bin_tide_m > 1.2 {
                ((bin_tide_m - 1.2) / 1.0).clamp(0.0, 1.0)
            } else {
                0.0
            };

            for r in 0..rows {
                for c in 0..cols {
                    let idx = r * cols + c;
                    let Some(z) = terrain.cells()[idx] else {
                        continue;
                    };
                    let local_mm_hr = if let Some(sg) = &req.spatial_rain_grid
                        && has_spatial
                    {
                        bilinear_sample_grid(
                            &bin_pt_means,
                            sg.nx,
                            sg.ny,
                            (c as f64 + 0.5) / (cols as f64),
                            (r as f64 + 0.5) / (rows as f64),
                        )
                    } else {
                        fallback_mean_mm
                    };
                    // D8 upstream channel concentration: river cells receive routed upstream inflow
                    let a = accum[idx];
                    let routing_mult = if a >= channel_accum_min {
                        1.0 + ((a as f64) / (channel_accum_min as f64)).ln_1p().min(1.6) * 0.55
                    } else {
                        0.85
                    };
                    // High-tide backwater lock on coastal lowlands (z <= 3.5 m)
                    let coastal_surge_m_s = if z <= 3.5 && tide_lock > 0.0 {
                        (0.45 * tide_lock / 3_600_000.0) * time_compression
                    } else {
                        0.0
                    };
                    rain_grid[idx] = (local_mm_hr / 3_600_000.0) * time_compression * routing_mult
                        + coastal_surge_m_s;

                    // Spatially varying drainage: uplands drain faster; real depressions,
                    // tide-locked coastal lowlands, and saturated valleys drain slowly so stagnant ponded water persists.
                    let mut drain_factor = if depression_mask[idx] {
                        0.20
                    } else if z <= low_z_cutoff {
                        0.50
                    } else {
                        1.35
                    };
                    if z <= 3.5 && tide_lock > 0.0 {
                        drain_factor *= 1.0 - 0.75 * tide_lock;
                    }
                    loss_grid[idx] = eff_evap_m_s + eff_drain_m_s * drain_factor;
                }
            }

            forcing.push(RainIntervalSpec {
                start_s: warmup_s + (b as f64) * bin_dur_s,
                end_s: warmup_s + ((b + 1) as f64) * bin_dur_s,
                uniform_rain_m_per_s: (fallback_mean_mm / 3_600_000.0) * time_compression,
                boundaries: boundary_forcings.clone(),
            });
            interval_losses.push((eff_evap_m_s, eff_drain_m_s));
            interval_rain_grids.push(rain_grid);
            interval_loss_grids.push(loss_grid);
        }
    } else {
        match req.hyetograph {
            HyetographPreset::Uniform => {
                forcing.push(RainIntervalSpec {
                    start_s: warmup_s,
                    end_s: warmup_s + req.duration_s,
                    uniform_rain_m_per_s: rain_m_per_s,
                    boundaries: boundary_forcings,
                });
                interval_losses.push((base_evap_mean_m_per_s, base_drain_m_per_s));
            }
            HyetographPreset::PeakStorm => {
                let t1 = warmup_s + req.duration_s * 0.25;
                let t2 = warmup_s + req.duration_s * 0.55;
                forcing.push(RainIntervalSpec {
                    start_s: warmup_s,
                    end_s: t1,
                    uniform_rain_m_per_s: rain_m_per_s * 0.65,
                    boundaries: boundary_forcings.clone(),
                });
                interval_losses.push((base_evap_mean_m_per_s, base_drain_m_per_s));
                forcing.push(RainIntervalSpec {
                    start_s: t1,
                    end_s: t2,
                    uniform_rain_m_per_s: rain_m_per_s * 1.6,
                    boundaries: boundary_forcings.clone(),
                });
                interval_losses.push((base_evap_mean_m_per_s * 0.5, base_drain_m_per_s));
                forcing.push(RainIntervalSpec {
                    start_s: t2,
                    end_s: warmup_s + req.duration_s,
                    uniform_rain_m_per_s: 0.0,
                    boundaries: boundary_forcings,
                });
                interval_losses.push((base_evap_mean_m_per_s * 1.8, base_drain_m_per_s));
            }
        }
    }

    let land = match req.land {
        LandChoice::Impervious => LandSpec::UniformImpervious,
        LandChoice::ModifiedGreenAmpt {
            suction_m,
            conductivity_m_per_s,
            maximum_deficit,
        } => LandSpec::UniformModifiedGreenAmpt {
            suction_m,
            conductivity_m_per_s,
            maximum_deficit,
        },
    };

    let initial_depth_m = if terrain_provenance == "BrowserTerrariumScreening" {
        0.0
    } else {
        0.14
    };
    let spec = SurfaceRunSpec {
        run_id: run_id.clone(),
        classification: RunClassification::NumericalDemonstration,
        terrain,
        uniform_manning_n: req.manning_n,
        land,
        initial_depth_m,
        open_faces,
        forcing,
        target_time_s: warmup_s + req.duration_s,
        max_steps: MAX_INTERACTIVE_STEPS,
        probes: vec![ProbeSpec {
            id: "basin-center".to_owned(),
            x_m: (cols as f64) * req.dx_m * 0.5,
            y_m: (rows as f64) * req.dx_m * 0.5,
        }],
        threshold_m: req.threshold_m,
        hysteresis_m: (req.threshold_m * 0.15).max(0.005),
        recession_persistence_s: 15.0,
        coastal: if req.wind_x_m_per_s != 0.0 || req.wind_y_m_per_s != 0.0 {
            Some(CoastalForcingSpec {
                wind_x_m_per_s: req.wind_x_m_per_s,
                wind_y_m_per_s: req.wind_y_m_per_s,
                ..CoastalForcingSpec::default()
            })
        } else {
            None
        },
    };

    let mut engine = spec
        .build_engine_with_spatial_physics(
            per_cell_initial.as_deref(),
            if use_spatial_grids {
                Some(&interval_rain_grids)
            } else {
                None
            },
            if use_spatial_grids {
                Some(&interval_loss_grids)
            } else {
                None
            },
            Some(&interval_losses),
        )
        .map_err(|e| (422, e.to_string()))?;
    if warmup_s > 0.0 {
        engine
            .advance_to(warmup_s, StepBudget::new(MAX_INTERACTIVE_STEPS))
            .map_err(|e| (422, e.to_string()))?;
    }
    let target_frames = 48_usize.min(MAX_INTERACTIVE_FRAMES);
    let frame_interval_s = (req.duration_s / (target_frames.saturating_sub(1) as f64)).max(1.0);
    let (advance, mut series) = engine
        .advance_with_frames(
            warmup_s + req.duration_s,
            StepBudget::new(MAX_INTERACTIVE_STEPS),
            token,
            FrameCaptureConfig {
                frame_interval_s,
                max_frames: MAX_INTERACTIVE_FRAMES,
                threshold_m: req.threshold_m,
                wgs84_bounds,
                terrain_provenance: terrain_provenance.clone(),
            },
        )
        .map_err(|e| (422, e.to_string()))?;
    if warmup_s > 0.0 {
        let warmup_f32 = warmup_s as f32;
        for frame in &mut series.frames {
            frame.time_s = (frame.time_s - warmup_f32).max(0.0);
        }
    }

    let frames_bin = series
        .encode_frames_bin()
        .map_err(|e| (500, e.to_string()))?;
    let water_budget = engine.water_budget();
    let budget_assessment = assess_water_budget(water_budget);

    let summary = SimulationSummaryResponse {
        run_id,
        classification: RunClassification::NumericalDemonstration,
        terrain_provenance,
        accepted_time_s: advance.accepted_time_s,
        steps_taken: advance.steps_taken,
        frame_count: series.frames.len(),
        water_budget,
        budget_assessment,
        earthwork,
        wgs84_bounds,
    };

    Ok(CachedSimulation {
        summary,
        series,
        frames_bin,
    })
}

fn compute_wgs84_bounds(
    center_lon: f64,
    center_lat: f64,
    cols: usize,
    rows: usize,
    dx_m: f64,
) -> [f64; 4] {
    let width_m = cols as f64 * dx_m;
    let height_m = rows as f64 * dx_m;
    let m_per_deg_lat = 111_320.0;
    let m_per_deg_lon = (111_320.0 * center_lat.to_radians().cos()).max(10_000.0);
    let half_dlat = (height_m * 0.5) / m_per_deg_lat;
    let half_dlon = (width_m * 0.5) / m_per_deg_lon;
    [
        center_lon - half_dlon,
        center_lat - half_dlat,
        center_lon + half_dlon,
        center_lat + half_dlat,
    ]
}

/// Builds a georeferenced synthetic sloping river-valley basin centered at `(center_lon, center_lat)`
/// with a winding central channel depression and corner nodata cells so coverage masks are visible.
fn build_synthetic_valley_terrain(
    center_lon: f64,
    center_lat: f64,
    cols: usize,
    rows: usize,
    dx_m: f64,
) -> Result<(TerrainGrid, [f64; 4]), String> {
    let bounds = compute_wgs84_bounds(center_lon, center_lat, cols, rows, dx_m);

    let mut cells = Vec::with_capacity(cols * rows);
    for r in 0..rows {
        let ny = (r as f64 + 0.5) / (rows as f64);
        for c in 0..cols {
            let nx = (c as f64 + 0.5) / (cols as f64);
            // Leave a small upper-right corner as explicit nodata to show out-of-coverage honesty.
            if c >= cols.saturating_sub(2) && r >= rows.saturating_sub(2) {
                cells.push(None);
                continue;
            }
            // Gently sloping valley from west (high) to east (outlet), with a winding river thalweg.
            let channel_y = 0.5 + 0.18 * (nx * std::f64::consts::TAU * 1.15).sin();
            let dist = (ny - channel_y).abs();
            let bank = (dist * dist * 5.2).min(2.6);
            let slope = (1.0 - nx) * 1.35;
            // Low-lying urban floodplain pocket near middle-south.
            let pocket_d2 = (nx - 0.52).powi(2) + (ny - 0.36).powi(2);
            let pocket = -0.55 * (-pocket_d2 / 0.022).exp();
            let elev = (1.4 + slope + bank + pocket).max(0.1);
            cells.push(Some(elev));
        }
    }

    let grid = TerrainGrid::new(
        GridSpec {
            columns: cols,
            rows,
            dx_m,
            dy_m: dx_m,
        },
        SpatialReference {
            projected_crs: format!("local-metric-tangent@{center_lat:.4},{center_lon:.4}"),
            vertical_datum: "local-synthetic-datum".to_owned(),
            origin_m: [0.0, 0.0],
        },
        "SyntheticDemoBasin",
        cells,
    )
    .map_err(|e| e.to_string())?;
    Ok((grid, bounds))
}

/// Formats `max_depth_m` from a cached simulation as an ESRI ASCII Grid (`.asc`) string.
pub fn export_max_depth_ascii(sim: &CachedSimulation) -> Result<String, String> {
    let meta = &sim.series.metadata;
    let cells: Vec<Option<f64>> = sim
        .series
        .envelope
        .max_depth_m
        .iter()
        .zip(meta.active_mask.iter())
        .map(|(&d, &active)| if active { Some(f64::from(d)) } else { None })
        .collect();
    let grid = TerrainGrid::new(
        GridSpec {
            columns: meta.cols,
            rows: meta.rows,
            dx_m: meta.dx_m,
            dy_m: meta.dy_m,
        },
        SpatialReference {
            projected_crs: meta.projected_crs.clone(),
            vertical_datum: meta.vertical_datum.clone(),
            origin_m: meta.origin_m,
        },
        "max-depth-export",
        cells,
    )
    .map_err(|e| e.to_string())?;
    write_esri_ascii(&grid, -9999.0).map_err(|e| e.to_string())
}

/// Formats per-cell peak depth, speed, DEFRA FD2320 hazard, and onset time as CSV.
pub fn export_envelope_csv(sim: &CachedSimulation) -> String {
    let meta = &sim.series.metadata;
    let env = &sim.series.envelope;
    let mut out = String::from(
        "col,row,x_m,y_m,active,bed_m,max_depth_m,max_speed_m_s,max_hazard_fd2320,onset_time_s\n",
    );
    for r in 0..meta.rows {
        for c in 0..meta.cols {
            let idx = r * meta.cols + c;
            let x_m = meta.origin_m[0] + (c as f64 + 0.5) * meta.dx_m;
            let y_m = meta.origin_m[1] + (r as f64 + 0.5) * meta.dy_m;
            let active = meta.active_mask[idx];
            let line = format!(
                "{c},{r},{x_m:.2},{y_m:.2},{active},{:.4},{:.4},{:.4},{:.4},{:.2}\n",
                meta.bed_m[idx],
                env.max_depth_m[idx],
                env.max_speed_m_s[idx],
                env.max_hazard_m2_s[idx],
                env.onset_time_s[idx]
            );
            out.push_str(&line);
        }
    }
    out
}

#[derive(Clone, Copy)]
struct MinF64 {
    val: f64,
    idx: usize,
}

impl PartialEq for MinF64 {
    fn eq(&self, other: &Self) -> bool {
        self.val.to_bits() == other.val.to_bits() && self.idx == other.idx
    }
}
impl Eq for MinF64 {}
impl PartialOrd for MinF64 {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}
impl Ord for MinF64 {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        // Reverse order for min-heap
        other
            .val
            .total_cmp(&self.val)
            .then_with(|| other.idx.cmp(&self.idx))
    }
}

/// Barnes Priority-Flood depression filling with a gentle monotonic drainage gradient (`epsilon`)
/// so coarse Terrarium DEMs do not trap water in artificial single-cell hilltop pits.
fn condition_hydrological_dem(elevations: &mut [Option<f64>], cols: usize, rows: usize) {
    let n = cols * rows;
    if elevations.len() != n || cols < 3 || rows < 3 {
        return;
    }
    let mut visited = vec![false; n];
    let mut heap: BinaryHeap<MinF64> = BinaryHeap::with_capacity(n);

    for r in 0..rows {
        for c in 0..cols {
            if r == 0 || r + 1 == rows || c == 0 || c + 1 == cols {
                let idx = r * cols + c;
                if let Some(z) = elevations[idx] {
                    visited[idx] = true;
                    heap.push(MinF64 { val: z, idx });
                }
            }
        }
    }

    let eps = 0.04;
    let dirs: [(isize, isize); 8] = [
        (-1, 0),
        (1, 0),
        (0, -1),
        (0, 1),
        (-1, -1),
        (-1, 1),
        (1, -1),
        (1, 1),
    ];

    while let Some(cur) = heap.pop() {
        let r = (cur.idx / cols) as isize;
        let c = (cur.idx % cols) as isize;
        for (dr, dc) in dirs {
            let nr = r + dr;
            let nc = c + dc;
            if nr < 0 || nc < 0 || nr >= rows as isize || nc >= cols as isize {
                continue;
            }
            let nidx = (nr as usize) * cols + (nc as usize);
            if visited[nidx] {
                continue;
            }
            visited[nidx] = true;
            if let Some(nz) = elevations[nidx] {
                // Selective pit conditioning: fill shallow DEM quantization noise (<= 0.6 m),
                // but preserve deeper natural depressions/backswamps so stagnant water can trap.
                let filled = if nz < cur.val + eps && (cur.val - nz) <= 0.6 {
                    cur.val + eps
                } else {
                    nz
                };
                elevations[nidx] = Some(filled);
                heap.push(MinF64 {
                    val: filled.max(cur.val),
                    idx: nidx,
                });
            }
        }
    }
}

/// Computes D8 contributing drainage area (in cell count) on a hydrologically conditioned DEM.
fn compute_d8_flow_accum(elevations: &[Option<f64>], cols: usize, rows: usize) -> Vec<u32> {
    let n = cols * rows;
    let mut order: Vec<(f64, usize)> = elevations
        .iter()
        .enumerate()
        .filter_map(|(i, z)| z.map(|v| (v, i)))
        .collect();
    order.sort_by(|a, b| b.0.total_cmp(&a.0));

    let mut accum = vec![1u32; n];
    let dirs: [(isize, isize); 8] = [
        (-1, 0),
        (1, 0),
        (0, -1),
        (0, 1),
        (-1, -1),
        (-1, 1),
        (1, -1),
        (1, 1),
    ];
    for (z, idx) in order {
        let r = (idx / cols) as isize;
        let c = (idx % cols) as isize;
        let mut best_slope = 0.0;
        let mut best_n = None;
        for (dr, dc) in dirs {
            let nr = r + dr;
            let nc = c + dc;
            if nr < 0 || nc < 0 || nr >= rows as isize || nc >= cols as isize {
                continue;
            }
            let nidx = (nr as usize) * cols + (nc as usize);
            if let Some(nz) = elevations[nidx] {
                let dist = if dr != 0 && dc != 0 {
                    std::f64::consts::SQRT_2
                } else {
                    1.0
                };
                let slope = (z - nz) / dist;
                if slope > best_slope {
                    best_slope = slope;
                    best_n = Some(nidx);
                }
            }
        }
        if let Some(target) = best_n {
            accum[target] = accum[target].saturating_add(accum[idx]);
        }
    }
    accum
}

/// Builds a channel-only / valley-only pre-state depth array at `t = 0`:
/// hillslopes start at `0.0 m`, while high flow-accumulation river corridors, coastal lowlands,
/// and localized valley cells around live observed flooded gauges start wet.
fn build_channel_prestate(
    elevations: &[Option<f64>],
    cols: usize,
    rows: usize,
    bounds: [f64; 4],
    antecedent_rain_mm: f64,
    gauges: &[ObservedGaugeSeed],
) -> Vec<f64> {
    let n = cols * rows;
    let mut depths = vec![0.0f64; n];
    let accum = compute_d8_flow_accum(elevations, cols, rows);

    // Determine elevation 25th percentile (lowland/valley floor) and flow-accumulation threshold
    let mut valid_z: Vec<f64> = elevations.iter().filter_map(|&z| z).collect();
    valid_z.sort_by(f64::total_cmp);
    let low_z_cutoff = if valid_z.is_empty() {
        10.0
    } else {
        valid_z[(valid_z.len() * 35) / 100]
    };
    let channel_accum_min = ((n as f64).sqrt() * 0.22).round().max(4.0) as u32;

    if antecedent_rain_mm > 0.0 {
        let scale = (antecedent_rain_mm / 60.0).clamp(0.25, 2.5);
        for idx in 0..n {
            let Some(z) = elevations[idx] else { continue };
            let a = accum[idx];
            if a >= channel_accum_min && z <= low_z_cutoff + 18.0 {
                let channel_factor = ((a as f64) / (channel_accum_min as f64)).ln_1p().min(2.6);
                depths[idx] = (0.14 * scale * channel_factor).min(0.45);
            }
        }
    }

    // Assimilate live observed gauge excess depths into local valley/channel cells around each gauge
    let [min_lon, min_lat, max_lon, max_lat] = bounds;
    let lon_span = (max_lon - min_lon).max(1e-6);
    let lat_span = (max_lat - min_lat).max(1e-6);
    for g in gauges {
        if !g.depth_m.is_finite() || g.depth_m <= 0.0 {
            continue;
        }
        let fx = ((g.lon - min_lon) / lon_span) * (cols as f64);
        let fy = ((g.lat - min_lat) / lat_span) * (rows as f64);
        if fx < -2.0 || fx > (cols as f64 + 2.0) || fy < -2.0 || fy > (rows as f64 + 2.0) {
            continue;
        }
        let gc = (fx.floor() as isize).clamp(0, cols as isize - 1);
        let gr = (fy.floor() as isize).clamp(0, rows as isize - 1);
        let rad = (g.radius_cells.clamp(1, 6)) as isize;

        // Find local valley floor near gauge so water stays in the valley, never on adjacent peaks
        let mut local_min_z = f64::INFINITY;
        for dr in -rad..=rad {
            for dc in -rad..=rad {
                let nr = gr + dr;
                let nc = gc + dc;
                if nr >= 0
                    && nc >= 0
                    && nr < rows as isize
                    && nc < cols as isize
                    && let Some(z) = elevations[(nr as usize) * cols + (nc as usize)]
                    && z < local_min_z
                {
                    local_min_z = z;
                }
            }
        }
        if !local_min_z.is_finite() {
            continue;
        }

        let rad_f = rad as f64;
        for dr in -rad..=rad {
            for dc in -rad..=rad {
                let nr = gr + dr;
                let nc = gc + dc;
                if nr < 0 || nc < 0 || nr >= rows as isize || nc >= cols as isize {
                    continue;
                }
                let dist = ((dr * dr + dc * dc) as f64).sqrt();
                if dist > rad_f {
                    continue;
                }
                let idx = (nr as usize) * cols + (nc as usize);
                if let Some(z) = elevations[idx] {
                    // Flood cells within 14m of local valley floor around observed gauge
                    let elev_drop = ((z - local_min_z) / 14.0).clamp(0.0, 1.0);
                    let w = (1.0 - dist / (rad_f + 0.6)) * (1.0 - elev_drop * 0.75);
                    let seeded = (g.depth_m * w).min(0.55);
                    if seeded > depths[idx] {
                        depths[idx] = seeded;
                    }
                }
            }
        }
    }

    // One gentle smoothing pass on initial depths to avoid unphysical wet-dry step shocks
    let mut smoothed = depths.clone();
    for r in 1..rows.saturating_sub(1) {
        for c in 1..cols.saturating_sub(1) {
            let idx = r * cols + c;
            if elevations[idx].is_none() {
                continue;
            }
            let avg = (depths[idx] * 2.0
                + depths[idx - 1]
                + depths[idx + 1]
                + depths[idx - cols]
                + depths[idx + cols])
                / 6.0;
            smoothed[idx] = avg;
        }
    }
    smoothed
}

/// Identifies natural topographic depressions/backswamps (cells lower than all 8 neighbors
/// by > 0.25 m) where stagnant ponded floodwater naturally traps.
fn detect_real_depressions(elevations: &[Option<f64>], cols: usize, rows: usize) -> Vec<bool> {
    let mut mask = vec![false; cols * rows];
    let dirs: [(isize, isize); 8] = [
        (-1, 0),
        (1, 0),
        (0, -1),
        (0, 1),
        (-1, -1),
        (-1, 1),
        (1, -1),
        (1, 1),
    ];
    for r in 1..rows.saturating_sub(1) {
        for c in 1..cols.saturating_sub(1) {
            let idx = r * cols + c;
            let Some(z) = elevations[idx] else { continue };
            let mut min_nb = f64::INFINITY;
            for (dr, dc) in dirs {
                let nidx = ((r as isize + dr) as usize) * cols + ((c as isize + dc) as usize);
                if let Some(nz) = elevations[nidx]
                    && nz < min_nb
                {
                    min_nb = nz;
                }
            }
            if min_nb.is_finite() && (min_nb - z) > 0.25 {
                mask[idx] = true;
            }
        }
    }
    mask
}

/// Bilinearly interpolates a coarse `nx x ny` spatial grid (row-major south-to-north) at `(u, v) in [0, 1]^2`.
fn bilinear_sample_grid(values: &[f64], nx: usize, ny: usize, u: f64, v: f64) -> f64 {
    if values.len() != nx * ny || nx == 0 || ny == 0 {
        return 0.0;
    }
    let fx = u.clamp(0.0, 1.0) * ((nx - 1) as f64);
    let fy = v.clamp(0.0, 1.0) * ((ny - 1) as f64);
    let x0 = (fx.floor() as usize).min(nx - 1);
    let y0 = (fy.floor() as usize).min(ny - 1);
    let x1 = (x0 + 1).min(nx - 1);
    let y1 = (y0 + 1).min(ny - 1);
    let tx = fx - (x0 as f64);
    let ty = fy - (y0 as f64);
    let v00 = values[y0 * nx + x0];
    let v10 = values[y0 * nx + x1];
    let v01 = values[y1 * nx + x0];
    let v11 = values[y1 * nx + x1];
    (v00 * (1.0 - tx) + v10 * tx) * (1.0 - ty) + (v01 * (1.0 - tx) + v11 * tx) * ty
}
