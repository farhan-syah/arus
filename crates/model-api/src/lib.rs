//! Backend-agnostic capability declarations, execution results, water budgets, and checkpoints.
//!
//! Numerical engine types stay behind adapter crates such as `arus-hydraulics`.

use arus_assets::{Archive, BlobRef};
use serde::{Deserialize, Serialize};

/// Typed errors returned across the model boundary.
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("capability refused: {0}")]
    CapabilityRefused(String),
    #[error("invalid model input: {0}")]
    InvalidInput(String),
    #[error("numerical engine error: {0}")]
    Engine(String),
    #[error("checkpoint error: {0}")]
    Checkpoint(String),
    #[error("asset archive error: {0}")]
    Archive(#[from] arus_assets::Error),
}

/// Result alias for model boundary operations.
pub type Result<T> = std::result::Result<T, ModelError>;

/// Support status for one execution runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSupportSummary {
    pub runtime: String,
    pub status: String,
}

/// Declared scope limit with a stable identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityLimitSummary {
    pub id: String,
    pub summary: String,
}

/// Stable machine-readable capability summary exported by a numerical backend component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendCapabilities {
    pub backend: String,
    pub component: String,
    pub contract: String,
    pub snapshot_schema: String,
    pub precision: Vec<String>,
    pub runtimes: Vec<RuntimeSupportSummary>,
    pub boundaries: Vec<String>,
    pub sources: Vec<String>,
    pub limits: Vec<CapabilityLimitSummary>,
}

/// Completion outcome of an advancement request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdvanceOutcome {
    ReachedTarget,
    Cancelled,
    StepBudgetExhausted,
}

/// Summary of one advancement call.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AdvanceSummary {
    pub outcome: AdvanceOutcome,
    pub accepted_time_s: f64,
    pub steps_taken: usize,
}

/// Summary of one accepted numerical step.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StepSummary {
    pub step: usize,
    pub start_s: f64,
    pub end_s: f64,
    pub residual_m3: f64,
    pub max_depth_m: f64,
}

/// Cumulative water budget of an accepted numerical run, in cubic metres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WaterBudget {
    pub storage_change_m3: f64,
    pub inputs_m3: f64,
    pub outputs_m3: f64,
    pub adjustment_m3: f64,
    pub residual_m3: f64,
    pub residual_bound_m3: f64,
    pub adjustment_used_m3: f64,
    pub adjustment_limit_m3: f64,
}

impl WaterBudget {
    /// Returns `true` when the absolute residual stays within the declared bound.
    pub fn within_residual_bound(self) -> bool {
        self.residual_m3.is_finite()
            && self.residual_bound_m3.is_finite()
            && self.residual_m3.abs() <= self.residual_bound_m3
    }

    /// Returns `true` when cumulative numerical adjustment stays within its declared limit.
    pub fn within_adjustment_limit(self) -> bool {
        self.adjustment_used_m3.is_finite()
            && self.adjustment_limit_m3.is_finite()
            && self.adjustment_used_m3 <= self.adjustment_limit_m3
    }
}

/// Cell-supported physical output at an accepted time without spatial interpolation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CellSample {
    /// Outside the grid or inside an excluded cell. Never means dry zero.
    Unsupported,
    /// Exactly zero depth. Elevation and velocity remain undefined.
    Dry,
    /// Positive represented water depth.
    Wet {
        depth_m: f64,
        water_elevation_m: f64,
        velocity_x_m_s: f64,
        velocity_y_m_s: f64,
    },
}

/// Metadata header persisted alongside an opaque binary checkpoint payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckpointHeader {
    pub schema: String,
    pub contract: String,
    pub accepted_time_s: f64,
    pub accepted_steps: usize,
    pub forcing_prefix_intervals: usize,
    pub forcing_prefix_digest_hex: String,
}

/// Opaque exact binary checkpoint package owned by the application.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckpointPackage {
    pub header: CheckpointHeader,
    pub payload: Vec<u8>,
}

/// Content-addressed checkpoint reference stored in an immutable asset archive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersistedCheckpoint {
    pub header: CheckpointHeader,
    pub blob: BlobRef,
}

impl CheckpointPackage {
    /// Stores the exact binary checkpoint payload in `archive` as a content-addressed blob.
    pub fn persist(&self, archive: &Archive) -> Result<PersistedCheckpoint> {
        let blob = archive.put_blob(&self.payload)?;
        Ok(PersistedCheckpoint {
            header: self.header.clone(),
            blob,
        })
    }

    /// Reloads and verifies a persisted checkpoint payload from `archive`.
    pub fn load(archive: &Archive, persisted: &PersistedCheckpoint) -> Result<Self> {
        let payload = archive.read_blob(&persisted.blob.sha256)?;
        if payload.len() as u64 != persisted.blob.bytes {
            return Err(ModelError::Checkpoint(format!(
                "checkpoint blob length mismatch: expected {} bytes, read {}",
                persisted.blob.bytes,
                payload.len()
            )));
        }
        Ok(Self {
            header: persisted.header.clone(),
            payload,
        })
    }
}
