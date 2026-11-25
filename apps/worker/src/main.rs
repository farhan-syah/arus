//! Native numerical worker for ARUS hydraulic executions.

mod video;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use arus_api_types::{
    API_SCHEMA_VERSION, ApiEpisodeSummary, ApiLocationOutlook, ApiLocationStatus,
};
use arus_assets::Archive;
use arus_domain::{GridSpec, Provenance, RunClassification, SpatialReference};
use arus_evaluation::{BudgetAssessment, assess_water_budget};
use arus_forecasting::{
    ApprovalStatus, EpisodePolicy, EpisodeTracker, FourStagePipelineReport, LocationEvolution,
    LocationSupportStatus, ModelForecastMember, OnsetStatus, PublicationCandidate,
    PublicationDecision, RecessionStatus, StationRegistry, Windowed2DOutcome,
    build_initial_basin_state, evaluate_compound_tide_lock, evaluate_publication,
    summarize_ensemble,
};
use arus_geo::TerrainGrid;
use arus_hydraulics::{
    BoundaryAxisSpec, BoundaryFaceSpec, BoundaryForcingSpec, CoastalForcingSpec, CoupledRunSpec,
    LandSpec, LocationProbe, NativePipeRunSpec, PoolStructureRunSpec, ProbeSpec, RainIntervalSpec,
    SurfaceEngine, SurfaceF32RunSpec, SurfaceRunSpec, SwmmDrainageSpec, all_component_capabilities,
    package_from_bytes, run_closed_basin_demo, run_native_worker_demo, surface_capabilities,
};
use arus_hydrology::{
    RunoffRunSpec, build_runoff_spec_from_hourly, derive_antecedent_state, detect_peak_window,
};
use arus_ingest::{parse_open_meteo_hourly, parse_open_meteo_marine};
use arus_jobs::{CancellationToken, StepBudget};
use arus_model_api::{AdvanceOutcome, PersistedCheckpoint};
use arus_scenarios::{ScenarioManifest, ScenarioRunSummary, compare_paired_runs};
use arus_storage::{PublishedPointer, RunRecord, RunStore};
use clap::{Parser, Subcommand};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Parser)]
#[command(name = "arus-worker", about = "Native numerical worker backed by hydr")]
struct Arguments {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print capability declarations from hydr.
    Capabilities {
        /// Print all 25 hydr component declarations instead of only the F64 surface solver.
        #[arg(long)]
        all: bool,
    },
    /// Run synthetic CPU F64 numerical demonstrations and check conservation and replay.
    Demo {
        /// Optional archive directory for persisting exact binary checkpoints.
        #[arg(long)]
        archive: Option<PathBuf>,
    },
    /// Run a synthetic CPU F64 surface demonstration and emit JSON API location outlooks
    /// and numerical budget assessment.
    Outlook {
        /// Stable run identifier used when persisting into an archive.
        #[arg(long, default_value = "demo-outlook-001")]
        run_id: String,
        /// Depth exceedance threshold in metres.
        #[arg(long, default_value_t = 0.003)]
        threshold_m: f64,
        /// Optional archive directory for persisting checkpoint, outlook JSON, and run receipt.
        #[arg(long)]
        archive: Option<PathBuf>,
    },
    /// Execute a JSON `SurfaceRunSpec` manifest and emit/persist location outlooks and budget assessment.
    Run {
        /// Path to the JSON `SurfaceRunSpec` manifest file.
        #[arg(long)]
        spec: PathBuf,
        /// Optional archive directory for persisting checkpoint, outlook JSON, and run receipt.
        #[arg(long)]
        archive: Option<PathBuf>,
    },
    /// Execute a paired baseline and intervention `SurfaceRunSpec` and emit a `PairedComparisonReport`.
    Compare {
        /// Path to the baseline JSON `SurfaceRunSpec`.
        #[arg(long)]
        baseline: PathBuf,
        /// Path to the intervention JSON `SurfaceRunSpec`.
        #[arg(long)]
        intervention: PathBuf,
    },
    /// Execute a JSON `RunoffRunSpec` subcatchment manifest and emit its water-balance JSON report.
    Runoff {
        /// Path to the JSON `RunoffRunSpec` manifest file.
        #[arg(long)]
        spec: PathBuf,
    },
    /// Execute a JSON `CoupledRunSpec` 2D surface + 1D channel manifest and emit its JSON report.
    Coupled {
        /// Path to the JSON `CoupledRunSpec` manifest file.
        #[arg(long)]
        spec: PathBuf,
    },
    /// Execute a JSON `PoolStructureRunSpec` level-pool + structures (weir/orifice/gate/pump) manifest.
    Pools {
        /// Path to the JSON `PoolStructureRunSpec` manifest file.
        #[arg(long)]
        spec: PathBuf,
    },
    /// Execute an opt-in `F32` closed-basin screening manifest via `SurfaceSolverF32`.
    SurfaceF32 {
        /// Path to the JSON `SurfaceF32RunSpec` manifest file.
        #[arg(long)]
        spec: PathBuf,
    },
    /// Translate an archived multi-model Open-Meteo hourly JSON blob into 1D hydrology runs,
    /// peak 3-6h window detection, and multi-model ensemble agreement summary.
    ForecastBridge {
        /// Path to an archived Open-Meteo single-model or multi-model hourly JSON blob.
        #[arg(long)]
        blob: PathBuf,
        /// Basin identifier attached to the generated runoff runs.
        #[arg(long, default_value = "catchment-1")]
        basin_id: String,
        /// Catchment contributing area in km².
        #[arg(long, default_value_t = 25.0)]
        area_km2: f64,
        /// Impervious fraction in [0.02, 0.95].
        #[arg(long, default_value_t = 0.15)]
        impervious_fraction: f64,
        /// Leading antecedent hours (`past_days * 24`) used to initialize Green-Ampt soil deficit.
        #[arg(long, default_value_t = 48)]
        past_hours: usize,
        /// Peak runoff window length in hours for 2D hydr refinement trigger (e.g. 6).
        #[arg(long, default_value_t = 6)]
        window_hours: usize,
    },
    /// Run the unified 4-stage flood forecasting pipeline:
    /// (1) Initial state + gauge headroom -> (2) 72-120h 1D hydrology ensemble ->
    /// (3) Coastal compound tide lock -> (4) Windowed 2D hydr 0.2.0 + onset/recession.
    Pipeline {
        /// Path to an archived Open-Meteo multi-model hourly JSON blob.
        #[arg(long)]
        meteo: PathBuf,
        /// Optional path to an archived Open-Meteo Marine hourly JSON blob (enables Stage 3 tide lock).
        #[arg(long)]
        marine: Option<PathBuf>,
        /// Optional path to a user-supplied JSON StationRegistry file.
        #[arg(long)]
        stations: Option<PathBuf>,
        /// Optional station ID inside `--stations`.
        #[arg(long)]
        station_id: Option<String>,
        /// Optional current live river stage in metres for headroom calculation.
        #[arg(long)]
        current_stage_m: Option<f64>,
        /// Basin identifier.
        #[arg(long, default_value = "catchment-1")]
        basin_id: String,
        /// Catchment area in km².
        #[arg(long, default_value_t = 25.0)]
        area_km2: f64,
        /// Impervious fraction in [0.02, 0.95].
        #[arg(long, default_value_t = 0.15)]
        impervious_fraction: f64,
        /// Leading antecedent hours (`past_days * 24`).
        #[arg(long, default_value_t = 48)]
        past_hours: usize,
        /// Peak window duration in hours for Stage 4 2D refinement (e.g. 3 or 6).
        #[arg(long, default_value_t = 3)]
        window_hours: usize,
        /// Sea-level height MSL threshold (m) flagging Stage 3 compound backwater risk.
        #[arg(long, default_value_t = 1.8)]
        tide_threshold_m: f64,
        /// Peak 24h rain threshold (mm) triggering Stage 4 2D simulation.
        #[arg(long, default_value_t = 25.0)]
        trigger_rain_mm: f64,
        /// Optional coastal wind forcing x-component (m/s) passed to Stage 4 hydr 0.2.0.
        #[arg(long, default_value_t = 0.0)]
        wind_m_per_s: f64,
    },
    /// Transcode a VP9 `.webm` storyboard recording to H.264 `.mp4` via `vidr` (WhatsApp/Telegram compatible).
    Transcode {
        /// Path to the input VP9 `.webm` file.
        #[arg(long)]
        input: PathBuf,
        /// Path to the output H.264 `.mp4` file.
        #[arg(long)]
        output: PathBuf,
        /// Optional leading seconds to trim from the start of the recording (e.g. browser boot/tile load).
        #[arg(long, default_value_t = 0.0)]
        trim_start_s: f64,
    },
    /// Execute a pure-Rust Preissmann-slot pressurized storm-drain network manifest (`hydr 0.2.0`).
    Pipes {
        /// Path to the JSON `NativePipeRunSpec` manifest file.
        #[arg(long)]
        spec: PathBuf,
    },
    /// Validate a pinned EPA SWMM v5.2.4 piped drainage manifest and check host worker readiness.
    DrainageCheck {
        /// Path to the JSON `SwmmDrainageSpec` manifest file.
        #[arg(long)]
        spec: PathBuf,
    },
    /// Verify and resume an archived run from its exact binary checkpoint blob.
    Resume {
        /// Archive directory containing the persisted run and checkpoint blob.
        #[arg(long)]
        archive: PathBuf,
        /// Existing run ID whose checkpoint blob will be resumed.
        #[arg(long)]
        from_run: String,
    },
    /// Evaluate an archived run against the publication gate and atomically advance the published pointer.
    Publish {
        /// Archive directory containing the persisted run.
        #[arg(long)]
        archive: PathBuf,
        /// Run ID to evaluate for publication.
        #[arg(long)]
        run_id: String,
        /// Cycle identifier.
        #[arg(long)]
        cycle_id: String,
        /// Strictly increasing cycle sequence number.
        #[arg(long)]
        cycle_sequence: u64,
        /// Valid-from Unix timestamp in seconds.
        #[arg(long)]
        valid_from_unix_s: i64,
        /// Expiry Unix timestamp in seconds.
        #[arg(long)]
        expires_at_unix_s: i64,
        /// Current Unix timestamp in seconds used for expiry checking.
        #[arg(long)]
        now_unix_s: i64,
        /// Declare scientific review approval.
        #[arg(long)]
        scientific_approved: bool,
        /// Declare owner publication approval.
        #[arg(long)]
        owner_approved: bool,
    },
}

#[derive(Debug, Serialize)]
struct OutlookEnvelope {
    run_id: String,
    classification: RunClassification,
    budget_assessment: BudgetAssessment,
    checkpoint: Option<PersistedCheckpoint>,
    outlooks: Vec<ApiLocationOutlook>,
}

fn main() -> ExitCode {
    let args = Arguments::parse();
    match execute(args.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("arus-worker error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn execute(command: Command) -> Result<(), String> {
    match command {
        Command::Capabilities { all } => {
            let json = if all {
                serde_json::to_string_pretty(&all_component_capabilities())
            } else {
                serde_json::to_string_pretty(&surface_capabilities())
            }
            .map_err(|error| error.to_string())?;
            println!("{json}");
            Ok(())
        }
        Command::Demo { archive } => {
            let closed = run_closed_basin_demo().map_err(|error| error.to_string())?;
            let opened = match archive {
                Some(path) => Some(Archive::open(&path).map_err(|error| error.to_string())?),
                None => None,
            };
            let native =
                run_native_worker_demo(opened.as_ref()).map_err(|error| error.to_string())?;
            println!(
                "classification: {}",
                native.execution.provenance.classification.id()
            );
            println!(
                "backend: {} ({})",
                native.capabilities.backend, native.capabilities.contract
            );
            println!(
                "closed_basin: t={} s residual={:.3e} m3 bound={:.3e} m3 checkpoint={} bytes",
                closed.accepted_time_s,
                closed.budget.residual_m3,
                closed.budget.residual_bound_m3,
                closed.checkpoint_bytes
            );
            println!(
                "native_worker: t={} s cancelled_at={} s steps={} boundary_bitwise={} splice_bitwise={} residual={:.3e} m3",
                native.execution.accepted_time_s,
                native.cancelled_at_s,
                native.observed_steps,
                native.boundary_bitwise_match,
                native.forecast_splice_bitwise_match,
                native.execution.budget.residual_m3
            );
            if let Some(checkpoint) = native.persisted_checkpoint {
                println!(
                    "checkpoint: schema={} sha256={} bytes={}",
                    checkpoint.header.schema, checkpoint.blob.sha256, checkpoint.blob.bytes
                );
            }
            Ok(())
        }
        Command::Outlook {
            run_id,
            threshold_m,
            archive,
        } => run_outlook_demo(run_id, threshold_m, archive),
        Command::Run { spec, archive } => run_from_spec(spec, archive),
        Command::Compare {
            baseline,
            intervention,
        } => run_paired_comparison(baseline, intervention),
        Command::Runoff { spec } => run_runoff_spec(spec),
        Command::Coupled { spec } => run_coupled_spec(spec),
        Command::Pools { spec } => run_pools_spec(spec),
        Command::SurfaceF32 { spec } => run_surface_f32_spec(spec),
        Command::ForecastBridge {
            blob,
            basin_id,
            area_km2,
            impervious_fraction,
            past_hours,
            window_hours,
        } => run_forecast_bridge(
            blob,
            basin_id,
            area_km2,
            impervious_fraction,
            past_hours,
            window_hours,
        ),
        Command::Pipeline {
            meteo,
            marine,
            stations,
            station_id,
            current_stage_m,
            basin_id,
            area_km2,
            impervious_fraction,
            past_hours,
            window_hours,
            tide_threshold_m,
            trigger_rain_mm,
            wind_m_per_s,
        } => run_four_stage_pipeline(PipelineArgs {
            meteo,
            marine,
            stations,
            station_id,
            current_stage_m,
            basin_id,
            area_km2,
            impervious_fraction,
            past_hours,
            window_hours,
            tide_threshold_m,
            trigger_rain_mm,
            wind_m_per_s,
        }),
        Command::Transcode {
            input,
            output,
            trim_start_s,
        } => {
            let frames = video::transcode_webm_to_mp4(&input, &output, trim_start_s)?;
            println!(
                "{{\"input\":\"{}\",\"output\":\"{}\",\"trim_start_s\":{trim_start_s},\"frames_encoded\":{frames}}}",
                input.display(),
                output.display()
            );
            Ok(())
        }
        Command::Pipes { spec } => run_native_pipes_spec(spec),
        Command::DrainageCheck { spec } => run_drainage_check(spec),
        Command::Resume { archive, from_run } => run_resume_check(archive, from_run),
        Command::Publish {
            archive,
            run_id,
            cycle_id,
            cycle_sequence,
            valid_from_unix_s,
            expires_at_unix_s,
            now_unix_s,
            scientific_approved,
            owner_approved,
        } => run_publish(
            archive,
            run_id,
            cycle_id,
            cycle_sequence,
            valid_from_unix_s,
            expires_at_unix_s,
            now_unix_s,
            scientific_approved,
            owner_approved,
        ),
    }
}

fn run_runoff_spec(spec_path: PathBuf) -> Result<(), String> {
    let raw = std::fs::read_to_string(&spec_path)
        .map_err(|error| format!("read {}: {error}", spec_path.display()))?;
    let spec: RunoffRunSpec = serde_json::from_str(&raw)
        .map_err(|error| format!("parse {}: {error}", spec_path.display()))?;
    let report = spec.execute().map_err(|error| error.to_string())?;
    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    println!("{json}");
    Ok(())
}

fn run_coupled_spec(spec_path: PathBuf) -> Result<(), String> {
    let raw = std::fs::read_to_string(&spec_path)
        .map_err(|error| format!("read {}: {error}", spec_path.display()))?;
    let spec: CoupledRunSpec = serde_json::from_str(&raw)
        .map_err(|error| format!("parse {}: {error}", spec_path.display()))?;
    let report = spec.execute().map_err(|error| error.to_string())?;
    let budget_assessment = assess_water_budget(report.water_budget);
    let envelope = serde_json::json!({
        "report": report,
        "budget_assessment": budget_assessment,
    });
    let json = serde_json::to_string_pretty(&envelope).map_err(|error| error.to_string())?;
    println!("{json}");
    if !budget_assessment.passed {
        return Err(format!(
            "coupled water budget failed: {:?}",
            budget_assessment.reasons
        ));
    }
    Ok(())
}

fn run_pools_spec(spec_path: PathBuf) -> Result<(), String> {
    let raw = std::fs::read_to_string(&spec_path)
        .map_err(|error| format!("read {}: {error}", spec_path.display()))?;
    let spec: PoolStructureRunSpec = serde_json::from_str(&raw)
        .map_err(|error| format!("parse {}: {error}", spec_path.display()))?;
    let report = spec.execute().map_err(|error| error.to_string())?;
    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    println!("{json}");
    Ok(())
}

fn run_surface_f32_spec(spec_path: PathBuf) -> Result<(), String> {
    let raw = std::fs::read_to_string(&spec_path)
        .map_err(|error| format!("read {}: {error}", spec_path.display()))?;
    let spec: SurfaceF32RunSpec = serde_json::from_str(&raw)
        .map_err(|error| format!("parse {}: {error}", spec_path.display()))?;
    let report = spec.execute().map_err(|error| error.to_string())?;
    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    println!("{json}");
    Ok(())
}

fn run_drainage_check(spec_path: PathBuf) -> Result<(), String> {
    let raw = std::fs::read_to_string(&spec_path)
        .map_err(|error| format!("read {}: {error}", spec_path.display()))?;
    let spec: SwmmDrainageSpec = serde_json::from_str(&raw)
        .map_err(|error| format!("parse {}: {error}", spec_path.display()))?;
    let report = spec.check().map_err(|error| error.to_string())?;
    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    println!("{json}");
    Ok(())
}

fn run_resume_check(archive_path: PathBuf, from_run: String) -> Result<(), String> {
    let archive = Archive::open(&archive_path).map_err(|error| error.to_string())?;
    let store = RunStore::new(&archive);
    let record = store
        .get_run(&from_run)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("run {from_run:?} not found in archive"))?;
    let blob = record
        .checkpoint_blob
        .ok_or_else(|| format!("run {from_run:?} has no persisted checkpoint blob"))?;
    let bytes = archive
        .read_blob(&blob.sha256)
        .map_err(|error| error.to_string())?;
    let package = package_from_bytes(bytes).map_err(|error| error.to_string())?;
    let engine = SurfaceEngine::resume_retained(&package).map_err(|error| error.to_string())?;
    let round_trip = engine.checkpoint().map_err(|error| error.to_string())?;
    let bitwise_match = round_trip.payload == package.payload;
    if !bitwise_match {
        return Err("resumed checkpoint payload diverged from archived blob".to_owned());
    }
    let summary = serde_json::json!({
        "run_id": from_run,
        "schema": package.header.schema,
        "contract": package.header.contract,
        "accepted_time_s": engine.time_s(),
        "accepted_steps": package.header.accepted_steps,
        "bitwise_match": bitwise_match,
        "budget": engine.water_budget(),
    });
    let json = serde_json::to_string_pretty(&summary).map_err(|error| error.to_string())?;
    println!("{json}");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_publish(
    archive_path: PathBuf,
    run_id: String,
    cycle_id: String,
    cycle_sequence: u64,
    valid_from_unix_s: i64,
    expires_at_unix_s: i64,
    now_unix_s: i64,
    scientific_approved: bool,
    owner_approved: bool,
) -> Result<(), String> {
    let archive = Archive::open(&archive_path).map_err(|error| error.to_string())?;
    let store = RunStore::new(&archive);
    let record = store
        .get_run(&run_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("run {run_id:?} not found in archive"))?;
    let current = store
        .load_published()
        .map_err(|error| error.to_string())?
        .map(|pointer| arus_forecasting::PublicationManifest {
            cycle_id: pointer.cycle_id,
            cycle_sequence: pointer.cycle_sequence,
            run_id: pointer.run_id,
            valid_from_unix_s: pointer.valid_from_unix_s,
            expires_at_unix_s: pointer.expires_at_unix_s,
            supersedes_cycle_id: pointer.supersedes_cycle_id,
        });

    let candidate = PublicationCandidate {
        cycle_id,
        cycle_sequence,
        run_id,
        classification: record.provenance.classification,
        execution_outcome: AdvanceOutcome::ReachedTarget,
        numerical_assessment_passed: record.conservation_passed,
        scientific_approval: if scientific_approved {
            ApprovalStatus::Approved
        } else {
            ApprovalStatus::Pending
        },
        owner_approval: if owner_approved {
            ApprovalStatus::Approved
        } else {
            ApprovalStatus::Pending
        },
        valid_from_unix_s,
        expires_at_unix_s,
    };

    let decision = evaluate_publication(&candidate, current.as_ref(), now_unix_s);
    if let PublicationDecision::Published(ref manifest) = decision {
        store
            .set_published(&PublishedPointer {
                cycle_id: manifest.cycle_id.clone(),
                cycle_sequence: manifest.cycle_sequence,
                run_id: manifest.run_id.clone(),
                valid_from_unix_s: manifest.valid_from_unix_s,
                expires_at_unix_s: manifest.expires_at_unix_s,
                supersedes_cycle_id: manifest.supersedes_cycle_id.clone(),
            })
            .map_err(|error| error.to_string())?;
    }
    let json = serde_json::to_string_pretty(&decision).map_err(|error| error.to_string())?;
    println!("{json}");
    match decision {
        PublicationDecision::Published(_) => Ok(()),
        PublicationDecision::Refused { reasons } => {
            Err(format!("publication refused: {}", reasons.join(", ")))
        }
    }
}

fn load_spec(path: &PathBuf) -> Result<SurfaceRunSpec, String> {
    let raw =
        std::fs::read(path).map_err(|error| format!("read spec {}: {error}", path.display()))?;
    serde_json::from_slice(&raw).map_err(|error| format!("parse spec {}: {error}", path.display()))
}

fn sha256_json<T: Serialize>(value: &T) -> Result<String, String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

fn run_scenario_summary(
    spec: &SurfaceRunSpec,
    baseline_id: &str,
) -> Result<ScenarioRunSummary, String> {
    let policy = EpisodePolicy::new(
        spec.threshold_m,
        spec.hysteresis_m,
        spec.recession_persistence_s,
    )
    .map_err(|error| error.to_string())?;
    let mut engine = spec.build_engine().map_err(|error| error.to_string())?;
    let probes = spec.location_probes();
    let mut trackers: Vec<EpisodeTracker> = probes
        .iter()
        .map(|probe| EpisodeTracker::new(probe.id.clone(), policy))
        .collect();
    for (probe, tracker) in probes.iter().zip(trackers.iter_mut()) {
        let initial = engine
            .cell_at(probe.x_m, probe.y_m)
            .map_err(|error| error.to_string())?;
        tracker
            .observe(engine.time_s(), initial)
            .map_err(|error| error.to_string())?;
    }
    let token = CancellationToken::new();
    let mut callback_error: Option<String> = None;
    engine
        .advance_with_probes(
            spec.target_time_s,
            StepBudget::new(spec.max_steps),
            &token,
            &probes,
            |_, samples| {
                for (tracker, sample) in trackers.iter_mut().zip(samples.iter()) {
                    if let Err(error) = tracker.observe(sample.time_s, sample.sample) {
                        callback_error = Some(error.to_string());
                    }
                }
            },
        )
        .map_err(|error| error.to_string())?;
    if let Some(error) = callback_error {
        return Err(error);
    }

    let mut location_peak_depths_m = BTreeMap::new();
    for tracker in trackers {
        let evo = tracker.finish();
        if let Some(max_depth) = evo.max_depth_m {
            location_peak_depths_m.insert(evo.location_id, max_depth);
        }
    }
    let forcing_digest = sha256_json(&spec.forcing)?;
    let initial_state_digest = sha256_json(&spec.initial_depth_m.to_bits())?;
    Ok(ScenarioRunSummary {
        manifest: ScenarioManifest {
            scenario_id: spec.run_id.clone(),
            baseline_id: baseline_id.to_owned(),
            forcing_digest,
            initial_state_digest,
            is_screening: true,
            assumptions: vec!["matched manifest forcing and initial depth".to_owned()],
            edits: Vec::new(),
        },
        budget: engine.water_budget(),
        location_peak_depths_m,
    })
}

fn run_paired_comparison(baseline_path: PathBuf, intervention_path: PathBuf) -> Result<(), String> {
    let baseline_spec = load_spec(&baseline_path)?;
    let intervention_spec = load_spec(&intervention_path)?;
    let baseline = run_scenario_summary(&baseline_spec, &baseline_spec.run_id)?;
    let intervention = run_scenario_summary(&intervention_spec, &baseline_spec.run_id)?;
    let report =
        compare_paired_runs(&baseline, &intervention).map_err(|error| error.to_string())?;
    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    println!("{json}");
    Ok(())
}

fn run_from_spec(spec_path: PathBuf, archive: Option<PathBuf>) -> Result<(), String> {
    let raw = std::fs::read(&spec_path)
        .map_err(|error| format!("read spec {}: {error}", spec_path.display()))?;
    let spec: SurfaceRunSpec = serde_json::from_slice(&raw)
        .map_err(|error| format!("parse spec {}: {error}", spec_path.display()))?;
    let policy = EpisodePolicy::new(
        spec.threshold_m,
        spec.hysteresis_m,
        spec.recession_persistence_s,
    )
    .map_err(|error| error.to_string())?;
    let mut engine = spec.build_engine().map_err(|error| error.to_string())?;
    let probes = spec.location_probes();
    execute_surface_outlook(
        &mut engine,
        spec.run_id,
        spec.classification,
        &probes,
        policy,
        spec.target_time_s,
        spec.max_steps,
        archive,
        "Manifest-driven CPU F64 surface run",
    )
}

fn run_outlook_demo(
    run_id: String,
    threshold_m: f64,
    archive: Option<PathBuf>,
) -> Result<(), String> {
    let policy = EpisodePolicy::new(threshold_m, 0.0, 10.0).map_err(|error| error.to_string())?;
    let mut engine = SurfaceEngine::demo_closed_basin_60s().map_err(|error| error.to_string())?;
    let probes = vec![
        LocationProbe {
            id: "basin-center".to_owned(),
            x_m: 25.0,
            y_m: 25.0,
        },
        LocationProbe {
            id: "outside-domain".to_owned(),
            x_m: 200.0,
            y_m: 25.0,
        },
    ];
    execute_surface_outlook(
        &mut engine,
        run_id,
        RunClassification::NumericalDemonstration,
        &probes,
        policy,
        60.0,
        10_000,
        archive,
        "Synthetic CPU F64 closed-basin outlook demonstration",
    )
}

#[allow(clippy::too_many_arguments)]
fn execute_surface_outlook(
    engine: &mut SurfaceEngine,
    run_id: String,
    classification: RunClassification,
    probes: &[LocationProbe],
    policy: EpisodePolicy,
    target_time_s: f64,
    max_steps: usize,
    archive: Option<PathBuf>,
    note: &str,
) -> Result<(), String> {
    let mut trackers: Vec<EpisodeTracker> = probes
        .iter()
        .map(|probe| EpisodeTracker::new(probe.id.clone(), policy))
        .collect();
    for (probe, tracker) in probes.iter().zip(trackers.iter_mut()) {
        let initial = engine
            .cell_at(probe.x_m, probe.y_m)
            .map_err(|error| error.to_string())?;
        tracker
            .observe(engine.time_s(), initial)
            .map_err(|error| error.to_string())?;
    }

    let token = CancellationToken::new();
    let mut callback_error: Option<String> = None;
    engine
        .advance_with_probes(
            target_time_s,
            StepBudget::new(max_steps),
            &token,
            probes,
            |_, samples| {
                for (tracker, sample) in trackers.iter_mut().zip(samples.iter()) {
                    if let Err(error) = tracker.observe(sample.time_s, sample.sample) {
                        callback_error = Some(error.to_string());
                    }
                }
            },
        )
        .map_err(|error| error.to_string())?;
    if let Some(error) = callback_error {
        return Err(error);
    }

    let budget_assessment = assess_water_budget(engine.water_budget());
    let outlooks: Vec<ApiLocationOutlook> = trackers
        .into_iter()
        .map(|tracker| to_api_outlook(tracker.finish(), classification))
        .collect();

    let checkpoint = match archive {
        Some(path) => {
            let opened = Archive::open(&path).map_err(|error| error.to_string())?;
            let package = engine.checkpoint().map_err(|error| error.to_string())?;
            let persisted = package
                .persist(&opened)
                .map_err(|error| error.to_string())?;
            let outlook_bytes = serde_json::to_vec(&outlooks).map_err(|error| error.to_string())?;
            let outlook_blob = opened
                .put_blob(&outlook_bytes)
                .map_err(|error| error.to_string())?;
            let caps = surface_capabilities();
            let store = RunStore::new(&opened);
            store
                .record_run(&RunRecord {
                    run_id: run_id.clone(),
                    provenance: Provenance {
                        backend: caps.backend,
                        contract: caps.contract,
                        classification,
                        note: note.to_owned(),
                    },
                    accepted_time_s: engine.time_s(),
                    conservation_passed: budget_assessment.passed,
                    checkpoint_blob: Some(persisted.blob.clone()),
                    outlook_blob: Some(outlook_blob),
                })
                .map_err(|error| error.to_string())?;
            Some(persisted)
        }
        None => None,
    };

    let envelope = OutlookEnvelope {
        run_id,
        classification,
        budget_assessment,
        checkpoint,
        outlooks,
    };
    let json = serde_json::to_string_pretty(&envelope).map_err(|error| error.to_string())?;
    println!("{json}");
    Ok(())
}

fn to_api_outlook(
    evolution: LocationEvolution,
    classification: RunClassification,
) -> ApiLocationOutlook {
    let support = match evolution.support {
        LocationSupportStatus::Supported => ApiLocationStatus::Supported,
        LocationSupportStatus::Unsupported => ApiLocationStatus::Unsupported,
    };
    let episodes = evolution
        .episodes
        .into_iter()
        .map(|ep| {
            let (onset_status, onset_time_s) = match ep.onset {
                OnsetStatus::Observed { time_s } => ("observed".to_owned(), time_s),
                OnsetStatus::LeftCensored { first_observed_s } => {
                    ("left-censored".to_owned(), first_observed_s)
                }
            };
            let (recession_status, recession_crossing_s, recession_confirmation_s) = match ep
                .recession
            {
                RecessionStatus::Confirmed {
                    crossing_time_s,
                    confirmation_time_s,
                } => (
                    "confirmed".to_owned(),
                    Some(crossing_time_s),
                    Some(confirmation_time_s),
                ),
                RecessionStatus::PendingAtHorizon {
                    crossing_time_s, ..
                } => ("pending-at-horizon".to_owned(), Some(crossing_time_s), None),
                RecessionStatus::RightCensored { .. } => ("right-censored".to_owned(), None, None),
            };
            ApiEpisodeSummary {
                onset_status,
                onset_time_s,
                initial_inundation: ep.initial_inundation,
                peak_depth_m: ep.peak_depth_m,
                peak_time_s: ep.peak_time_s,
                peak_at_horizon_end: ep.peak_at_horizon_end,
                exceedance_duration_s: ep.exceedance_duration_s,
                recession_status,
                recession_crossing_s,
                recession_confirmation_s,
                rebound_count: ep.rebound_count,
            }
        })
        .collect();

    ApiLocationOutlook {
        schema_version: API_SCHEMA_VERSION.to_owned(),
        location_id: evolution.location_id,
        classification,
        support,
        threshold_m: evolution.policy.threshold_m,
        horizon_start_s: evolution.horizon_start_s,
        horizon_end_s: evolution.horizon_end_s,
        max_depth_m: evolution.max_depth_m,
        episodes,
    }
}

fn run_native_pipes_spec(spec_path: PathBuf) -> Result<(), String> {
    let raw = std::fs::read_to_string(&spec_path)
        .map_err(|error| format!("read {} failed: {error}", spec_path.display()))?;
    let spec: NativePipeRunSpec = serde_json::from_str(&raw)
        .map_err(|error| format!("parse {} failed: {error}", spec_path.display()))?;
    let report = spec.execute().map_err(|error| error.to_string())?;
    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    println!("{json}");
    Ok(())
}

fn run_forecast_bridge(
    blob_path: PathBuf,
    basin_id: String,
    area_km2: f64,
    impervious_fraction: f64,
    past_hours: usize,
    window_hours: usize,
) -> Result<(), String> {
    let bytes = std::fs::read(&blob_path)
        .map_err(|error| format!("read {} failed: {error}", blob_path.display()))?;
    let parsed = parse_open_meteo_hourly(&bytes).map_err(|error| error.to_string())?;
    let mut members = Vec::with_capacity(parsed.series.len());

    for s in &parsed.series {
        let split = past_hours.min(s.precipitation_mm.len().saturating_sub(1));
        let past_rain_mm: f64 = s.precipitation_mm[..split].iter().filter_map(|v| *v).sum();
        let latest_soil = s.soil_moisture_0_to_7cm[..=split]
            .iter()
            .rev()
            .find_map(|v| *v);
        let antecedent = derive_antecedent_state(past_rain_mm, latest_soil, 0.30);

        let forecast_rain = &s.precipitation_mm[split..];
        let missing_precip_hours = forecast_rain.iter().filter(|v| v.is_none()).count();
        let spec = build_runoff_spec_from_hourly(
            format!("{}-{}", basin_id, s.model),
            basin_id.clone(),
            area_km2,
            impervious_fraction,
            forecast_rain,
            antecedent,
        )
        .map_err(|error| error.to_string())?;
        let report = spec.execute().map_err(|error| error.to_string())?;
        let peak_win =
            detect_peak_window(&report, forecast_rain, window_hours).map_err(|e| e.to_string())?;

        let total_rain_mm: f64 = forecast_rain.iter().filter_map(|v| *v).sum();
        let peak_hourly_mm = forecast_rain
            .iter()
            .filter_map(|v| *v)
            .fold(0.0_f64, f64::max);
        let mut peak_24h_mm = 0.0_f64;
        let win24 = 24.min(forecast_rain.len());
        if win24 > 0 {
            for start in 0..=(forecast_rain.len() - win24) {
                let sum: f64 = forecast_rain[start..start + win24]
                    .iter()
                    .filter_map(|v| *v)
                    .sum();
                if sum > peak_24h_mm {
                    peak_24h_mm = sum;
                }
            }
        }

        members.push(ModelForecastMember {
            model: s.model.clone(),
            total_rain_mm,
            peak_24h_mm,
            peak_hourly_mm,
            total_runoff_m3: report.total_outlet_m3,
            peak_window_start_h: peak_win.start_hour,
            peak_window_end_h: peak_win.end_hour,
            missing_precip_hours,
            antecedent_deficit: antecedent.effective_maximum_deficit,
        });
    }

    let summary = summarize_ensemble(members);
    let json = serde_json::to_string_pretty(&summary).map_err(|error| error.to_string())?;
    println!("{json}");
    Ok(())
}

struct PipelineArgs {
    meteo: PathBuf,
    marine: Option<PathBuf>,
    stations: Option<PathBuf>,
    station_id: Option<String>,
    current_stage_m: Option<f64>,
    basin_id: String,
    area_km2: f64,
    impervious_fraction: f64,
    past_hours: usize,
    window_hours: usize,
    tide_threshold_m: f64,
    trigger_rain_mm: f64,
    wind_m_per_s: f64,
}

fn run_four_stage_pipeline(args: PipelineArgs) -> Result<(), String> {
    let meteo_bytes = std::fs::read(&args.meteo)
        .map_err(|error| format!("read {} failed: {error}", args.meteo.display()))?;
    let parsed = parse_open_meteo_hourly(&meteo_bytes).map_err(|error| error.to_string())?;

    // Stage 2: 72-120h 1D hydrology ensemble across all archived NWP models
    let mut members = Vec::with_capacity(parsed.series.len());
    let mut worst_deficit = 0.30_f64;
    let mut worst_window_rain: Vec<Option<f64>> = Vec::new();
    let mut worst_peak_24h = -1.0_f64;

    for s in &parsed.series {
        let split = args
            .past_hours
            .min(s.precipitation_mm.len().saturating_sub(1));
        let past_rain_mm: f64 = s.precipitation_mm[..split].iter().filter_map(|v| *v).sum();
        let latest_soil = s.soil_moisture_0_to_7cm[..=split]
            .iter()
            .rev()
            .find_map(|v| *v);
        let antecedent = derive_antecedent_state(past_rain_mm, latest_soil, 0.30);
        worst_deficit = worst_deficit.min(antecedent.effective_maximum_deficit);

        let forecast_rain = &s.precipitation_mm[split..];
        let missing_precip_hours = forecast_rain.iter().filter(|v| v.is_none()).count();
        let spec = build_runoff_spec_from_hourly(
            format!("{}-{}", args.basin_id, s.model),
            args.basin_id.clone(),
            args.area_km2,
            args.impervious_fraction,
            forecast_rain,
            antecedent,
        )
        .map_err(|error| error.to_string())?;
        let report = spec.execute().map_err(|error| error.to_string())?;
        let peak_win = detect_peak_window(&report, forecast_rain, args.window_hours)
            .map_err(|e| e.to_string())?;

        let total_rain_mm: f64 = forecast_rain.iter().filter_map(|v| *v).sum();
        let peak_hourly_mm = forecast_rain
            .iter()
            .filter_map(|v| *v)
            .fold(0.0_f64, f64::max);
        let mut peak_24h_mm = 0.0_f64;
        let win24 = 24.min(forecast_rain.len());
        if win24 > 0 {
            for start in 0..=(forecast_rain.len() - win24) {
                let sum: f64 = forecast_rain[start..start + win24]
                    .iter()
                    .filter_map(|v| *v)
                    .sum();
                if sum > peak_24h_mm {
                    peak_24h_mm = sum;
                }
            }
        }
        if peak_24h_mm > worst_peak_24h {
            worst_peak_24h = peak_24h_mm;
            worst_window_rain = forecast_rain
                .get(peak_win.start_hour..peak_win.end_hour)
                .unwrap_or(&[])
                .to_vec();
        }
        members.push(ModelForecastMember {
            model: s.model.clone(),
            total_rain_mm,
            peak_24h_mm,
            peak_hourly_mm,
            total_runoff_m3: report.total_outlet_m3,
            peak_window_start_h: peak_win.start_hour,
            peak_window_end_h: peak_win.end_hour,
            missing_precip_hours,
            antecedent_deficit: antecedent.effective_maximum_deficit,
        });
    }

    let stage2_ensemble = summarize_ensemble(members);

    // Stage 1: Initial basin state + gauge headroom (or estimated GloFAS warm-up fallback)
    let registry_opt: Option<StationRegistry> = match &args.stations {
        Some(path) => {
            let raw = std::fs::read_to_string(path)
                .map_err(|e| format!("read {} failed: {e}", path.display()))?;
            let reg: StationRegistry = serde_json::from_str(&raw)
                .map_err(|e| format!("parse {} failed: {e}", path.display()))?;
            reg.validate().map_err(|e| e.to_string())?;
            Some(reg)
        }
        None => None,
    };
    let matched_station = match (&registry_opt, &args.station_id) {
        (Some(reg), Some(id)) => reg.find(id),
        _ => None,
    };
    let stage1_initial_state =
        build_initial_basin_state(matched_station, args.current_stage_m, worst_deficit);

    // Stage 3: Compound coastal tide-lock overlap check against Stage 2 worst-case peak window
    let worst_member = stage2_ensemble
        .members
        .iter()
        .find(|m| m.model == stage2_ensemble.worst_case_model)
        .or_else(|| stage2_ensemble.members.first());
    let (win_start_h, win_end_h) = worst_member
        .map(|m| (m.peak_window_start_h, m.peak_window_end_h))
        .unwrap_or((0, args.window_hours));

    let stage3_compound_tide = match &args.marine {
        Some(path) => {
            let bytes =
                std::fs::read(path).map_err(|e| format!("read {} failed: {e}", path.display()))?;
            let marine = parse_open_meteo_marine(&bytes).map_err(|e| e.to_string())?;
            Some(evaluate_compound_tide_lock(
                win_start_h,
                win_end_h,
                args.past_hours,
                &marine.times,
                &marine.sea_level_height_msl_m,
                args.tide_threshold_m,
            ))
        }
        None => None,
    };

    // Stage 4: Windowed 2D hydr 0.2.0 inundation + onset/recession (waktu surut)
    let tide_risk = stage3_compound_tide
        .as_ref()
        .is_some_and(|t| t.compound_backwater_risk);
    let low_headroom = stage1_initial_state
        .headroom_to_alert_m
        .is_some_and(|h| h <= 0.5);
    let heavy_rain = stage2_ensemble.max_peak_24h_mm >= args.trigger_rain_mm;
    let should_trigger = heavy_rain || tide_risk || low_headroom;

    let stage4_windowed_2d = if should_trigger && !worst_window_rain.is_empty() {
        let mut reasons = Vec::new();
        if heavy_rain {
            reasons.push(format!(
                "peak_24h_mm={:.1}>={:.1}",
                stage2_ensemble.max_peak_24h_mm, args.trigger_rain_mm
            ));
        }
        if tide_risk {
            reasons.push("compound_tide_lock".to_owned());
        }
        if low_headroom {
            reasons.push("low_gauge_headroom".to_owned());
        }

        let terrain = TerrainGrid::new(
            GridSpec {
                columns: 6,
                rows: 3,
                dx_m: 100.0,
                dy_m: 100.0,
            },
            SpatialReference {
                projected_crs: "local-metric-screening".to_owned(),
                vertical_datum: "local-datum".to_owned(),
                origin_m: [0.0, 0.0],
            },
            "pipeline-screening-valley",
            (0..18)
                .map(|i| Some(10.0 - ((i % 6) as f64) * 0.35))
                .collect(),
        )
        .map_err(|e| e.to_string())?;
        let open_faces: Vec<BoundaryFaceSpec> = (0..3)
            .map(|row| BoundaryFaceSpec {
                axis: BoundaryAxisSpec::X,
                column: 6,
                row,
            })
            .collect();
        let boundaries: Vec<BoundaryForcingSpec> = open_faces
            .iter()
            .copied()
            .map(|face| BoundaryForcingSpec::ExteriorDry { face })
            .collect();

        let forcing: Vec<RainIntervalSpec> = worst_window_rain
            .iter()
            .enumerate()
            .map(|(h, opt_mm)| RainIntervalSpec {
                start_s: (h as f64) * 3600.0,
                end_s: ((h + 1) as f64) * 3600.0,
                uniform_rain_m_per_s: opt_mm.unwrap_or(0.0).max(0.0) / 3_600_000.0,
                boundaries: boundaries.clone(),
            })
            .collect();
        let total_s = (worst_window_rain.len() as f64) * 3600.0;
        let max_steps = (total_s as usize).saturating_add(5_000);
        let spec = SurfaceRunSpec {
            run_id: format!("{}-stage4-2d", args.basin_id),
            classification: RunClassification::NumericalDemonstration,
            terrain,
            uniform_manning_n: 0.035,
            land: LandSpec::UniformImpervious,
            initial_depth_m: 0.0,
            open_faces,
            forcing,
            target_time_s: total_s,
            max_steps,
            probes: vec![ProbeSpec {
                id: "outlet-corridor".to_owned(),
                x_m: 450.0,
                y_m: 150.0,
            }],
            threshold_m: 0.02,
            hysteresis_m: 0.005,
            recession_persistence_s: 300.0,
            coastal: if args.wind_m_per_s != 0.0 {
                Some(CoastalForcingSpec {
                    wind_x_m_per_s: args.wind_m_per_s,
                    ..CoastalForcingSpec::default()
                })
            } else {
                None
            },
        };
        let mut engine = spec.build_engine().map_err(|e| e.to_string())?;
        let token = CancellationToken::new();
        let probes = spec.location_probes();
        let mut tracker = EpisodeTracker::new(
            "outlet-corridor",
            EpisodePolicy {
                threshold_m: spec.threshold_m,
                hysteresis_m: spec.hysteresis_m,
                recession_persistence_s: spec.recession_persistence_s,
            },
        );
        if let Some(p) = probes.first() {
            let initial = engine.cell_at(p.x_m, p.y_m).map_err(|e| e.to_string())?;
            tracker
                .observe(engine.time_s(), initial)
                .map_err(|e| e.to_string())?;
        }
        let mut cb_err: Option<String> = None;
        engine
            .advance_with_probes(
                total_s,
                StepBudget::new(max_steps),
                &token,
                &probes,
                |_, samples| {
                    if let Some(s) = samples.first()
                        && let Err(e) = tracker.observe(s.time_s, s.sample)
                    {
                        cb_err = Some(e.to_string());
                    }
                },
            )
            .map_err(|e| e.to_string())?;
        if let Some(e) = cb_err {
            return Err(e);
        }
        let evolution = tracker.finish();
        let (onset_time_s, recession_status) = match evolution.episodes.first() {
            Some(ep) => {
                let onset = match ep.onset {
                    OnsetStatus::Observed { time_s } => Some(time_s),
                    OnsetStatus::LeftCensored { first_observed_s } => Some(first_observed_s),
                };
                let rec = match ep.recession {
                    RecessionStatus::Confirmed {
                        confirmation_time_s,
                        ..
                    } => format!("confirmed@{confirmation_time_s:.0}s"),
                    RecessionStatus::PendingAtHorizon { .. } => "pending-at-horizon".to_owned(),
                    RecessionStatus::RightCensored { .. } => "right-censored".to_owned(),
                };
                (onset, rec)
            }
            None => (None, "no-exceedance".to_owned()),
        };
        let budget = engine.water_budget();
        Windowed2DOutcome {
            triggered: true,
            trigger_reason: reasons.join(" + "),
            driver_model: stage2_ensemble.worst_case_model.clone(),
            window_hours: worst_window_rain.len(),
            max_depth_m: evolution.max_depth_m.unwrap_or(0.0),
            onset_time_s,
            recession_status,
            mass_residual_m3: budget.residual_m3,
            mass_valid: budget.within_residual_bound(),
        }
    } else {
        Windowed2DOutcome {
            triggered: false,
            trigger_reason: "below-trigger-thresholds".to_owned(),
            driver_model: stage2_ensemble.worst_case_model.clone(),
            window_hours: 0,
            max_depth_m: 0.0,
            onset_time_s: None,
            recession_status: "not-run".to_owned(),
            mass_residual_m3: 0.0,
            mass_valid: true,
        }
    };

    let report = FourStagePipelineReport {
        basin_id: args.basin_id,
        label: "screening-only".to_owned(),
        classification: RunClassification::NumericalDemonstration,
        stage1_initial_state,
        stage2_ensemble,
        stage3_compound_tide,
        stage4_windowed_2d,
    };
    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    println!("{json}");
    Ok(())
}
