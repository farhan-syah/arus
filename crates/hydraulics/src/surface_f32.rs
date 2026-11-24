//! Opt-in `F32` 2D shallow-water screening adapter over `hydr::solver::SurfaceSolverF32`.
//!
//! Admits closed impermeable wall boundaries and distributed rain, evaporation, and withdrawal.
//! Reports an explicit `f32-screening` precision label so single-precision runs never substitute
//! for certified `F64` reference runs.

use arus_domain::RunClassification;
use arus_geo::TerrainGrid;
use arus_model_api::{ModelError, Result};
use hydr::model::{CellState, FlowState};
use hydr::solver::{
    SurfaceSolverF32, SurfaceSolverF32Config, SurfaceSolverF32Ledger,
    surface_solver_f32_capabilities,
};
use serde::{Deserialize, Serialize};

use crate::runtime::{ReferenceRuntime, reference_cpu_client};
use crate::surface::domain_from_terrain;

/// JSON-serializable manifest for an opt-in `F32` closed-basin surface screening run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceF32RunSpec {
    pub run_id: String,
    pub classification: RunClassification,
    pub terrain: TerrainGrid,
    pub uniform_manning_n: f64,
    pub initial_depth_m: f64,
    pub uniform_rain_m_per_s: f32,
    pub uniform_evap_m_per_s: f32,
    pub uniform_withdrawal_m_per_s: f32,
    pub target_time_s: f64,
    pub max_steps: usize,
}

/// Summary report emitted by an `F32` screening execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceF32RunReport {
    pub run_id: String,
    pub classification: RunClassification,
    pub contract: String,
    pub precision: String,
    pub accepted_time_s: f64,
    pub accepted_steps: u64,
    pub initial_storage_m3: f64,
    pub final_storage_m3: f64,
    pub rain_m3: f64,
    pub evaporation_m3: f64,
    pub withdrawal_m3: f64,
    pub residual_m3: f64,
    pub max_depth_m: f64,
}

/// Deterministic CPU `F32` 2D surface screening engine wrapping `SurfaceSolverF32`.
pub struct SurfaceEngineF32 {
    solver: SurfaceSolverF32<ReferenceRuntime>,
}

impl SurfaceEngineF32 {
    /// Constructs an `F32` surface engine from a checked `TerrainGrid` and initial depth.
    pub fn from_terrain(
        terrain: &TerrainGrid,
        uniform_manning_n: f64,
        initial_depth_m: f64,
    ) -> Result<Self> {
        if !initial_depth_m.is_finite() || initial_depth_m < 0.0 {
            return Err(ModelError::InvalidInput(format!(
                "initial_depth_m={initial_depth_m} must be finite nonnegative"
            )));
        }
        let n = terrain
            .grid()
            .cell_count()
            .ok_or_else(|| ModelError::InvalidInput("terrain cell count overflow".to_owned()))?;
        let domain = domain_from_terrain(terrain, vec![uniform_manning_n; n])?;
        let flow_cells: Vec<CellState> = domain
            .active()
            .iter()
            .map(|&is_active| CellState {
                h: if is_active { initial_depth_m } else { 0.0 },
                qx: 0.0,
                qy: 0.0,
            })
            .collect();
        let initial = FlowState::new(&domain, flow_cells)
            .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
        let solver = SurfaceSolverF32::new(
            reference_cpu_client(),
            domain,
            initial,
            SurfaceSolverF32Config::default(),
        )
        .map_err(|error| ModelError::Engine(error.to_string()))?;
        Ok(Self { solver })
    }

    /// Advances to `target_s` under uniform per-cell forcing rates.
    pub fn advance_uniform(
        &mut self,
        target_s: f64,
        rain_m_per_s: f32,
        evap_m_per_s: f32,
        withdrawal_m_per_s: f32,
        max_steps: usize,
    ) -> Result<usize> {
        let count = self.solver.domain().grid().cell_count();
        let rain = vec![rain_m_per_s; count];
        let evap = vec![evap_m_per_s; count];
        let withdrawal = vec![withdrawal_m_per_s; count];
        self.solver
            .advance_to(target_s, &rain, &evap, &withdrawal, max_steps)
            .map_err(|error| ModelError::Engine(error.to_string()))
    }

    /// Current accepted time in seconds.
    pub fn time_s(&self) -> f64 {
        self.solver.time_s()
    }

    /// Total accepted steps.
    pub fn accepted_steps(&self) -> u64 {
        self.solver.accepted_steps()
    }

    /// Current SI water-balance ledger.
    pub fn ledger(&self) -> SurfaceSolverF32Ledger {
        self.solver.ledger()
    }

    /// Maximum water depth across all cells in metres.
    pub fn max_depth_m(&self) -> Result<f64> {
        let states = self
            .solver
            .state()
            .map_err(|error| ModelError::Engine(error.to_string()))?;
        Ok(states
            .iter()
            .map(|c| c.h)
            .fold(0.0_f64, |acc, v| if v > acc { v } else { acc }))
    }
}

impl SurfaceF32RunSpec {
    /// Checks inputs and runs the `F32` screening simulation to `target_time_s`.
    pub fn execute(&self) -> Result<SurfaceF32RunReport> {
        let mut engine = SurfaceEngineF32::from_terrain(
            &self.terrain,
            self.uniform_manning_n,
            self.initial_depth_m,
        )?;
        engine.advance_uniform(
            self.target_time_s,
            self.uniform_rain_m_per_s,
            self.uniform_evap_m_per_s,
            self.uniform_withdrawal_m_per_s,
            self.max_steps,
        )?;
        let ledger = engine.ledger();
        let caps = surface_solver_f32_capabilities();
        Ok(SurfaceF32RunReport {
            run_id: self.run_id.clone(),
            classification: self.classification,
            contract: caps.contract.unwrap_or("hydr-swe-f32-numr-1").to_owned(),
            precision: "f32-screening".to_owned(),
            accepted_time_s: engine.time_s(),
            accepted_steps: engine.accepted_steps(),
            initial_storage_m3: ledger.initial_storage_m3,
            final_storage_m3: ledger.final_storage_m3,
            rain_m3: ledger.rain_m3,
            evaporation_m3: ledger.evaporation_m3,
            withdrawal_m3: ledger.withdrawal_m3,
            residual_m3: ledger.residual_m3,
            max_depth_m: engine.max_depth_m()?,
        })
    }
}
