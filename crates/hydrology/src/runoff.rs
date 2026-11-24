//! Deterministic CPU `F64` subcatchment runoff adapter over `hydr::runoff::SubcatchmentBatch`.

use arus_model_api::{ModelError, Result};
use hydr::runoff::{
    SubcatchmentBatch, SubcatchmentConfig, SubcatchmentDefinition, SubcatchmentInput,
    SubcatchmentSnapshotData, SubcatchmentState,
};
use numr::runtime::cpu::{CpuClient, CpuDevice, CpuRuntime, ParallelismConfig};
use serde::{Deserialize, Serialize};

/// Per-step water accounting summary for one accepted subcatchment interval.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SubcatchmentStepSummary {
    pub start_s: f64,
    pub end_s: f64,
    pub rain_m3: f64,
    pub infiltration_m3: f64,
    pub evaporation_m3: f64,
    pub outlet_m3: f64,
    pub residual_m3: f64,
}

/// Deterministic single-threaded CPU `F64` subcatchment runoff engine.
pub struct SubcatchmentEngine {
    batch: SubcatchmentBatch<CpuRuntime>,
}

impl SubcatchmentEngine {
    /// Checks definitions and initial states and builds the CPU `F64` subcatchment batch.
    pub fn new(
        definitions: Vec<SubcatchmentDefinition>,
        states: Vec<SubcatchmentState>,
        initial_time_s: f64,
        config: SubcatchmentConfig,
    ) -> Result<Self> {
        let client = CpuClient::new(CpuDevice::new())
            .with_parallelism(ParallelismConfig::new(Some(1), None));
        let batch = SubcatchmentBatch::new(client, definitions, states, initial_time_s, config)
            .map_err(|error| ModelError::Engine(error.to_string()))?;
        Ok(Self { batch })
    }

    /// Evaluates one trial interval to `end_time_s` and commits atomically on success.
    ///
    /// On error, accepted state remains unchanged.
    pub fn step_to(
        &mut self,
        end_time_s: f64,
        inputs: &[SubcatchmentInput],
    ) -> Result<SubcatchmentStepSummary> {
        let start_s = self.batch.time();
        let trial = self
            .batch
            .trial_to(end_time_s, inputs)
            .map_err(|error| ModelError::Engine(error.to_string()))?;
        let diag = trial
            .diagnostics()
            .map_err(|error| ModelError::Engine(error.to_string()))?;
        self.batch = trial.into_next();
        Ok(SubcatchmentStepSummary {
            start_s,
            end_s: self.batch.time(),
            rain_m3: diag.applied_rain_volume_m3,
            infiltration_m3: diag.infiltration_volume_m3,
            evaporation_m3: diag.evaporation_volume_m3,
            outlet_m3: diag.outlet_volume_m3,
            residual_m3: diag.water_residual_m3,
        })
    }

    /// Returns the accepted elapsed time in seconds.
    pub fn time_s(&self) -> f64 {
        self.batch.time()
    }

    /// Exports the typed checkpoint data for this subcatchment batch.
    pub fn snapshot_data(&self) -> Result<SubcatchmentSnapshotData> {
        self.batch
            .snapshot()
            .data()
            .map_err(|error| ModelError::Checkpoint(error.to_string()))
    }

    /// Restores a compatible subcatchment checkpoint package.
    pub fn restore_data(&mut self, data: &SubcatchmentSnapshotData) -> Result<()> {
        self.batch
            .restore_data(data)
            .map_err(|error| ModelError::Checkpoint(error.to_string()))
    }
}
