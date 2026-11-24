//! Paired baseline-vs-intervention scenario comparison with matched-assumption checks.

use std::collections::BTreeMap;

use arus_model_api::WaterBudget;
use serde::{Deserialize, Serialize};

/// Typed errors returned by scenario validation and comparison.
#[derive(Debug, thiserror::Error)]
pub enum ScenarioError {
    #[error("unmatched paired scenario assumptions: {0}")]
    UnmatchedAssumptions(String),
    #[error("invalid scenario summary: {0}")]
    InvalidSummary(String),
}

/// Result alias for scenario operations.
pub type Result<T> = std::result::Result<T, ScenarioError>;

/// Immutable conditional scenario manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioManifest {
    pub scenario_id: String,
    pub baseline_id: String,
    pub forcing_digest: String,
    pub initial_state_digest: String,
    pub is_screening: bool,
    pub assumptions: Vec<String>,
    pub edits: Vec<String>,
}

/// Summary of one scenario run used for paired comparison.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenarioRunSummary {
    pub manifest: ScenarioManifest,
    pub budget: WaterBudget,
    /// Peak depth in metres per supported location ID.
    pub location_peak_depths_m: BTreeMap<String, f64>,
}

/// Peak-depth change at one location (`intervention - baseline`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocationDelta {
    pub location_id: String,
    pub baseline_peak_m: f64,
    pub intervention_peak_m: f64,
    pub delta_m: f64,
}

/// Paired comparison report separating improved, unchanged, and worsened (transferred-impact) sites.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairedComparisonReport {
    pub baseline_id: String,
    pub intervention_id: String,
    pub is_screening: bool,
    pub conservation_valid: bool,
    pub improved_locations: Vec<LocationDelta>,
    pub unchanged_locations: Vec<LocationDelta>,
    pub worsened_locations: Vec<LocationDelta>,
}

/// Compares a baseline run against an intervention run under matched forcing and initial state.
pub fn compare_paired_runs(
    baseline: &ScenarioRunSummary,
    intervention: &ScenarioRunSummary,
) -> Result<PairedComparisonReport> {
    if baseline.manifest.forcing_digest != intervention.manifest.forcing_digest {
        return Err(ScenarioError::UnmatchedAssumptions(format!(
            "forcing_digest mismatch: baseline={} vs intervention={}",
            baseline.manifest.forcing_digest, intervention.manifest.forcing_digest
        )));
    }
    if baseline.manifest.initial_state_digest != intervention.manifest.initial_state_digest {
        return Err(ScenarioError::UnmatchedAssumptions(format!(
            "initial_state_digest mismatch: baseline={} vs intervention={}",
            baseline.manifest.initial_state_digest, intervention.manifest.initial_state_digest
        )));
    }
    if baseline.location_peak_depths_m.len() != intervention.location_peak_depths_m.len()
        || !baseline
            .location_peak_depths_m
            .keys()
            .all(|k| intervention.location_peak_depths_m.contains_key(k))
    {
        return Err(ScenarioError::InvalidSummary(
            "baseline and intervention must evaluate the exact same location set".to_owned(),
        ));
    }

    let conservation_valid = baseline.budget.within_residual_bound()
        && baseline.budget.within_adjustment_limit()
        && intervention.budget.within_residual_bound()
        && intervention.budget.within_adjustment_limit();

    let mut improved_locations = Vec::new();
    let mut unchanged_locations = Vec::new();
    let mut worsened_locations = Vec::new();

    for (location_id, &baseline_peak_m) in &baseline.location_peak_depths_m {
        let Some(&intervention_peak_m) = intervention.location_peak_depths_m.get(location_id)
        else {
            return Err(ScenarioError::InvalidSummary(format!(
                "missing location {location_id} in intervention summary"
            )));
        };
        if !baseline_peak_m.is_finite() || !intervention_peak_m.is_finite() {
            return Err(ScenarioError::InvalidSummary(format!(
                "non-finite peak depth at {location_id}"
            )));
        }
        let delta_m = intervention_peak_m - baseline_peak_m;
        let entry = LocationDelta {
            location_id: location_id.clone(),
            baseline_peak_m,
            intervention_peak_m,
            delta_m,
        };
        if delta_m > 0.0 {
            worsened_locations.push(entry);
        } else if delta_m < 0.0 {
            improved_locations.push(entry);
        } else {
            unchanged_locations.push(entry);
        }
    }

    Ok(PairedComparisonReport {
        baseline_id: baseline.manifest.scenario_id.clone(),
        intervention_id: intervention.manifest.scenario_id.clone(),
        is_screening: baseline.manifest.is_screening || intervention.manifest.is_screening,
        conservation_valid,
        improved_locations,
        unchanged_locations,
        worsened_locations,
    })
}
