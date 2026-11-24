//! Coupled 2D surface + 1D channel/pool coordinator adapter over `hydr::coupling::SurfaceChannelBatch`.

use arus_jobs::{CancellationToken, StepBudget};
use arus_model_api::{AdvanceOutcome, AdvanceSummary, CellSample, ModelError, Result, WaterBudget};
use hydr::coupling::{
    SurfaceChannelBatch, SurfaceChannelForcingInterval, SurfaceChannelInitialState,
    SurfaceChannelLinkLedger, SurfaceChannelSetup, SurfaceChannelWaterBalance,
};
use hydr::solver::CellResult;
use serde::{Deserialize, Serialize};

use crate::runtime::{ReferenceRuntime, reference_cpu_client};

/// Lifetime directional bank-weir exchange through one surface-channel link.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BankExchangeSummary {
    pub link_id: String,
    pub surface_to_channel_m3: f64,
    pub channel_to_surface_m3: f64,
    pub roundoff_bound_m3: f64,
}

/// Deterministic CPU `F64` coupled 2D surface + 1D channel engine wrapping `SurfaceChannelBatch`.
pub struct CoupledSurfaceChannelEngine {
    batch: SurfaceChannelBatch<ReferenceRuntime>,
}

impl CoupledSurfaceChannelEngine {
    /// Checks `setup` and `initial` and builds every child on the CPU `F64` reference client.
    pub fn new(setup: SurfaceChannelSetup, initial: SurfaceChannelInitialState) -> Result<Self> {
        let batch = SurfaceChannelBatch::new(reference_cpu_client(), setup, initial)
            .map_err(|error| ModelError::Engine(error.to_string()))?;
        Ok(Self { batch })
    }

    /// Advances the coupled system toward `target_s`, committing each accepted common interval
    /// atomically and checking `token` between steps.
    pub fn advance_to<F>(
        &mut self,
        target_s: f64,
        budget: StepBudget,
        token: &CancellationToken,
        mut forcing_for: F,
    ) -> Result<AdvanceSummary>
    where
        F: FnMut(f64, f64) -> SurfaceChannelForcingInterval,
    {
        if !target_s.is_finite() || target_s < self.batch.time() {
            return Err(ModelError::InvalidInput(format!(
                "target_s={target_s} is invalid for current time {} s",
                self.batch.time()
            )));
        }
        let mut steps_taken = 0_usize;
        while self.batch.time() < target_s {
            if token.is_cancelled() {
                return Ok(AdvanceSummary {
                    outcome: AdvanceOutcome::Cancelled,
                    accepted_time_s: self.batch.time(),
                    steps_taken,
                });
            }
            if steps_taken >= budget.max_steps {
                return Ok(AdvanceSummary {
                    outcome: AdvanceOutcome::StepBudgetExhausted,
                    accepted_time_s: self.batch.time(),
                    steps_taken,
                });
            }
            let start_s = self.batch.time();
            let forcing = forcing_for(start_s, target_s);
            let trial = self
                .batch
                .trial_to(target_s, &forcing)
                .map_err(|error| ModelError::Engine(error.to_string()))?;
            self.batch = trial.into_next();
            steps_taken += 1;
        }
        Ok(AdvanceSummary {
            outcome: AdvanceOutcome::ReachedTarget,
            accepted_time_s: self.batch.time(),
            steps_taken,
        })
    }

    /// Returns the common accepted elapsed time in seconds.
    pub fn time_s(&self) -> f64 {
        self.batch.time()
    }

    /// Returns total accepted common intervals.
    pub fn accepted_steps(&self) -> usize {
        self.batch.accepted_steps()
    }

    /// Projects the composite system water budget across surface, reaches, pools, and runoff.
    pub fn water_budget(&self) -> Result<WaterBudget> {
        let balance = self
            .batch
            .ledger()
            .map_err(|error| ModelError::Engine(error.to_string()))?;
        Ok(map_coupled_balance(&balance, &self.batch))
    }

    /// Projects lifetime directional exchange totals for each bank link.
    pub fn link_ledgers(&self) -> Result<Vec<BankExchangeSummary>> {
        let ledgers = self
            .batch
            .link_ledgers()
            .map_err(|error| ModelError::Engine(error.to_string()))?;
        Ok(ledgers.into_iter().map(map_link_ledger).collect())
    }

    /// Queries the coupled 2D surface cell at projected `(x_m, y_m)` without interpolation.
    pub fn surface_cell_at(&self, x_m: f64, y_m: f64) -> Result<CellSample> {
        let sample = self
            .batch
            .surface()
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

fn map_link_ledger(ledger: SurfaceChannelLinkLedger) -> BankExchangeSummary {
    BankExchangeSummary {
        link_id: ledger.link_id,
        surface_to_channel_m3: ledger.surface_to_channel_m3,
        channel_to_surface_m3: ledger.channel_to_surface_m3,
        roundoff_bound_m3: ledger.roundoff_bound_m3,
    }
}

fn map_coupled_balance(
    balance: &SurfaceChannelWaterBalance,
    batch: &SurfaceChannelBatch<ReferenceRuntime>,
) -> WaterBudget {
    let surface_ledger = batch.surface().ledger();
    let surface_config = batch.surface().config();
    WaterBudget {
        storage_change_m3: balance.storage_m3 - balance.initial_storage_m3,
        inputs_m3: balance.external_in_m3,
        outputs_m3: balance.external_out_m3,
        adjustment_m3: balance.water_adjustment_m3,
        residual_m3: balance.residual_m3,
        residual_bound_m3: balance.bound_m3,
        adjustment_used_m3: surface_ledger.water_adjustment_abs_bound,
        adjustment_limit_m3: surface_config.max_cumulative_water_adjustment_m3,
    }
}
