//! Bounded interactive `hydr 0.1.0` simulation execution, binary frame cache, paired diff, and exports.

use std::collections::{BTreeMap, VecDeque};
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
    /// Optional antecedent rainfall total (mm) spun up BEFORE Frame 0 so the simulation starts
    /// from an already-wet / actively flooding current basin state instead of a bone-dry bed.
    #[serde(default)]
    pub antecedent_rain_mm: f64,
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
                    sampled.clone(),
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
        BoundaryChoice::Transmissive | BoundaryChoice::OpenStage { .. } => (0..rows)
            .filter(|&r| terrain.cells()[r * cols + (cols - 1)].is_some())
            .map(|row| BoundaryFaceSpec {
                axis: BoundaryAxisSpec::X,
                column: cols,
                row,
            })
            .collect(),
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

    let warmup_s = if req.antecedent_rain_mm > 0.0 {
        600.0
    } else {
        0.0
    };
    let mut forcing = Vec::new();
    if warmup_s > 0.0 {
        // Pre-state / Nowcast warm-up: rain for 70% of warmup_s, then 30% hydraulic settling
        // so valleys, rivers, and floodplains reflect current wet/flooding state at t = 0.
        let rain_dur = warmup_s * 0.70;
        let ant_rate = (req.antecedent_rain_mm / 1000.0) / rain_dur;
        forcing.push(RainIntervalSpec {
            start_s: 0.0,
            end_s: rain_dur,
            uniform_rain_m_per_s: ant_rate,
            boundaries: boundary_forcings.clone(),
        });
        forcing.push(RainIntervalSpec {
            start_s: rain_dur,
            end_s: warmup_s,
            uniform_rain_m_per_s: 0.0,
            boundaries: boundary_forcings.clone(),
        });
    }
    match req.hyetograph {
        HyetographPreset::Uniform => forcing.push(RainIntervalSpec {
            start_s: warmup_s,
            end_s: warmup_s + req.duration_s,
            uniform_rain_m_per_s: rain_m_per_s,
            boundaries: boundary_forcings,
        }),
        HyetographPreset::PeakStorm => {
            let t1 = warmup_s + req.duration_s * 0.25;
            let t2 = warmup_s + req.duration_s * 0.55;
            forcing.push(RainIntervalSpec {
                start_s: warmup_s,
                end_s: t1,
                uniform_rain_m_per_s: rain_m_per_s * 0.65,
                boundaries: boundary_forcings.clone(),
            });
            forcing.push(RainIntervalSpec {
                start_s: t1,
                end_s: t2,
                uniform_rain_m_per_s: rain_m_per_s * 1.6,
                boundaries: boundary_forcings.clone(),
            });
            forcing.push(RainIntervalSpec {
                start_s: t2,
                end_s: warmup_s + req.duration_s,
                uniform_rain_m_per_s: 0.0,
                boundaries: boundary_forcings,
            });
        }
    };

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

    let mut engine = spec.build_engine().map_err(|e| (422, e.to_string()))?;
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
