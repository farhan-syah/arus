//! Censored timing metrics, depth error scores, and numerical budget assessment.

use arus_model_api::WaterBudget;
use serde::{Deserialize, Serialize};

/// Typed errors returned by evaluation scoring functions.
#[derive(Debug, thiserror::Error)]
pub enum EvaluationError {
    #[error("invalid evaluation input: {0}")]
    InvalidInput(String),
}

/// Result alias for evaluation operations.
pub type Result<T> = std::result::Result<T, EvaluationError>;

/// Event partition enforcing separation between parameter tuning and final evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvaluationPartition {
    Calibration,
    Development,
    LockedTest,
}

/// Assessment mode preventing retrospective reconstructions from mixing with forecast replays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvaluationMode {
    Reconstruction,
    AsIssuedReplay,
}

/// Time representation supporting exact, interval-censored, left/right-censored, and missing values.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CensoredTime {
    Exact { time_s: f64 },
    Interval { start_s: f64, end_s: f64 },
    LeftCensored { upper_bound_s: f64 },
    RightCensored { lower_bound_s: f64 },
    Missing,
}

/// Outcome of scoring a predicted crossing time against a possibly censored observation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TimingScore {
    /// Exact predicted time minus exact observed time in seconds.
    ExactError { signed_error_s: f64 },
    /// Distance outside an observed interval `[start_s, end_s]`; zero inside the interval.
    IntervalDistance {
        signed_distance_s: f64,
        interval_width_s: f64,
    },
    /// Predicted time satisfies the censoring bound without inventing an exact error.
    ConsistentWithCensoring,
    /// Predicted time violates the censoring bound by `signed_violation_s`.
    ViolatesCensoring { signed_violation_s: f64 },
    /// Timing cannot be scored cleanly due to missing or mutually unresolved censoring.
    Unscorable { reason: &'static str },
}

/// Scores a predicted crossing time against an observed (possibly censored) crossing time.
pub fn score_crossing_time(predicted: CensoredTime, observed: CensoredTime) -> Result<TimingScore> {
    check_censored_time("predicted", predicted)?;
    check_censored_time("observed", observed)?;

    let score = match (predicted, observed) {
        (_, CensoredTime::Missing) => TimingScore::Unscorable {
            reason: "observation-missing",
        },
        (CensoredTime::Missing, _) => TimingScore::Unscorable {
            reason: "prediction-missing",
        },
        (CensoredTime::Exact { time_s: p }, CensoredTime::Exact { time_s: o }) => {
            TimingScore::ExactError {
                signed_error_s: p - o,
            }
        }
        (
            CensoredTime::Exact { time_s: p },
            CensoredTime::Interval {
                start_s: a,
                end_s: b,
            },
        ) => {
            let signed_distance_s = if p < a {
                p - a
            } else if p > b {
                p - b
            } else {
                0.0
            };
            TimingScore::IntervalDistance {
                signed_distance_s,
                interval_width_s: b - a,
            }
        }
        (CensoredTime::Exact { time_s: p }, CensoredTime::LeftCensored { upper_bound_s: u }) => {
            if p <= u {
                TimingScore::ConsistentWithCensoring
            } else {
                TimingScore::ViolatesCensoring {
                    signed_violation_s: p - u,
                }
            }
        }
        (CensoredTime::Exact { time_s: p }, CensoredTime::RightCensored { lower_bound_s: l }) => {
            if p >= l {
                TimingScore::ConsistentWithCensoring
            } else {
                TimingScore::ViolatesCensoring {
                    signed_violation_s: p - l,
                }
            }
        }
        (
            CensoredTime::LeftCensored { upper_bound_s: p_u },
            CensoredTime::LeftCensored { upper_bound_s: o_u },
        ) => {
            if p_u <= o_u {
                TimingScore::ConsistentWithCensoring
            } else {
                TimingScore::Unscorable {
                    reason: "both-left-censored",
                }
            }
        }
        (
            CensoredTime::RightCensored { lower_bound_s: p_l },
            CensoredTime::RightCensored { lower_bound_s: o_l },
        ) => {
            if p_l >= o_l {
                TimingScore::ConsistentWithCensoring
            } else {
                TimingScore::Unscorable {
                    reason: "both-right-censored",
                }
            }
        }
        (CensoredTime::RightCensored { lower_bound_s: p_l }, CensoredTime::Exact { time_s: o }) => {
            if o < p_l {
                TimingScore::ViolatesCensoring {
                    signed_violation_s: p_l - o,
                }
            } else {
                TimingScore::Unscorable {
                    reason: "prediction-right-censored",
                }
            }
        }
        _ => TimingScore::Unscorable {
            reason: "incompatible-censoring",
        },
    };
    Ok(score)
}

fn check_censored_time(label: &str, value: CensoredTime) -> Result<()> {
    match value {
        CensoredTime::Exact { time_s } => {
            if !time_s.is_finite() {
                return Err(EvaluationError::InvalidInput(format!(
                    "{label} exact time {time_s} is non-finite"
                )));
            }
        }
        CensoredTime::Interval { start_s, end_s } => {
            if !start_s.is_finite() || !end_s.is_finite() || end_s < start_s {
                return Err(EvaluationError::InvalidInput(format!(
                    "{label} interval [{start_s}, {end_s}] is invalid"
                )));
            }
        }
        CensoredTime::LeftCensored { upper_bound_s } => {
            if !upper_bound_s.is_finite() {
                return Err(EvaluationError::InvalidInput(format!(
                    "{label} left-censor bound {upper_bound_s} is non-finite"
                )));
            }
        }
        CensoredTime::RightCensored { lower_bound_s } => {
            if !lower_bound_s.is_finite() {
                return Err(EvaluationError::InvalidInput(format!(
                    "{label} right-censor bound {lower_bound_s} is non-finite"
                )));
            }
        }
        CensoredTime::Missing => {}
    }
    Ok(())
}

/// Summary error metrics over matched `(predicted_m, observed_m)` depth pairs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DepthSeriesMetrics {
    pub count: usize,
    pub bias_m: f64,
    pub mae_m: f64,
    pub rmse_m: f64,
    pub peak_depth_error_m: f64,
}

/// Computes bias, MAE, RMSE, and peak depth error over matched `(predicted_m, observed_m)` pairs.
pub fn score_depth_series(pairs: &[(f64, f64)]) -> Result<DepthSeriesMetrics> {
    if pairs.is_empty() {
        return Err(EvaluationError::InvalidInput(
            "cannot score an empty depth series".to_owned(),
        ));
    }
    let mut sum_err = 0.0_f64;
    let mut sum_abs = 0.0_f64;
    let mut sum_sq = 0.0_f64;
    let mut max_pred = f64::NEG_INFINITY;
    let mut max_obs = f64::NEG_INFINITY;

    for (index, &(pred, obs)) in pairs.iter().enumerate() {
        if !pred.is_finite() || !obs.is_finite() || pred < 0.0 || obs < 0.0 {
            return Err(EvaluationError::InvalidInput(format!(
                "pair[{index}]=({pred}, {obs}): require finite nonnegative depths in metres"
            )));
        }
        let err = pred - obs;
        sum_err += err;
        sum_abs += err.abs();
        sum_sq += err * err;
        max_pred = max_pred.max(pred);
        max_obs = max_obs.max(obs);
    }

    let n = pairs.len() as f64;
    Ok(DepthSeriesMetrics {
        count: pairs.len(),
        bias_m: sum_err / n,
        mae_m: sum_abs / n,
        rmse_m: (sum_sq / n).sqrt(),
        peak_depth_error_m: max_pred - max_obs,
    })
}

/// Numerical conservation assessment separate from worker execution completion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetAssessment {
    pub passed: bool,
    pub reasons: Vec<String>,
}

/// Evaluates whether a reported `WaterBudget` satisfies declared residual and adjustment bounds.
pub fn assess_water_budget(budget: WaterBudget) -> BudgetAssessment {
    let mut reasons = Vec::new();
    if !budget.within_residual_bound() {
        reasons.push(format!(
            "water residual {} m3 exceeds bound {} m3",
            budget.residual_m3, budget.residual_bound_m3
        ));
    }
    if !budget.within_adjustment_limit() {
        reasons.push(format!(
            "cumulative adjustment {} m3 exceeds limit {} m3",
            budget.adjustment_used_m3, budget.adjustment_limit_m3
        ));
    }
    BudgetAssessment {
        passed: reasons.is_empty(),
        reasons,
    }
}
