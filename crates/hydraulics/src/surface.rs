//! Surface-flow engine wrapper over `hydr::solver::Solver<CpuRuntime>`.

use arus_geo::TerrainGrid;
use arus_jobs::{CancellationToken, StepBudget};
use arus_model_api::{
    AdvanceOutcome, AdvanceSummary, CellSample, CheckpointPackage, ModelError, Result, StepSummary,
    WaterBudget,
};
use hydr::grid::CartesianGrid;
use hydr::model::{CellState, Domain, SpatialReference as HydrSpatialReference};
use hydr::solver::{
    AdvanceReport, AdvanceStatus, CellResult, ForcingSchedule, InitialState, Solver, SolverConfig,
    SolverSetup, WaterLedger,
};

use crate::codec::{decode_snapshot, encode_snapshot};
use crate::runtime::{ReferenceRuntime, reference_cpu_client};

/// Converts an `arus_geo::TerrainGrid` and row-major Manning roughness array into a checked
/// `hydr::model::Domain`. Nodata (`None`) terrain cells map to inactive hydraulic cells.
pub fn domain_from_terrain(terrain: &TerrainGrid, manning: Vec<f64>) -> Result<Domain> {
    let spec = terrain.grid();
    let grid = CartesianGrid::new(spec.columns, spec.rows, spec.dx_m, spec.dy_m)
        .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
    let reference = HydrSpatialReference::new(
        terrain.reference().projected_crs.clone(),
        terrain.reference().vertical_datum.clone(),
        terrain.reference().origin_m,
    )
    .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
    let active = terrain.active_mask();
    let bed: Vec<f64> = terrain
        .cells()
        .iter()
        .map(|cell| cell.unwrap_or(0.0))
        .collect();
    Domain::new(grid, bed, manning, active, reference)
        .map_err(|error| ModelError::InvalidInput(error.to_string()))
}

/// Named projected coordinate probe evaluated at accepted solver steps.
#[derive(Debug, Clone, PartialEq)]
pub struct LocationProbe {
    pub id: String,
    pub x_m: f64,
    pub y_m: f64,
}

/// Sample emitted for one `LocationProbe` at one accepted solver time.
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeSample {
    pub id: String,
    pub time_s: f64,
    pub sample: CellSample,
}

/// Deterministic CPU `F64` shallow-water engine wrapping `hydr::solver::Solver`.
pub struct SurfaceEngine {
    solver: Solver<ReferenceRuntime>,
    observed_steps: usize,
}

impl SurfaceEngine {
    /// Constructs a checked CPU `F64` surface solver.
    pub fn new(setup: SolverSetup, initial: InitialState) -> Result<Self> {
        let solver = Solver::new(reference_cpu_client(), setup, initial)
            .map_err(|error| ModelError::Engine(error.to_string()))?;
        Ok(Self {
            solver,
            observed_steps: 0,
        })
    }

    /// Builds a synthetic 5x5 closed-basin demonstration engine (60 s constant rain) without
    /// exposing `hydr` setup types to application callers.
    pub fn demo_closed_basin_60s() -> Result<Self> {
        crate::demo::build_closed_basin_60s_engine()
    }

    /// Resumes an engine from an exact binary checkpoint package under `forcing`.
    pub fn resume(package: &CheckpointPackage, forcing: ForcingSchedule) -> Result<Self> {
        let data = decode_snapshot(package)?;
        let observed_steps = data.accepted_steps;
        let solver = Solver::resume(reference_cpu_client(), &data, forcing)
            .map_err(|error| ModelError::Engine(error.to_string()))?;
        Ok(Self {
            solver,
            observed_steps,
        })
    }

    /// Resumes an engine from `package` using the forcing window retained in the checkpoint.
    pub fn resume_retained(package: &CheckpointPackage) -> Result<Self> {
        let data = decode_snapshot(package)?;
        let observed_steps = data.accepted_steps;
        let forcing = data.setup.forcing.clone();
        let solver = Solver::resume(reference_cpu_client(), &data, forcing)
            .map_err(|error| ModelError::Engine(error.to_string()))?;
        Ok(Self {
            solver,
            observed_steps,
        })
    }

    /// Advances toward `target_s` within `budget`.
    pub fn advance_to(&mut self, target_s: f64, budget: StepBudget) -> Result<AdvanceSummary> {
        let report = self
            .solver
            .advance_to(target_s, budget.max_steps)
            .map_err(|error| ModelError::Engine(error.to_string()))?;
        self.observed_steps += report.accepted_steps;
        Ok(map_advance_report(report))
    }

    /// Advances toward `target_s`, checking `token` and invoking `on_step` at accepted steps.
    pub fn advance_controlled<F>(
        &mut self,
        target_s: f64,
        budget: StepBudget,
        token: &CancellationToken,
        mut on_step: F,
    ) -> Result<AdvanceSummary>
    where
        F: FnMut(StepSummary),
    {
        let mut step_counter = self.observed_steps;
        let report = self
            .solver
            .advance_controlled(
                target_s,
                budget.max_steps,
                || token.is_cancelled(),
                |step, cells| {
                    step_counter += 1;
                    let max_depth_m = cells.iter().map(|cell| cell.h).fold(0.0, f64::max);
                    on_step(StepSummary {
                        step: step_counter,
                        start_s: step.start,
                        end_s: step.end,
                        residual_m3: step.residual,
                        max_depth_m,
                    });
                },
            )
            .map_err(|error| ModelError::Engine(error.to_string()))?;
        self.observed_steps = step_counter;
        Ok(map_advance_report(report))
    }

    /// Advances toward `target_s` and samples `probes` at every accepted step directly from
    /// accepted cell states without spatial interpolation.
    pub fn advance_with_probes<F>(
        &mut self,
        target_s: f64,
        budget: StepBudget,
        token: &CancellationToken,
        probes: &[LocationProbe],
        mut on_probes: F,
    ) -> Result<AdvanceSummary>
    where
        F: FnMut(StepSummary, &[ProbeSample]),
    {
        let domain = self.solver.domain().clone();
        let resolved: Vec<(&str, Option<usize>)> = probes
            .iter()
            .map(|probe| {
                (
                    probe.id.as_str(),
                    locate_active_cell(&domain, probe.x_m, probe.y_m),
                )
            })
            .collect();
        let mut step_counter = self.observed_steps;
        let mut samples = Vec::with_capacity(resolved.len());
        let report = self
            .solver
            .advance_controlled(
                target_s,
                budget.max_steps,
                || token.is_cancelled(),
                |step, cells| {
                    step_counter += 1;
                    let max_depth_m = cells.iter().map(|cell| cell.h).fold(0.0, f64::max);
                    samples.clear();
                    for &(id, cell_index) in &resolved {
                        let sample =
                            match cell_index.and_then(|idx| cells.get(idx).map(|c| (idx, c))) {
                                Some((idx, cell)) => sample_from_cell(&domain, idx, *cell),
                                None => CellSample::Unsupported,
                            };
                        samples.push(ProbeSample {
                            id: id.to_owned(),
                            time_s: step.end,
                            sample,
                        });
                    }
                    on_probes(
                        StepSummary {
                            step: step_counter,
                            start_s: step.start,
                            end_s: step.end,
                            residual_m3: step.residual,
                            max_depth_m,
                        },
                        &samples,
                    );
                },
            )
            .map_err(|error| ModelError::Engine(error.to_string()))?;
        self.observed_steps = step_counter;
        Ok(map_advance_report(report))
    }

    /// Splices future forcing onto accepted history without resetting state or ledgers.
    pub fn continue_with_forcing(&mut self, forcing: ForcingSchedule) -> Result<()> {
        self.solver
            .continue_with_forcing(forcing)
            .map_err(|error| ModelError::Engine(error.to_string()))
    }

    /// Captures an exact binary checkpoint package.
    pub fn checkpoint(&self) -> Result<CheckpointPackage> {
        let data = self
            .solver
            .snapshot()
            .data()
            .map_err(|error| ModelError::Checkpoint(error.to_string()))?;
        encode_snapshot(&data)
    }

    /// Restores an exact binary checkpoint package into this engine.
    pub fn restore(&mut self, package: &CheckpointPackage) -> Result<()> {
        let data = decode_snapshot(package)?;
        self.solver
            .restore_data(&data)
            .map_err(|error| ModelError::Checkpoint(error.to_string()))?;
        self.observed_steps = data.accepted_steps;
        Ok(())
    }

    /// Returns the accepted elapsed time in seconds.
    pub fn time_s(&self) -> f64 {
        self.solver.time()
    }

    /// Returns the cumulative water budget under the solver's configured tolerances.
    pub fn water_budget(&self) -> WaterBudget {
        map_water_budget(self.solver.ledger(), self.solver.config())
    }

    pub(crate) fn domain(&self) -> &Domain {
        self.solver.domain()
    }

    pub(crate) fn current_cells(&self) -> Result<Vec<hydr::model::CellState>> {
        self.solver
            .state()
            .map(|flow| flow.cells().to_vec())
            .map_err(|error| ModelError::Engine(error.to_string()))
    }

    pub(crate) fn advance_raw_steps<F>(
        &mut self,
        target_s: f64,
        budget: StepBudget,
        token: &CancellationToken,
        mut on_step: F,
    ) -> Result<AdvanceSummary>
    where
        F: FnMut(f64, &[hydr::model::CellState]),
    {
        let mut step_counter = self.observed_steps;
        let report = self
            .solver
            .advance_controlled(
                target_s,
                budget.max_steps,
                || token.is_cancelled(),
                |step, cells| {
                    step_counter += 1;
                    on_step(step.end, cells);
                },
            )
            .map_err(|error| ModelError::Engine(error.to_string()))?;
        self.observed_steps = step_counter;
        Ok(map_advance_report(report))
    }

    /// Queries the containing cell at projected `(x_m, y_m)` without interpolation.
    pub fn cell_at(&self, x_m: f64, y_m: f64) -> Result<CellSample> {
        let sample = self
            .solver
            .cell_at(x_m, y_m)
            .map_err(|error| ModelError::Engine(error.to_string()))?;
        Ok(match sample {
            CellResult::Unsupported => CellSample::Unsupported,
            CellResult::Dry => CellSample::Dry,
            CellResult::Wet {
                depth,
                elevation,
                velocity,
                ..
            } => CellSample::Wet {
                depth_m: depth,
                water_elevation_m: elevation,
                velocity_x_m_s: velocity[0],
                velocity_y_m_s: velocity[1],
            },
        })
    }
}

fn locate_active_cell(domain: &Domain, x_m: f64, y_m: f64) -> Option<usize> {
    let [ox, oy] = domain.reference().origin();
    let (dx, dy) = domain.grid().spacing();
    let (nx, ny) = domain.grid().dimensions();
    let column = (x_m - ox) / dx;
    let row = (y_m - oy) / dy;
    if !column.is_finite()
        || !row.is_finite()
        || column < 0.0
        || row < 0.0
        || column >= nx as f64
        || row >= ny as f64
    {
        return None;
    }
    let index = row.floor() as usize * nx + column.floor() as usize;
    domain
        .active()
        .get(index)
        .copied()
        .unwrap_or(false)
        .then_some(index)
}

fn sample_from_cell(domain: &Domain, index: usize, cell: CellState) -> CellSample {
    if cell.h == 0.0 {
        return CellSample::Dry;
    }
    let bed = domain.bed().get(index).copied().unwrap_or(0.0);
    CellSample::Wet {
        depth_m: cell.h,
        water_elevation_m: bed + cell.h,
        velocity_x_m_s: cell.qx / cell.h,
        velocity_y_m_s: cell.qy / cell.h,
    }
}

fn map_advance_report(report: AdvanceReport) -> AdvanceSummary {
    let outcome = match report.status {
        AdvanceStatus::ReachedTarget => AdvanceOutcome::ReachedTarget,
        AdvanceStatus::Cancelled => AdvanceOutcome::Cancelled,
        AdvanceStatus::StepLimit => AdvanceOutcome::StepBudgetExhausted,
    };
    AdvanceSummary {
        outcome,
        accepted_time_s: report.accepted_time,
        steps_taken: report.accepted_steps,
    }
}

fn map_water_budget(ledger: &WaterLedger, config: &SolverConfig) -> WaterBudget {
    let scale = [
        ledger.storage,
        ledger.rain,
        ledger.withdrawal,
        ledger.infiltration,
        ledger.evaporation,
        ledger.water_adjustment.abs(),
        ledger.boundary_in,
        ledger.boundary_out,
    ]
    .into_iter()
    .fold(ledger.initial_storage.max(1.0), f64::max);

    WaterBudget {
        storage_change_m3: ledger.storage - ledger.initial_storage,
        inputs_m3: ledger.rain + ledger.boundary_in,
        outputs_m3: ledger.boundary_out
            + ledger.withdrawal
            + ledger.infiltration
            + ledger.evaporation,
        adjustment_m3: ledger.water_adjustment,
        residual_m3: ledger.residual,
        residual_bound_m3: config.volume_absolute + config.volume_relative * scale,
        adjustment_used_m3: ledger.water_adjustment_abs_bound,
        adjustment_limit_m3: config.max_cumulative_water_adjustment_m3,
    }
}
