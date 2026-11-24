//! Constrained mitigation screening and non-dominated Pareto front selection.

use arus_scenarios::PairedComparisonReport;
use serde::{Deserialize, Serialize};

/// Typed errors returned by mitigation screening.
#[derive(Debug, thiserror::Error)]
pub enum MitigationError {
    #[error("invalid mitigation constraint or candidate: {0}")]
    InvalidInput(String),
}

/// Result alias for optimisation screening operations.
pub type Result<T> = std::result::Result<T, MitigationError>;

/// Hard feasibility constraints applied before Pareto ranking.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MitigationConstraints {
    /// Maximum capital cost in MYR.
    pub max_cost_myr: f64,
    /// Maximum permitted peak-depth worsening at any transferred-impact location, in metres.
    pub max_allowed_worsening_m: f64,
}

/// Candidate mitigation intervention paired against a common baseline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MitigationCandidate {
    pub candidate_id: String,
    pub cost_myr: f64,
    pub comparison: PairedComparisonReport,
}

/// Feasible candidate summary scored for Pareto ranking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MitigationScore {
    pub candidate_id: String,
    pub cost_myr: f64,
    pub total_peak_reduction_m: f64,
    pub max_worsening_m: f64,
    pub requires_full_model_check: bool,
}

/// Infeasible candidate with explicit rejection reasons.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectedCandidate {
    pub candidate_id: String,
    pub reasons: Vec<String>,
}

/// Advisory screening report separating feasible, Pareto-optimal, and rejected candidates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreeningReport {
    pub pareto_front: Vec<MitigationScore>,
    pub feasible: Vec<MitigationScore>,
    pub rejected: Vec<RejectedCandidate>,
}

/// Filters candidates by conservation and feasibility constraints, then extracts the
/// non-dominated Pareto front minimizing `cost_myr` and maximizing `total_peak_reduction_m`.
pub fn screen_candidates(
    candidates: &[MitigationCandidate],
    constraints: MitigationConstraints,
) -> Result<ScreeningReport> {
    if !constraints.max_cost_myr.is_finite()
        || constraints.max_cost_myr < 0.0
        || !constraints.max_allowed_worsening_m.is_finite()
        || constraints.max_allowed_worsening_m < 0.0
    {
        return Err(MitigationError::InvalidInput(format!(
            "constraints={constraints:?}: require finite nonnegative limits"
        )));
    }

    let mut feasible = Vec::new();
    let mut rejected = Vec::new();

    for candidate in candidates {
        if !candidate.cost_myr.is_finite() || candidate.cost_myr < 0.0 {
            return Err(MitigationError::InvalidInput(format!(
                "candidate {} cost_myr={} must be finite nonnegative",
                candidate.candidate_id, candidate.cost_myr
            )));
        }
        let mut reasons = Vec::new();
        if !candidate.comparison.conservation_valid {
            reasons.push("failed-conservation-check".to_owned());
        }
        if candidate.cost_myr > constraints.max_cost_myr {
            reasons.push(format!(
                "cost {} MYR exceeds budget {} MYR",
                candidate.cost_myr, constraints.max_cost_myr
            ));
        }
        let max_worsening_m = candidate
            .comparison
            .worsened_locations
            .iter()
            .map(|d| d.delta_m)
            .fold(0.0_f64, f64::max);
        if max_worsening_m > constraints.max_allowed_worsening_m {
            reasons.push(format!(
                "downstream worsening {max_worsening_m} m exceeds limit {} m",
                constraints.max_allowed_worsening_m
            ));
        }
        let total_peak_reduction_m: f64 = candidate
            .comparison
            .improved_locations
            .iter()
            .map(|d| -d.delta_m)
            .sum();

        if !reasons.is_empty() {
            rejected.push(RejectedCandidate {
                candidate_id: candidate.candidate_id.clone(),
                reasons,
            });
            continue;
        }

        feasible.push(MitigationScore {
            candidate_id: candidate.candidate_id.clone(),
            cost_myr: candidate.cost_myr,
            total_peak_reduction_m,
            max_worsening_m,
            requires_full_model_check: true,
        });
    }

    let mut pareto_front = Vec::new();
    for (i, a) in feasible.iter().enumerate() {
        let dominated = feasible.iter().enumerate().any(|(j, b)| {
            if i == j {
                return false;
            }
            let no_worse =
                b.cost_myr <= a.cost_myr && b.total_peak_reduction_m >= a.total_peak_reduction_m;
            let strictly_better =
                b.cost_myr < a.cost_myr || b.total_peak_reduction_m > a.total_peak_reduction_m;
            no_worse && strictly_better
        });
        if !dominated {
            pareto_front.push(a.clone());
        }
    }

    Ok(ScreeningReport {
        pareto_front,
        feasible,
        rejected,
    })
}
