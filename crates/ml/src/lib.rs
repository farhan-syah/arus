//! Optional numerical ML baseline-improvement gate.
//!
//! No language model is required. Any optional numerical model must beat the physics
//! baseline on independent locked test events before becoming eligible.

use serde::{Deserialize, Serialize};

/// Typed errors returned by ML eligibility checks.
#[derive(Debug, thiserror::Error)]
pub enum MlError {
    #[error("invalid ML gate input: {0}")]
    InvalidInput(String),
}

/// Result alias for ML gate checks.
pub type Result<T> = std::result::Result<T, MlError>;

/// Thresholds required for an optional numerical ML model to qualify over the baseline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MlGateConfig {
    /// Minimum required RMSE reduction in metres on locked test events.
    pub min_rmse_improvement_m: f64,
    /// Minimum number of independent locked test events.
    pub min_locked_events: usize,
}

/// Paired baseline vs ML evaluation evidence on locked test events.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BaselineComparison {
    pub locked_event_count: usize,
    pub baseline_rmse_m: f64,
    pub ml_rmse_m: f64,
}

/// Eligibility verdict for an optional numerical ML model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum MlEligibility {
    Eligible,
    Rejected { reasons: Vec<String> },
}

/// Evaluates whether an optional numerical ML model meets independent-event improvement gates.
pub fn evaluate_ml_eligibility(
    evidence: BaselineComparison,
    config: MlGateConfig,
) -> Result<MlEligibility> {
    if !evidence.baseline_rmse_m.is_finite()
        || evidence.baseline_rmse_m < 0.0
        || !evidence.ml_rmse_m.is_finite()
        || evidence.ml_rmse_m < 0.0
        || !config.min_rmse_improvement_m.is_finite()
        || config.min_rmse_improvement_m < 0.0
    {
        return Err(MlError::InvalidInput(
            "RMSE values and improvement thresholds must be finite nonnegative".to_owned(),
        ));
    }

    let mut reasons = Vec::new();
    if evidence.locked_event_count < config.min_locked_events {
        reasons.push(format!(
            "locked events {} below minimum {}",
            evidence.locked_event_count, config.min_locked_events
        ));
    }
    let improvement = evidence.baseline_rmse_m - evidence.ml_rmse_m;
    if improvement < config.min_rmse_improvement_m {
        reasons.push(format!(
            "RMSE improvement {improvement} m below required {} m",
            config.min_rmse_improvement_m
        ));
    }

    if reasons.is_empty() {
        Ok(MlEligibility::Eligible)
    } else {
        Ok(MlEligibility::Rejected { reasons })
    }
}
