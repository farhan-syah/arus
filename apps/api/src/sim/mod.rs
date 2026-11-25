//! Bounded interactive `hydr 0.2.0` simulation execution, binary frame cache, async basin jobs, and exports.

mod dem;
mod export;

pub use export::{export_envelope_csv, export_max_depth_ascii};

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use arus_domain::{GridSpec, RunClassification, SpatialReference};
use arus_evaluation::{BudgetAssessment, assess_water_budget};
use arus_geo::{EarthworkSummary, TerrainEdit, TerrainGrid, parse_esri_ascii_bounded};
use arus_hydraulics::{
    BoundaryAxisSpec, BoundaryFaceSpec, BoundaryForcingSpec, CoastalForcingSpec, EnvelopeSummary,
    FrameCaptureConfig, FrameSeries, GridMetadata, LandSpec, ProbeSpec, RainIntervalSpec,
    SurfacePhysicsOverrides, SurfaceRunSpec,
};
use arus_jobs::{CancellationToken, StepBudget};
use arus_model_api::WaterBudget;
use dem::{
    bilinear_sample_grid, build_channel_prestate, build_synthetic_valley_terrain,
    burn_river_channels, compute_d8_flow_accum, compute_wgs84_bounds, condition_hydrological_dem,
    detect_real_depressions,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
pub const SIM_TIMEOUT_SECS: u64 = 45;
/// Maximum interactive runs kept in the bounded in-memory LRU cache.
pub const MAX_CACHED_RUNS: usize = 16;
/// Maximum 2D grid cells permitted in an async high-resolution basin job (`512x512`).
pub const MAX_JOB_CELLS: usize = 262_144;
/// Maximum solver steps permitted in an async high-resolution basin job.
pub const MAX_JOB_STEPS: usize = 200_000;
/// Maximum simulated duration (`s`) permitted in an async high-resolution basin job (12 h).
pub const MAX_JOB_DURATION_S: f64 = 43_200.0;
/// Maximum wall-clock duration (`s`) per async high-resolution basin job.
pub const JOB_TIMEOUT_SECS: u64 = 300;
/// Maximum async job records retained in memory.
pub const MAX_TRACKED_JOBS: usize = 64;

/// Execution bounds separating fast synchronous Studio runs from async high-resolution basin jobs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExecutionLimits {
    pub max_cells: usize,
    pub max_steps: usize,
    pub max_duration_s: f64,
    pub timeout_secs: u64,
}

impl ExecutionLimits {
    pub const fn interactive() -> Self {
        Self {
            max_cells: MAX_INTERACTIVE_CELLS,
            max_steps: MAX_INTERACTIVE_STEPS,
            max_duration_s: 3600.0,
            timeout_secs: SIM_TIMEOUT_SECS,
        }
    }

    pub const fn async_job() -> Self {
        Self {
            max_cells: MAX_JOB_CELLS,
            max_steps: MAX_JOB_STEPS,
            max_duration_s: MAX_JOB_DURATION_S,
            timeout_secs: JOB_TIMEOUT_SECS,
        }
    }
}

/// Lifecycle state of an asynchronous high-resolution basin simulation job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Cancelled,
    Failed,
}

/// Public progress and outcome snapshot for an async high-resolution simulation job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationJobRecord {
    pub job_id: String,
    pub run_id: String,
    pub status: JobStatus,
    pub progress_pct: f32,
    pub accepted_time_s: f64,
    pub target_duration_s: f64,
    pub steps_taken: usize,
    pub cells: usize,
    pub elapsed_s: f64,
    pub error: Option<String>,
    pub summary: Option<SimulationSummaryResponse>,
}

struct ActiveJobEntry {
    record: SimulationJobRecord,
    created_at: Instant,
    token: CancellationToken,
}

/// Bounded in-memory queue and progress tracker for asynchronous high-resolution basin jobs.
pub struct JobStore {
    next_id: AtomicU64,
    semaphore: Arc<Semaphore>,
    jobs: RwLock<BTreeMap<String, ActiveJobEntry>>,
    order: RwLock<VecDeque<String>>,
}

impl Default for JobStore {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_SIMS)),
            jobs: RwLock::new(BTreeMap::new()),
            order: RwLock::new(VecDeque::new()),
        }
    }
}

impl std::fmt::Debug for JobStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobStore")
            .field("next_id", &self.next_id.load(Ordering::Relaxed))
            .finish()
    }
}

impl JobStore {
    /// Validates and enqueues a high-resolution async basin simulation job.
    pub fn submit(
        self: &Arc<Self>,
        interactive: Arc<InteractiveStore>,
        request: CreateSimulationRequest,
    ) -> Result<SimulationJobRecord, (u16, String)> {
        let limits = ExecutionLimits::async_job();
        validate_request(&request, limits)?;

        let seq = self.next_id.fetch_add(1, Ordering::Relaxed);
        let job_id = format!("job-{seq:04}");
        let run_id = format!("hires-{seq:04}");
        let cells = request.cols.saturating_mul(request.rows);
        let token = CancellationToken::new();
        let initial = SimulationJobRecord {
            job_id: job_id.clone(),
            run_id: run_id.clone(),
            status: JobStatus::Queued,
            progress_pct: 0.0,
            accepted_time_s: 0.0,
            target_duration_s: request.duration_s,
            steps_taken: 0,
            cells,
            elapsed_s: 0.0,
            error: None,
            summary: None,
        };

        if let (Ok(mut map), Ok(mut order)) = (self.jobs.write(), self.order.write()) {
            if order.len() >= MAX_TRACKED_JOBS
                && let Some(oldest) = order.pop_front()
            {
                map.remove(&oldest);
            }
            order.push_back(job_id.clone());
            map.insert(
                job_id.clone(),
                ActiveJobEntry {
                    record: initial.clone(),
                    created_at: Instant::now(),
                    token: token.clone(),
                },
            );
        }

        let store = Arc::clone(self);
        let job_id_bg = job_id.clone();
        let run_id_bg = run_id.clone();
        tokio::spawn(async move {
            let permit = match store.semaphore.clone().acquire_owned().await {
                Ok(p) => p,
                Err(_) => {
                    store.mark_failed(&job_id_bg, "job worker pool unavailable".to_owned());
                    return;
                }
            };
            if token.is_cancelled() {
                store.mark_cancelled(&job_id_bg);
                return;
            }
            store.mark_running(&job_id_bg);

            let store_progress = Arc::clone(&store);
            let job_id_progress = job_id_bg.clone();
            let token_bg = token.clone();
            let target_dur = request.duration_s;

            let join = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                execute_blocking(
                    run_id_bg,
                    &request,
                    &token_bg,
                    limits,
                    move |steps, accepted_s| {
                        store_progress.update_progress(
                            &job_id_progress,
                            steps,
                            accepted_s,
                            target_dur,
                        );
                    },
                )
            });

            let result = tokio::time::timeout(Duration::from_secs(limits.timeout_secs), join).await;
            match result {
                Ok(Ok(Ok(sim))) => {
                    if token.is_cancelled() {
                        store.mark_cancelled(&job_id_bg);
                    } else {
                        let summary = sim.summary.clone();
                        let rid = sim.summary.run_id.clone();
                        interactive.insert(rid, sim);
                        store.mark_completed(&job_id_bg, summary);
                    }
                }
                Ok(Ok(Err((_, msg)))) => {
                    if token.is_cancelled() {
                        store.mark_cancelled(&job_id_bg);
                    } else {
                        store.mark_failed(&job_id_bg, msg);
                    }
                }
                Ok(Err(join_err)) => {
                    store.mark_failed(&job_id_bg, format!("worker join error: {join_err}"));
                }
                Err(_) => {
                    token.cancel();
                    store.mark_failed(
                        &job_id_bg,
                        format!("job exceeded {}s wall-clock limit", limits.timeout_secs),
                    );
                }
            }
        });

        Ok(initial)
    }

    /// Lists tracked jobs in submission order.
    pub fn list(&self) -> Vec<SimulationJobRecord> {
        let Ok(map) = self.jobs.read() else {
            return Vec::new();
        };
        let Ok(order) = self.order.read() else {
            return Vec::new();
        };
        order
            .iter()
            .filter_map(|id| map.get(id).map(snapshot_entry))
            .collect()
    }

    /// Retrieves one job's live status snapshot.
    pub fn get(&self, job_id: &str) -> Option<SimulationJobRecord> {
        self.jobs
            .read()
            .ok()
            .and_then(|map| map.get(job_id).map(snapshot_entry))
    }

    /// Requests cooperative cancellation of a queued or running job.
    pub fn cancel(&self, job_id: &str) -> Option<SimulationJobRecord> {
        let mut map = self.jobs.write().ok()?;
        let entry = map.get_mut(job_id)?;
        entry.token.cancel();
        if matches!(entry.record.status, JobStatus::Queued | JobStatus::Running) {
            entry.record.status = JobStatus::Cancelled;
            entry.record.elapsed_s = entry.created_at.elapsed().as_secs_f64();
        }
        Some(entry.record.clone())
    }

    fn mark_running(&self, job_id: &str) {
        if let Ok(mut map) = self.jobs.write()
            && let Some(entry) = map.get_mut(job_id)
            && entry.record.status == JobStatus::Queued
        {
            entry.record.status = JobStatus::Running;
            entry.record.elapsed_s = entry.created_at.elapsed().as_secs_f64();
        }
    }

    fn update_progress(&self, job_id: &str, steps: usize, accepted_s: f64, target_dur: f64) {
        if let Ok(mut map) = self.jobs.write()
            && let Some(entry) = map.get_mut(job_id)
        {
            entry.record.steps_taken = steps;
            entry.record.accepted_time_s = accepted_s;
            let pct = if target_dur > 0.0 {
                ((accepted_s / target_dur) * 100.0).clamp(0.0, 99.9) as f32
            } else {
                0.0
            };
            entry.record.progress_pct = pct;
            entry.record.elapsed_s = entry.created_at.elapsed().as_secs_f64();
        }
    }

    fn mark_completed(&self, job_id: &str, summary: SimulationSummaryResponse) {
        if let Ok(mut map) = self.jobs.write()
            && let Some(entry) = map.get_mut(job_id)
        {
            entry.record.status = JobStatus::Completed;
            entry.record.progress_pct = 100.0;
            entry.record.accepted_time_s = summary.accepted_time_s;
            entry.record.steps_taken = summary.steps_taken;
            entry.record.elapsed_s = entry.created_at.elapsed().as_secs_f64();
            entry.record.summary = Some(summary);
        }
    }

    fn mark_cancelled(&self, job_id: &str) {
        if let Ok(mut map) = self.jobs.write()
            && let Some(entry) = map.get_mut(job_id)
        {
            entry.record.status = JobStatus::Cancelled;
            entry.record.elapsed_s = entry.created_at.elapsed().as_secs_f64();
        }
    }

    fn mark_failed(&self, job_id: &str, error: String) {
        if let Ok(mut map) = self.jobs.write()
            && let Some(entry) = map.get_mut(job_id)
        {
            entry.record.status = JobStatus::Failed;
            entry.record.error = Some(error);
            entry.record.elapsed_s = entry.created_at.elapsed().as_secs_f64();
        }
    }
}

fn snapshot_entry(entry: &ActiveJobEntry) -> SimulationJobRecord {
    let mut rec = entry.record.clone();
    if matches!(rec.status, JobStatus::Queued | JobStatus::Running) {
        rec.elapsed_s = entry.created_at.elapsed().as_secs_f64();
    }
    rec
}

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

/// Coastal tide-lock forcing mode for interactive and stress-test simulations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TideMode {
    #[default]
    Live,
    SpringHigh,
    Off,
}

const fn default_true() -> bool {
    true
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

/// Optional coarse spatial NWP forcing grid (`nx` lon x `ny` lat points, row-major south-to-north)
/// bilinearly interpolated onto the 2D hydraulic mesh per interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialRainGrid {
    pub nx: usize,
    pub ny: usize,
    pub hourly_mm: Vec<Vec<f64>>,
    /// Optional hourly FAO-56 Penman-Monteith reference evapotranspiration (mm/hr per point),
    /// derived from live temperature, relative humidity, solar radiation/cloud cover, and wind.
    #[serde(default)]
    pub et0_mm: Vec<Vec<f64>>,
    /// Optional initial volumetric soil moisture (m³/m³, 0..0.55) per grid point for dynamic
    /// saturation-excess infiltration.
    #[serde(default)]
    pub soil_moisture_init: Vec<f64>,
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
    /// Optional baseline terrain edits for paired compare (e.g. intact bund vs breached bund).
    #[serde(default)]
    pub baseline_interventions: Vec<InterventionCellDelta>,
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
    /// When `true`, forces a completely dry initial basin (`0.0 m`) ignoring antecedent rain and gauges.
    #[serde(default)]
    pub start_dry: bool,
    /// Enables FAO-56 Penman-Monteith / diurnal evaporation losses (`false` = worst-case no evaporation).
    #[serde(default = "default_true")]
    pub enable_evaporation: bool,
    /// Enables subsurface / storm-drainage loss rate (`false` = blocked/surcharged drainage).
    #[serde(default = "default_true")]
    pub enable_drainage: bool,
    /// Coastal tide-lock mode (`live` uses `hourly_tide_m`, `spring-high` applies `+2.2 m`, `off` disables).
    #[serde(default)]
    pub tide_mode: TideMode,
    /// Baseline engineered storm-drain capacity score in `0.0..=1.0`
    /// (`0.0` = clogged/none, `0.5` = average urban/rural drains, `1.0` = high-capacity trunk drains).
    #[serde(default = "default_storm_drain_score")]
    pub storm_drain_score: f64,
    /// Optional per-cell engineered storm-drain capacity grid (`cols * rows`, each `0.0..=1.0`)
    /// replacing `storm_drain_score` when real local drainage network survey or GIS data is supplied.
    #[serde(default)]
    pub storm_drain_grid: Option<Vec<f64>>,
}

const fn default_storm_drain_score() -> f64 {
    0.5
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
    hash_to_run_id: BTreeMap<[u8; 32], String>,
    run_id_to_hash: BTreeMap<String, [u8; 32]>,
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
        self.insert_with_hash(run_id, sim, None);
    }

    fn insert_with_hash(
        &self,
        run_id: String,
        sim: CachedSimulation,
        content_hash: Option<[u8; 32]>,
    ) {
        if let Ok(mut guard) = self.cache.write() {
            if guard.entries.contains_key(&run_id) {
                guard.order.retain(|id| id != &run_id);
                if let Some(old_hash) = guard.run_id_to_hash.remove(&run_id) {
                    guard.hash_to_run_id.remove(&old_hash);
                }
            } else if guard.order.len() >= MAX_CACHED_RUNS
                && let Some(oldest) = guard.order.pop_front()
            {
                guard.entries.remove(&oldest);
                if let Some(old_hash) = guard.run_id_to_hash.remove(&oldest) {
                    guard.hash_to_run_id.remove(&old_hash);
                }
            }
            guard.order.push_back(run_id.clone());
            guard.entries.insert(run_id.clone(), Arc::new(sim));
            if let Some(hash) = content_hash {
                guard.hash_to_run_id.insert(hash, run_id.clone());
                guard.run_id_to_hash.insert(run_id, hash);
            }
        }
    }

    fn get_by_hash(&self, hash: &[u8; 32]) -> Option<Arc<CachedSimulation>> {
        let guard = self.cache.read().ok()?;
        let run_id = guard.hash_to_run_id.get(hash)?;
        guard.entries.get(run_id).cloned()
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
        let limits = ExecutionLimits::interactive();
        validate_request(&request, limits)?;
        let content_hash = if fixed_id.is_none() && request.ascii_dem.is_none() {
            let h = compute_request_hash(&request);
            if let Some(cached) = self.get_by_hash(&h) {
                return Ok(cached);
            }
            Some(h)
        } else {
            None
        };
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
            execute_blocking(run_id_clone, &request, &token, limits, |_, _| {})
        });

        let result = tokio::time::timeout(Duration::from_secs(limits.timeout_secs), join).await;
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

        self.insert_with_hash(run_id.clone(), sim, content_hash);
        self.get(&run_id)
            .ok_or_else(|| (500, "failed to read newly cached simulation".to_owned()))
    }
}

fn validate_request(
    req: &CreateSimulationRequest,
    limits: ExecutionLimits,
) -> Result<(), (u16, String)> {
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
    if !req.duration_s.is_finite() || !(10.0..=limits.max_duration_s).contains(&req.duration_s) {
        return Err((
            422,
            format!(
                "duration_s must be finite in [10, {}]",
                limits.max_duration_s
            ),
        ));
    }
    if !req.manning_n.is_finite() || !(0.005..=0.5).contains(&req.manning_n) {
        return Err((422, "manning_n must be finite in [0.005, 0.5]".to_owned()));
    }
    if !req.threshold_m.is_finite() || req.threshold_m <= 0.0 {
        return Err((422, "threshold_m must be finite positive".to_owned()));
    }
    if req.ascii_dem.is_none() {
        let cells = req.cols.saturating_mul(req.rows);
        if req.cols < 4 || req.rows < 4 || cells > limits.max_cells {
            return Err((
                422,
                format!(
                    "grid must be at least 4x4 and <= {} cells",
                    limits.max_cells
                ),
            ));
        }
        if !req.dx_m.is_finite() || !(1.0..=2500.0).contains(&req.dx_m) {
            return Err((422, "dx_m must be finite in [1, 2500] metres".to_owned()));
        }
    }
    if !req.drainage_mm_per_hr.is_finite() || req.drainage_mm_per_hr < 0.0 {
        return Err((
            422,
            "drainage_mm_per_hr must be finite nonnegative".to_owned(),
        ));
    }
    if !req.storm_drain_score.is_finite() || !(0.0..=1.0).contains(&req.storm_drain_score) {
        return Err((
            422,
            "storm_drain_score must be finite in [0.0, 1.0]".to_owned(),
        ));
    }
    if let Some(dg) = &req.storm_drain_grid
        && dg.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err((
            422,
            "storm_drain_grid values must be finite in [0.0, 1.0]".to_owned(),
        ));
    }
    if !req.evaporation_mm_per_day.is_finite() || req.evaporation_mm_per_day < 0.0 {
        return Err((
            422,
            "evaporation_mm_per_day must be finite nonnegative".to_owned(),
        ));
    }
    if !req.wind_x_m_per_s.is_finite() || !req.wind_y_m_per_s.is_finite() {
        return Err((422, "wind components must be finite".to_owned()));
    }
    if let LandChoice::ModifiedGreenAmpt {
        suction_m,
        conductivity_m_per_s,
        maximum_deficit,
    } = req.land
        && (!suction_m.is_finite()
            || suction_m <= 0.0
            || !conductivity_m_per_s.is_finite()
            || conductivity_m_per_s <= 0.0
            || !maximum_deficit.is_finite()
            || maximum_deficit <= 0.0)
    {
        return Err((
            422,
            "ModifiedGreenAmpt parameters must be finite positive".to_owned(),
        ));
    }
    if let BoundaryChoice::OpenStage { elevation_m } = req.boundary
        && !elevation_m.is_finite()
    {
        return Err((422, "OpenStage elevation_m must be finite".to_owned()));
    }
    if req.interventions.iter().any(|iv| !iv.delta_m.is_finite()) {
        return Err((422, "interventions[*].delta_m must be finite".to_owned()));
    }
    if let Some(elevs) = &req.terrain_elevations_m
        && elevs.iter().flatten().any(|z| !z.is_finite())
    {
        return Err((
            422,
            "terrain_elevations_m entries must be finite".to_owned(),
        ));
    }
    if req
        .hourly_rain_mm
        .iter()
        .any(|v| !v.is_finite() || *v < 0.0)
    {
        return Err((
            422,
            "hourly_rain_mm values must be finite nonnegative".to_owned(),
        ));
    }
    if req.hourly_tide_m.iter().any(|v| !v.is_finite()) {
        return Err((422, "hourly_tide_m values must be finite".to_owned()));
    }
    if req.observed_gauges.iter().any(|g| {
        !g.lat.is_finite() || !g.lon.is_finite() || !g.depth_m.is_finite() || g.depth_m < 0.0
    }) {
        return Err((
            422,
            "observed_gauges fields must be finite with nonnegative depth".to_owned(),
        ));
    }
    if let Some(sg) = &req.spatial_rain_grid
        && (sg
            .hourly_mm
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || *v < 0.0)
            || sg
                .et0_mm
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || *v < 0.0)
            || sg
                .soil_moisture_init
                .iter()
                .any(|v| !v.is_finite() || *v < 0.0))
    {
        return Err((
            422,
            "spatial_rain_grid values must be finite nonnegative".to_owned(),
        ));
    }
    let n_hours = req
        .spatial_rain_grid
        .as_ref()
        .filter(|g| {
            g.nx >= 2 && g.ny >= 2 && g.hourly_mm.len() == g.nx * g.ny && !g.hourly_mm[0].is_empty()
        })
        .map_or(req.hourly_rain_mm.len(), |g| g.hourly_mm[0].len());
    if n_hours > 0 {
        let compression = (n_hours as f64) * 3600.0 / req.duration_s;
        if compression > 240.0 + 1e-6 {
            return Err((
                422,
                format!(
                    "time compression {compression:.1}x exceeds 240x limit for {n_hours}h over {}s; increase duration_s or reduce hours",
                    req.duration_s
                ),
            ));
        }
    }
    Ok(())
}

fn hash_f64(h: &mut Sha256, v: f64) {
    let norm = if v == 0.0 { 0.0_f64 } else { v };
    h.update(norm.to_le_bytes());
}

fn hash_usize(h: &mut Sha256, v: usize) {
    h.update((v as u64).to_le_bytes());
}

fn hash_str(h: &mut Sha256, s: &str) {
    hash_usize(h, s.len());
    h.update(s.as_bytes());
}

fn compute_request_hash(req: &CreateSimulationRequest) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"arus-sim-req-v2");
    hash_f64(&mut h, req.center_lon);
    hash_f64(&mut h, req.center_lat);
    hash_usize(&mut h, req.cols);
    hash_usize(&mut h, req.rows);
    hash_f64(&mut h, req.dx_m);
    hash_f64(&mut h, req.rainfall_mm_per_hr);
    h.update([match req.hyetograph {
        HyetographPreset::Uniform => 0u8,
        HyetographPreset::PeakStorm => 1u8,
    }]);
    hash_f64(&mut h, req.duration_s);
    hash_f64(&mut h, req.manning_n);
    match req.land {
        LandChoice::Impervious => h.update([0u8]),
        LandChoice::ModifiedGreenAmpt {
            suction_m,
            conductivity_m_per_s,
            maximum_deficit,
        } => {
            h.update([1u8]);
            hash_f64(&mut h, suction_m);
            hash_f64(&mut h, conductivity_m_per_s);
            hash_f64(&mut h, maximum_deficit);
        }
    }
    match req.boundary {
        BoundaryChoice::Closed => h.update([0u8]),
        BoundaryChoice::Transmissive => h.update([1u8]),
        BoundaryChoice::OpenStage { elevation_m } => {
            h.update([2u8]);
            hash_f64(&mut h, elevation_m);
        }
    }
    hash_f64(&mut h, req.threshold_m);
    h.update([match req.precision {
        PrecisionChoice::F64 => 0u8,
        PrecisionChoice::F32 => 1u8,
    }]);
    hash_usize(&mut h, req.interventions.len());
    for iv in &req.interventions {
        hash_usize(&mut h, iv.col);
        hash_usize(&mut h, iv.row);
        hash_f64(&mut h, iv.delta_m);
    }
    match &req.terrain_elevations_m {
        None => h.update([0u8]),
        Some(elevs) => {
            h.update([1u8]);
            hash_usize(&mut h, elevs.len());
            for z in elevs {
                match z {
                    None => h.update([0u8]),
                    Some(v) => {
                        h.update([1u8]);
                        hash_f64(&mut h, *v);
                    }
                }
            }
        }
    }
    match &req.terrain_provenance {
        None => h.update([0u8]),
        Some(p) => {
            h.update([1u8]);
            hash_str(&mut h, p);
        }
    }
    hash_f64(&mut h, req.antecedent_rain_mm);
    hash_usize(&mut h, req.hourly_rain_mm.len());
    for &v in &req.hourly_rain_mm {
        hash_f64(&mut h, v);
    }
    match &req.spatial_rain_grid {
        None => h.update([0u8]),
        Some(sg) => {
            h.update([1u8]);
            hash_usize(&mut h, sg.nx);
            hash_usize(&mut h, sg.ny);
            hash_usize(&mut h, sg.hourly_mm.len());
            for row in &sg.hourly_mm {
                hash_usize(&mut h, row.len());
                for &v in row {
                    hash_f64(&mut h, v);
                }
            }
            hash_usize(&mut h, sg.et0_mm.len());
            for row in &sg.et0_mm {
                hash_usize(&mut h, row.len());
                for &v in row {
                    hash_f64(&mut h, v);
                }
            }
            hash_usize(&mut h, sg.soil_moisture_init.len());
            for &v in &sg.soil_moisture_init {
                hash_f64(&mut h, v);
            }
        }
    }
    hash_usize(&mut h, req.hourly_tide_m.len());
    for &v in &req.hourly_tide_m {
        hash_f64(&mut h, v);
    }
    hash_f64(&mut h, req.drainage_mm_per_hr);
    hash_f64(&mut h, req.storm_drain_score);
    match &req.storm_drain_grid {
        None => h.update([0u8]),
        Some(dg) => {
            h.update([1u8]);
            hash_usize(&mut h, dg.len());
            for &v in dg {
                hash_f64(&mut h, v);
            }
        }
    }
    hash_f64(&mut h, req.evaporation_mm_per_day);
    hash_usize(&mut h, req.observed_gauges.len());
    for g in &req.observed_gauges {
        hash_f64(&mut h, g.lat);
        hash_f64(&mut h, g.lon);
        hash_f64(&mut h, g.depth_m);
        hash_usize(&mut h, g.radius_cells);
    }
    hash_f64(&mut h, req.wind_x_m_per_s);
    hash_f64(&mut h, req.wind_y_m_per_s);
    h.update([
        u8::from(req.start_dry),
        u8::from(req.enable_evaporation),
        u8::from(req.enable_drainage),
        match req.tide_mode {
            TideMode::Live => 0u8,
            TideMode::SpringHigh => 1u8,
            TideMode::Off => 2u8,
        },
    ]);
    h.finalize().into()
}

fn execute_blocking<F>(
    run_id: String,
    req: &CreateSimulationRequest,
    token: &CancellationToken,
    limits: ExecutionLimits,
    mut on_progress: F,
) -> Result<CachedSimulation, (u16, String)>
where
    F: FnMut(usize, f64),
{
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
                limits.max_cells,
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
                    burn_river_channels(&mut conditioned, req.cols, req.rows, req.dx_m);
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
    let has_prestate =
        !req.start_dry && (req.antecedent_rain_mm > 0.0 || !req.observed_gauges.is_empty());
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
    let base_drain_m_per_s = if req.enable_drainage {
        req.drainage_mm_per_hr.max(0.0) / 3_600_000.0
    } else {
        0.0
    };
    let base_evap_mean_m_per_s = if req.enable_evaporation {
        req.evaporation_mm_per_day.max(0.0) / 86_400_000.0
    } else {
        0.0
    };

    let mut forcing = Vec::new();
    let mut interval_losses: Vec<(f64, f64)> = Vec::new();
    let mut interval_rain_grids: Vec<Vec<f64>> = Vec::new();
    let mut interval_loss_grids: Vec<Vec<f64>> = Vec::new();
    let mut manning_per_cell: Option<Vec<f64>> = None;
    let mut use_spatial_grids = false;

    let has_spatial = req.spatial_rain_grid.as_ref().is_some_and(|g| {
        g.nx >= 2 && g.ny >= 2 && g.hourly_mm.len() == g.nx * g.ny && !g.hourly_mm[0].is_empty()
    });
    let has_spatial_et0 = req.enable_evaporation
        && req.spatial_rain_grid.as_ref().is_some_and(|g| {
            has_spatial && g.et0_mm.len() == g.nx * g.ny && !g.et0_mm[0].is_empty()
        });
    let has_spatial_sm = req
        .spatial_rain_grid
        .as_ref()
        .is_some_and(|g| has_spatial && g.soil_moisture_init.len() == g.nx * g.ny);

    if has_spatial || !req.hourly_rain_mm.is_empty() {
        // Full physical 2D forcing:
        // (1) Spatial NWP rainfall + D8 upstream channel routing + perimeter river entry inflow
        // (2) Live FAO-56 Penman-Monteith ET0 (temperature, humidity, solar/cloud, wind; suppressed during active rain)
        // (3) Dynamic per-cell soil moisture saturation tracking -> infiltration capacity
        // (4) Spatially varying Manning n (river channel 0.032, lowland floodplain 0.060, upland forest 0.110)
        // (5) Selective depression preservation + coastal tide-lock for stagnant ponded floodwater
        let n_hours = if let Some(sg) = &req.spatial_rain_grid
            && has_spatial
        {
            sg.hourly_mm[0].len()
        } else {
            req.hourly_rain_mm.len()
        };
        let n_bins = n_hours.clamp(1, 24);
        let bin_dur_s = req.duration_s / (n_bins as f64);
        let bin_hours = (n_hours as f64) / (n_bins as f64);
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

        let mut n_cells = vec![req.manning_n; cols * rows];
        let mut soil_sm = vec![0.42f64; cols * rows];
        for r in 0..rows {
            for c in 0..cols {
                let idx = r * cols + c;
                let Some(z) = terrain.cells()[idx] else {
                    continue;
                };
                let a = accum[idx];
                n_cells[idx] = if a >= channel_accum_min {
                    0.032
                } else if z <= low_z_cutoff {
                    0.060
                } else {
                    0.110
                };
                if let Some(sg) = &req.spatial_rain_grid
                    && has_spatial_sm
                {
                    let sampled_sm = bilinear_sample_grid(
                        &sg.soil_moisture_init,
                        sg.nx,
                        sg.ny,
                        (c as f64 + 0.5) / (cols as f64),
                        (r as f64 + 0.5) / (rows as f64),
                    );
                    soil_sm[idx] = if sampled_sm > 0.05 {
                        sampled_sm.clamp(0.12, 0.52)
                    } else {
                        0.45
                    };
                }
            }
        }
        manning_per_cell = Some(n_cells);

        for b in 0..n_bins {
            let h0 = (b * n_hours) / n_bins;
            let h1 = (((b + 1) * n_hours) / n_bins).max(h0 + 1).min(n_hours);
            let bin_len = (h1 - h0).max(1) as f64;

            let mid_h = ((h0 + h1) / 2 + 8) % 24;
            let solar_weight = if (7..=18).contains(&mid_h) {
                let phase = ((mid_h as f64 - 7.0) / 11.0) * std::f64::consts::PI;
                phase.sin().max(0.0) * 2.2
            } else {
                0.05
            };
            let fallback_evap_m_s = base_evap_mean_m_per_s * solar_weight * time_compression;
            let eff_drain_m_s = base_drain_m_per_s * time_compression;

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

            for r in 0..rows {
                for c in 0..cols {
                    let idx = r * cols + c;
                    let Some(z) = terrain.cells()[idx] else {
                        continue;
                    };
                    let u = (c as f64 + 0.5) / (cols as f64);
                    let v = (r as f64 + 0.5) / (rows as f64);
                    let local_mm_hr = if let Some(sg) = &req.spatial_rain_grid
                        && has_spatial
                    {
                        bilinear_sample_grid(&bin_pt_means, sg.nx, sg.ny, u, v)
                    } else {
                        fallback_mean_mm
                    };
                    let a = accum[idx];
                    let is_perim_inlet = (r == 0 || r + 1 == rows || c == 0 || c + 1 == cols)
                        && z > low_z_cutoff
                        && a >= channel_accum_min;
                    let routing_mult = if is_perim_inlet { 1.25 } else { 1.0 };
                    let coastal_surge_m_s = if z <= 3.5 && tide_lock > 0.0 {
                        (0.45 * tide_lock / 3_600_000.0) * time_compression
                    } else {
                        0.0
                    };
                    rain_grid[idx] = (local_mm_hr / 3_600_000.0) * time_compression * routing_mult
                        + coastal_surge_m_s;

                    // Live FAO-56 Penman-Monteith ET0 (suppressed during active rainfall > 0.2 mm/hr)
                    let local_et0_m_s = if let Some(sg) = &req.spatial_rain_grid
                        && has_spatial_et0
                    {
                        let et0_mm_hr = bilinear_sample_grid(&bin_et0_means, sg.nx, sg.ny, u, v);
                        let cloud_rain_suppress = if local_mm_hr > 0.2 { 0.12 } else { 1.0 };
                        (et0_mm_hr * cloud_rain_suppress / 3_600_000.0) * time_compression
                    } else {
                        let cloud_rain_suppress = if local_mm_hr > 0.2 { 0.15 } else { 1.0 };
                        fallback_evap_m_s * cloud_rain_suppress
                    };

                    // Natural soil infiltration + upland hillslope subsurface interflow
                    let sm = soil_sm[idx];
                    let sat_deficit = (1.0 - sm / 0.52).clamp(0.08, 1.0);
                    let dynamic_infil_factor = sat_deficit.powf(1.4);
                    let rain_gain = (local_mm_hr * bin_hours) / 320.0;
                    let dry_loss = if local_mm_hr <= 0.1 {
                        0.004 * bin_hours
                    } else {
                        0.0
                    };
                    soil_sm[idx] = (sm + rain_gain - dry_loss).clamp(0.12, 0.52);

                    let mut soil_factor = if depression_mask[idx] {
                        0.20 * dynamic_infil_factor
                    } else if z <= low_z_cutoff {
                        0.55 * dynamic_infil_factor
                    } else {
                        1.45 * dynamic_infil_factor
                    };
                    if z <= 3.5 && tide_lock > 0.0 {
                        soil_factor *= 1.0 - 0.75 * tide_lock;
                    }

                    // Local slope & relief for well-drained tropical upland hills/forests
                    let mut max_drop = 0.0f64;
                    for (dr, dc) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
                        let nr = r as isize + dr;
                        let nc = c as isize + dc;
                        if nr >= 0
                            && nc >= 0
                            && nr < rows as isize
                            && nc < cols as isize
                            && let Some(nz) = terrain.cells()[(nr as usize) * cols + (nc as usize)]
                        {
                            max_drop = max_drop.max(z - nz);
                        }
                    }
                    let slope = max_drop / terrain.grid().dx_m.max(1.0);
                    let relief_m = (z - low_z_cutoff).max(0.0);
                    let upland_mm_hr = if relief_m > 12.0 || slope > 0.015 {
                        ((relief_m - 12.0).max(0.0) / 50.0).clamp(0.0, 1.0) * 14.0
                            + ((slope - 0.015).max(0.0) / 0.04).clamp(0.0, 1.0) * 12.0
                    } else {
                        0.0
                    };
                    let upland_drain_m_s = (upland_mm_hr / 3_600_000.0) * time_compression;

                    // Engineered storm-drain capacity (independent of soil saturation;
                    // reduced only in closed depressions, active river channels, or coastal tide-lock)
                    let cell_sd = req
                        .storm_drain_grid
                        .as_ref()
                        .and_then(|g| g.get(idx).copied())
                        .unwrap_or(req.storm_drain_score)
                        .clamp(0.0, 1.0);
                    let mut sd_factor = cell_sd * 0.25;
                    if a >= channel_accum_min {
                        sd_factor *= 0.15;
                    } else if depression_mask[idx] {
                        sd_factor *= 0.25;
                    }
                    if z <= 3.5 && tide_lock > 0.0 {
                        sd_factor *= 1.0 - 0.80 * tide_lock;
                    }

                    loss_grid[idx] = local_et0_m_s
                        + eff_drain_m_s * (soil_factor + sd_factor)
                        + upland_drain_m_s;
                }
            }

            forcing.push(RainIntervalSpec {
                start_s: warmup_s + (b as f64) * bin_dur_s,
                end_s: warmup_s + ((b + 1) as f64) * bin_dur_s,
                uniform_rain_m_per_s: (fallback_mean_mm / 3_600_000.0) * time_compression,
                boundaries: boundary_forcings.clone(),
            });
            interval_losses.push((fallback_evap_m_s, eff_drain_m_s));
            interval_rain_grids.push(rain_grid);
            interval_loss_grids.push(loss_grid);
        }
    } else {
        let spring_tide_drain_scale = if req.tide_mode == TideMode::SpringHigh {
            0.25
        } else {
            1.0
        };
        let eff_drain = base_drain_m_per_s * spring_tide_drain_scale;
        match req.hyetograph {
            HyetographPreset::Uniform => {
                forcing.push(RainIntervalSpec {
                    start_s: warmup_s,
                    end_s: warmup_s + req.duration_s,
                    uniform_rain_m_per_s: rain_m_per_s,
                    boundaries: boundary_forcings,
                });
                interval_losses.push((base_evap_mean_m_per_s, eff_drain));
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
                interval_losses.push((base_evap_mean_m_per_s, eff_drain));
                forcing.push(RainIntervalSpec {
                    start_s: t1,
                    end_s: t2,
                    uniform_rain_m_per_s: rain_m_per_s * 1.6,
                    boundaries: boundary_forcings.clone(),
                });
                interval_losses.push((base_evap_mean_m_per_s * 0.5, eff_drain));
                forcing.push(RainIntervalSpec {
                    start_s: t2,
                    end_s: warmup_s + req.duration_s,
                    uniform_rain_m_per_s: 0.0,
                    boundaries: boundary_forcings,
                });
                interval_losses.push((base_evap_mean_m_per_s * 1.8, eff_drain));
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

    let initial_depth_m = if req.start_dry || terrain_provenance == "BrowserTerrariumScreening" {
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
        max_steps: limits.max_steps,
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
        .build_engine_with_overrides(SurfacePhysicsOverrides {
            manning_n_per_cell: manning_per_cell.as_deref(),
            initial_depths_m: per_cell_initial.as_deref(),
            interval_rain_grids: if use_spatial_grids {
                Some(&interval_rain_grids)
            } else {
                None
            },
            interval_loss_grids: if use_spatial_grids {
                Some(&interval_loss_grids)
            } else {
                None
            },
            interval_losses: Some(&interval_losses),
        })
        .map_err(|e| (422, e.to_string()))?;
    if warmup_s > 0.0 {
        engine
            .advance_to(warmup_s, StepBudget::new(limits.max_steps))
            .map_err(|e| (422, e.to_string()))?;
    }
    let target_frames = 48_usize.min(MAX_INTERACTIVE_FRAMES);
    let frame_interval_s = (req.duration_s / (target_frames.saturating_sub(1) as f64)).max(1.0);
    let (advance, mut series) = engine
        .advance_with_frames_and_progress(
            warmup_s + req.duration_s,
            StepBudget::new(limits.max_steps),
            token,
            FrameCaptureConfig {
                frame_interval_s,
                max_frames: MAX_INTERACTIVE_FRAMES,
                threshold_m: req.threshold_m,
                wgs84_bounds,
                terrain_provenance: terrain_provenance.clone(),
            },
            |steps, end_s| {
                on_progress(steps, (end_s - warmup_s).max(0.0));
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
