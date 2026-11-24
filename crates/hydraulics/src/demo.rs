//! First integration milestone demonstrations on the `hydr` CPU `F64` reference runtime.
//!
//! These synthetic cases verify capability admission, conservation, cancellation,
//! exact binary checkpointing, and forecast splicing. They establish no site-specific
//! Malaysian flood calibration or operational readiness.

use arus_assets::Archive;
use arus_domain::{Provenance, RunClassification};
use arus_jobs::{CancellationToken, ExecutionRecord, StepBudget};
use arus_model_api::{
    AdvanceOutcome, BackendCapabilities, CellSample, ModelError, PersistedCheckpoint, Result,
    WaterBudget,
};
use hydr::capability::{BoundaryKind, SourceKind};
use hydr::grid::CartesianGrid;
use hydr::model::{CellState, Domain, FlowState, SpatialReference};
use hydr::soil::{LandModel, SoilState};
use hydr::solver::{
    Axis, BoundaryConditions, BoundaryFace, FluxPort, ForcingInterval, ForcingSchedule,
    InitialState, SolverConfig, SolverSetup,
};

use crate::capabilities::check_surface_admission;
use crate::surface::SurfaceEngine;

const MAX_STEPS: usize = 10_000;
const CANCEL_AFTER_STEPS: usize = 30;
const CYCLE_EDGES_S: [f64; 4] = [0.0, 1.0, 2.0, 3.0];

const PORT_FACE: BoundaryFace = BoundaryFace {
    axis: Axis::X,
    column: 0,
    row: 0,
};

/// Result of the closed-basin rainfall demonstration.
#[derive(Debug, Clone, PartialEq)]
pub struct ClosedBasinDemoReport {
    pub capabilities: BackendCapabilities,
    pub accepted_time_s: f64,
    pub center_sample: CellSample,
    pub budget: WaterBudget,
    pub checkpoint_bytes: usize,
}

/// Result of the native-worker lifecycle demonstration.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeWorkerDemoReport {
    pub capabilities: BackendCapabilities,
    pub execution: ExecutionRecord,
    pub cancelled_at_s: f64,
    pub observed_steps: usize,
    pub boundary_bitwise_match: bool,
    pub forecast_splice_bitwise_match: bool,
    pub persisted_checkpoint: Option<PersistedCheckpoint>,
}

/// Runs a 5x5 closed basin under constant rainfall for 60 s and verifies conservation
/// plus exact checkpoint round-trip.
pub fn run_closed_basin_demo() -> Result<ClosedBasinDemoReport> {
    let capabilities = check_surface_admission(
        &[BoundaryKind::Wall],
        &[SourceKind::Rain, SourceKind::ImperviousLand],
    )?;

    let grid = CartesianGrid::new(5, 5, 10.0, 10.0)
        .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
    let n = grid.cell_count();
    let reference = SpatialReference::new(
        "synthetic metric plane".to_owned(),
        "synthetic datum".to_owned(),
        [0.0, 0.0],
    )
    .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
    let domain = Domain::new(grid, vec![0.0; n], vec![0.03; n], vec![true; n], reference)
        .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
    let land = LandModel::impervious(&domain);
    let soil = SoilState::initialized(&land)
        .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
    let flow = FlowState::new(
        &domain,
        vec![
            CellState {
                h: 0.0,
                qx: 0.0,
                qy: 0.0,
            };
            n
        ],
    )
    .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
    let boundaries = BoundaryConditions::impermeable_walls();
    let forcing = ForcingSchedule::new(
        &domain,
        &boundaries,
        vec![ForcingInterval {
            start: 0.0,
            end: 60.0,
            rain: vec![1e-4; n],
            potential_evaporation: vec![0.0; n],
            withdrawal: vec![0.0; n],
            boundaries: Vec::new(),
        }],
    )
    .map_err(|error| ModelError::InvalidInput(error.to_string()))?;

    let mut engine = SurfaceEngine::new(
        SolverSetup {
            domain,
            land,
            boundaries,
            forcing,
            config: SolverConfig::default(),
        },
        InitialState {
            time: 0.0,
            flow,
            soil,
        },
    )?;

    let summary = engine.advance_to(60.0, StepBudget::new(MAX_STEPS))?;
    if summary.outcome != AdvanceOutcome::ReachedTarget {
        return Err(ModelError::Engine(format!(
            "closed basin demo stopped early: {:?}",
            summary.outcome
        )));
    }
    let package = engine.checkpoint()?;
    let resumed = SurfaceEngine::resume_retained(&package)?;
    let resumed_package = resumed.checkpoint()?;
    if package.payload != resumed_package.payload {
        return Err(ModelError::Checkpoint(
            "closed basin checkpoint round-trip altered binary payload".to_owned(),
        ));
    }

    let budget = engine.water_budget();
    if !budget.within_residual_bound() || !budget.within_adjustment_limit() {
        return Err(ModelError::Engine(format!(
            "closed basin budget exceeded bounds: {budget:?}"
        )));
    }

    Ok(ClosedBasinDemoReport {
        capabilities,
        accepted_time_s: engine.time_s(),
        center_sample: engine.cell_at(25.0, 25.0)?,
        budget,
        checkpoint_bytes: package.payload.len(),
    })
}

/// Runs the native worker lifecycle demonstration: capability check, cancellation,
/// optional immutable archive checkpointing, restore/resume bitwise parity, and forecast splice.
pub fn run_native_worker_demo(archive: Option<&Archive>) -> Result<NativeWorkerDemoReport> {
    let capabilities = check_surface_admission(
        &[BoundaryKind::Wall, BoundaryKind::FluxPort],
        &[SourceKind::Rain, SourceKind::ImperviousLand],
    )?;

    let mut uninterrupted = build_port_engine(0..2)?;
    require_target(&mut uninterrupted, CYCLE_EDGES_S[2])?;

    let mut worker = build_port_engine(0..2)?;
    let token = CancellationToken::new();
    let mut observed_steps = 0_usize;
    let cancelled = worker.advance_controlled(
        CYCLE_EDGES_S[2],
        StepBudget::new(MAX_STEPS),
        &token,
        |step| {
            observed_steps = step.step;
            if observed_steps == CANCEL_AFTER_STEPS {
                token.cancel();
            }
        },
    )?;
    if cancelled.outcome != AdvanceOutcome::Cancelled {
        return Err(ModelError::Engine(format!(
            "expected cancellation after {CANCEL_AFTER_STEPS} steps, got {:?}",
            cancelled.outcome
        )));
    }
    let cancel_package = worker.checkpoint()?;
    let persisted_checkpoint = match archive {
        Some(archive) => Some(cancel_package.persist(archive)?),
        None => None,
    };
    let reloaded_package = match (archive, &persisted_checkpoint) {
        (Some(archive), Some(persisted)) => {
            arus_model_api::CheckpointPackage::load(archive, persisted)?
        }
        _ => cancel_package.clone(),
    };

    token.reset();
    let resumed_call = worker.advance_controlled(
        CYCLE_EDGES_S[2],
        StepBudget::new(MAX_STEPS),
        &token,
        |step| {
            observed_steps = step.step;
        },
    )?;
    if resumed_call.outcome != AdvanceOutcome::ReachedTarget {
        return Err(ModelError::Engine(format!(
            "resumed worker stopped as {:?}",
            resumed_call.outcome
        )));
    }

    let mut restored = build_port_engine(0..2)?;
    restored.restore(&reloaded_package)?;
    require_target(&mut restored, CYCLE_EDGES_S[2])?;

    let mut resumed_pkg = SurfaceEngine::resume_retained(&reloaded_package)?;
    require_target(&mut resumed_pkg, CYCLE_EDGES_S[2])?;

    let boundary_ref = uninterrupted.checkpoint()?.payload;
    let boundary_bitwise_match = worker.checkpoint()?.payload == boundary_ref
        && restored.checkpoint()?.payload == boundary_ref
        && resumed_pkg.checkpoint()?.payload == boundary_ref;
    if !boundary_bitwise_match {
        return Err(ModelError::Checkpoint(
            "cycle-boundary checkpoints diverged from uninterrupted reference".to_owned(),
        ));
    }

    let mut single_forecast = build_port_engine(0..3)?;
    require_target(&mut single_forecast, CYCLE_EDGES_S[2])?;
    require_target(&mut single_forecast, CYCLE_EDGES_S[3])?;
    let single_payload = single_forecast.checkpoint()?.payload;

    let boundary_pkg = worker.checkpoint()?;
    let mut forecast_resumed = SurfaceEngine::resume(&boundary_pkg, port_schedule(2..3)?)?;
    require_target(&mut forecast_resumed, CYCLE_EDGES_S[3])?;

    worker.continue_with_forcing(port_schedule(2..3)?)?;
    require_target(&mut worker, CYCLE_EDGES_S[3])?;

    let forecast_splice_bitwise_match = worker.checkpoint()?.payload == single_payload
        && forecast_resumed.checkpoint()?.payload == single_payload;
    if !forecast_splice_bitwise_match {
        return Err(ModelError::Checkpoint(
            "forecast-splice checkpoints diverged from single-run reference".to_owned(),
        ));
    }

    let budget = worker.water_budget();
    if !budget.within_residual_bound() || !budget.within_adjustment_limit() {
        return Err(ModelError::Engine(format!(
            "native worker budget exceeded bounds: {budget:?}"
        )));
    }

    let execution = ExecutionRecord {
        job_id: "hydr-native-worker-demo".to_owned(),
        provenance: Provenance {
            backend: capabilities.backend.clone(),
            contract: capabilities.contract.clone(),
            classification: RunClassification::NumericalDemonstration,
            note: "Synthetic CPU F64 capability, cancellation, checkpoint, and splice check"
                .to_owned(),
        },
        outcome: AdvanceOutcome::ReachedTarget,
        accepted_time_s: worker.time_s(),
        steps_taken: observed_steps,
        budget,
        checkpoint: persisted_checkpoint.clone(),
    };

    Ok(NativeWorkerDemoReport {
        capabilities,
        execution,
        cancelled_at_s: cancelled.accepted_time_s,
        observed_steps,
        boundary_bitwise_match,
        forecast_splice_bitwise_match,
        persisted_checkpoint,
    })
}

fn require_target(engine: &mut SurfaceEngine, target_s: f64) -> Result<()> {
    let summary = engine.advance_to(target_s, StepBudget::new(MAX_STEPS))?;
    if summary.outcome != AdvanceOutcome::ReachedTarget {
        return Err(ModelError::Engine(format!(
            "advance to {target_s} s stopped as {:?} at {} s",
            summary.outcome, summary.accepted_time_s
        )));
    }
    Ok(())
}

fn port_domain() -> Result<Domain> {
    let side = 4_usize;
    let grid = CartesianGrid::new(side, side, 1.0, 1.0)
        .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
    let reference = SpatialReference::new(
        "synthetic metric plane".to_owned(),
        "synthetic datum".to_owned(),
        [0.0, 0.0],
    )
    .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
    Domain::new(
        grid,
        vec![0.0; side * side],
        vec![0.03; side * side],
        vec![true; side * side],
        reference,
    )
    .map_err(|error| ModelError::InvalidInput(error.to_string()))
}

fn port_interval(index: usize) -> ForcingInterval {
    let scale = (index + 1) as f64;
    let cells = 16_usize;
    ForcingInterval {
        start: CYCLE_EDGES_S[index],
        end: CYCLE_EDGES_S[index + 1],
        rain: vec![1e-4 * scale; cells],
        potential_evaporation: vec![0.0; cells],
        withdrawal: vec![0.0; cells],
        boundaries: vec![
            FluxPort {
                face: PORT_FACE,
                water_rate: -1e-3 * scale,
                momentum_rate: [-4.905, 0.0],
                signal_speed: 10.0,
                provenance: "manufactured full flux, h near 1 m, speed bound 10 m/s".to_owned(),
            }
            .into(),
        ],
    }
}

fn port_schedule(range: std::ops::Range<usize>) -> Result<ForcingSchedule> {
    let domain = port_domain()?;
    let boundaries = BoundaryConditions::with_open_faces(&domain, vec![PORT_FACE])
        .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
    ForcingSchedule::new(&domain, &boundaries, range.map(port_interval).collect())
        .map_err(|error| ModelError::InvalidInput(error.to_string()))
}

fn build_port_engine(range: std::ops::Range<usize>) -> Result<SurfaceEngine> {
    let domain = port_domain()?;
    let land = LandModel::impervious(&domain);
    let soil = SoilState::initialized(&land)
        .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
    let flow = FlowState::new(
        &domain,
        vec![
            CellState {
                h: 1.0,
                ..CellState::default()
            };
            16
        ],
    )
    .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
    let boundaries = BoundaryConditions::with_open_faces(&domain, vec![PORT_FACE])
        .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
    let forcing = ForcingSchedule::new(&domain, &boundaries, range.map(port_interval).collect())
        .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
    SurfaceEngine::new(
        SolverSetup {
            domain,
            land,
            boundaries,
            forcing,
            config: SolverConfig::default(),
        },
        InitialState {
            time: 0.0,
            flow,
            soil,
        },
    )
}
