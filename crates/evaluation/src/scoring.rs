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

/// Hydrological skill metrics over a simulated vs observed `(time_s, depth_m)` hydrograph.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HydrographSkillMetrics {
    pub count: usize,
    /// Nash-Sutcliffe Efficiency (`1.0` = perfect, `<= 0` = no better than observed mean).
    pub nse: f64,
    /// Kling-Gupta Efficiency (Gupta et al., 2009; `1.0` = perfect).
    pub kge: f64,
    /// Pearson correlation `r` between simulated and observed series.
    pub correlation_r: f64,
    /// Variability ratio `alpha = std(sim) / std(obs)`.
    pub variability_ratio_alpha: f64,
    /// Bias ratio `beta = mean(sim) / mean(obs)`.
    pub bias_ratio_beta: f64,
    pub rmse_m: f64,
    pub mae_m: f64,
    /// `max(sim) - max(obs)` in metres.
    pub peak_error_m: f64,
    /// `t_peak(sim) - t_peak(obs)` in seconds (positive = late peak).
    pub peak_timing_error_s: f64,
}

/// Computes NSE, KGE, RMSE, MAE, peak depth error, and peak timing error.
///
/// Predicted series `(time_s, depth_m)` is linearly interpolated onto each observed timestamp.
pub fn score_hydrograph(
    predicted: &[(f64, f64)],
    observed: &[(f64, f64)],
) -> Result<HydrographSkillMetrics> {
    if predicted.is_empty() || observed.is_empty() {
        return Err(EvaluationError::InvalidInput(
            "predicted and observed hydrographs must be non-empty".to_owned(),
        ));
    }
    validate_time_series("predicted", predicted)?;
    validate_time_series("observed", observed)?;

    let mut sim_vals = Vec::with_capacity(observed.len());
    let mut obs_vals = Vec::with_capacity(observed.len());

    let mut max_obs = f64::NEG_INFINITY;
    let mut t_peak_obs = observed[0].0;
    let mut max_sim = f64::NEG_INFINITY;
    let mut t_peak_sim = observed[0].0;

    for &(t_obs, d_obs) in observed {
        let d_sim = interpolate_series(predicted, t_obs);
        sim_vals.push(d_sim);
        obs_vals.push(d_obs);
        if d_obs > max_obs {
            max_obs = d_obs;
            t_peak_obs = t_obs;
        }
        if d_sim > max_sim {
            max_sim = d_sim;
            t_peak_sim = t_obs;
        }
    }

    let n = obs_vals.len() as f64;
    let mean_obs = obs_vals.iter().sum::<f64>() / n;
    let mean_sim = sim_vals.iter().sum::<f64>() / n;

    let mut sum_sq_err = 0.0_f64;
    let mut sum_abs_err = 0.0_f64;
    let mut ss_tot = 0.0_f64;
    let mut var_sim = 0.0_f64;
    let mut cov = 0.0_f64;

    for (&s, &o) in sim_vals.iter().zip(obs_vals.iter()) {
        let err = s - o;
        sum_sq_err += err * err;
        sum_abs_err += err.abs();
        let do_ = o - mean_obs;
        let ds = s - mean_sim;
        ss_tot += do_ * do_;
        var_sim += ds * ds;
        cov += ds * do_;
    }

    let rmse_m = (sum_sq_err / n).sqrt();
    let mae_m = sum_abs_err / n;
    let nse = if ss_tot > 1e-18 {
        1.0 - (sum_sq_err / ss_tot)
    } else if sum_sq_err <= 1e-18 {
        1.0
    } else {
        f64::NEG_INFINITY
    };

    let std_obs = (ss_tot / n).sqrt();
    let std_sim = (var_sim / n).sqrt();
    let correlation_r = if std_obs > 1e-15 && std_sim > 1e-15 {
        (cov / (n * std_obs * std_sim)).clamp(-1.0, 1.0)
    } else if (std_obs - std_sim).abs() <= 1e-15 {
        1.0
    } else {
        0.0
    };
    let variability_ratio_alpha = if std_obs > 1e-15 {
        std_sim / std_obs
    } else if std_sim <= 1e-15 {
        1.0
    } else {
        0.0
    };
    let bias_ratio_beta = if mean_obs.abs() > 1e-15 {
        mean_sim / mean_obs
    } else if mean_sim.abs() <= 1e-15 {
        1.0
    } else {
        0.0
    };

    let kge = 1.0
        - ((correlation_r - 1.0).powi(2)
            + (variability_ratio_alpha - 1.0).powi(2)
            + (bias_ratio_beta - 1.0).powi(2))
        .sqrt();

    Ok(HydrographSkillMetrics {
        count: obs_vals.len(),
        nse,
        kge,
        correlation_r,
        variability_ratio_alpha,
        bias_ratio_beta,
        rmse_m,
        mae_m,
        peak_error_m: max_sim - max_obs,
        peak_timing_error_s: t_peak_sim - t_peak_obs,
    })
}

fn validate_time_series(label: &str, series: &[(f64, f64)]) -> Result<()> {
    let mut prev_t = f64::NEG_INFINITY;
    for (idx, &(t, d)) in series.iter().enumerate() {
        if !t.is_finite() || !d.is_finite() || d < 0.0 {
            return Err(EvaluationError::InvalidInput(format!(
                "{label}[{idx}]=({t}, {d}) must have finite time and nonnegative depth"
            )));
        }
        if t < prev_t {
            return Err(EvaluationError::InvalidInput(format!(
                "{label}[{idx}] time {t} is earlier than previous time {prev_t}"
            )));
        }
        prev_t = t;
    }
    Ok(())
}

fn interpolate_series(series: &[(f64, f64)], t: f64) -> f64 {
    if t <= series[0].0 {
        return series[0].1;
    }
    let last = series.len() - 1;
    if t >= series[last].0 {
        return series[last].1;
    }
    let pos = series.partition_point(|&(ts, _)| ts < t);
    let (t0, d0) = series[pos - 1];
    let (t1, d1) = series[pos];
    let span = t1 - t0;
    if span <= 1e-15 {
        return d1;
    }
    let frac = ((t - t0) / span).clamp(0.0, 1.0);
    d0 + frac * (d1 - d0)
}

/// Spatial contingency skill metrics for a predicted flood footprint vs an observed flood mask.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FloodExtentSkillMetrics {
    pub threshold_m: f64,
    pub hits: usize,
    pub misses: usize,
    pub false_alarms: usize,
    pub correct_negatives: usize,
    /// Critical Success Index (Threat Score) `H / (H + M + F)`.
    pub csi: f64,
    /// Probability of Detection (Hit Rate) `H / (H + M)`.
    pub pod: f64,
    /// False Alarm Ratio `F / (H + F)`.
    pub far: f64,
    /// Dice / F1 score `2H / (2H + M + F)`.
    pub f1: f64,
    /// Frequency bias `(H + F) / (H + M)`.
    pub bias_score: f64,
}

/// Scores predicted cell peak depths against an observed binary flood mask over active cells.
pub fn score_flood_extent(
    predicted_depths_m: &[f64],
    observed_flooded: &[bool],
    active_mask: &[bool],
    threshold_m: f64,
) -> Result<FloodExtentSkillMetrics> {
    if !threshold_m.is_finite() || threshold_m <= 0.0 {
        return Err(EvaluationError::InvalidInput(format!(
            "threshold_m {threshold_m} must be finite positive"
        )));
    }
    if predicted_depths_m.is_empty()
        || predicted_depths_m.len() != observed_flooded.len()
        || predicted_depths_m.len() != active_mask.len()
    {
        return Err(EvaluationError::InvalidInput(
            "predicted_depths_m, observed_flooded, and active_mask must have equal non-empty lengths"
                .to_owned(),
        ));
    }

    let mut hits = 0_usize;
    let mut misses = 0_usize;
    let mut false_alarms = 0_usize;
    let mut correct_negatives = 0_usize;

    for (idx, ((&depth, &obs), &active)) in predicted_depths_m
        .iter()
        .zip(observed_flooded.iter())
        .zip(active_mask.iter())
        .enumerate()
    {
        if !active {
            continue;
        }
        if !depth.is_finite() || depth < 0.0 {
            return Err(EvaluationError::InvalidInput(format!(
                "predicted_depths_m[{idx}]={depth} must be finite nonnegative"
            )));
        }
        let pred = depth >= threshold_m;
        match (pred, obs) {
            (true, true) => hits += 1,
            (false, true) => misses += 1,
            (true, false) => false_alarms += 1,
            (false, false) => correct_negatives += 1,
        }
    }

    let h = hits as f64;
    let m = misses as f64;
    let f = false_alarms as f64;
    let csi = if hits + misses + false_alarms > 0 {
        h / (h + m + f)
    } else {
        1.0
    };
    let pod = if hits + misses > 0 { h / (h + m) } else { 1.0 };
    let far = if hits + false_alarms > 0 {
        f / (h + f)
    } else {
        0.0
    };
    let f1 = if 2 * hits + misses + false_alarms > 0 {
        (2.0 * h) / (2.0 * h + m + f)
    } else {
        1.0
    };
    let bias_score = if hits + misses > 0 {
        (h + f) / (h + m)
    } else if false_alarms == 0 {
        1.0
    } else {
        f64::INFINITY
    };

    Ok(FloodExtentSkillMetrics {
        threshold_m,
        hits,
        misses,
        false_alarms,
        correct_negatives,
        csi,
        pod,
        far,
        f1,
        bias_score,
    })
}

#[cfg(test)]
mod skill_tests {
    use super::*;

    #[test]
    fn perfect_hydrograph_scores_unit_nse_and_kge() -> Result<()> {
        let obs = vec![(0.0, 0.1), (60.0, 0.4), (120.0, 0.9), (180.0, 0.3)];
        let metrics = score_hydrograph(&obs, &obs)?;
        assert!((metrics.nse - 1.0).abs() < 1e-12);
        assert!((metrics.kge - 1.0).abs() < 1e-12);
        assert!((metrics.rmse_m).abs() < 1e-12);
        assert!((metrics.peak_timing_error_s).abs() < 1e-12);
        Ok(())
    }

    #[test]
    fn delayed_and_biased_hydrograph_reports_timing_and_kge_penalty() -> Result<()> {
        let obs = vec![(0.0, 0.1), (60.0, 0.5), (120.0, 1.0), (180.0, 0.4)];
        let sim = vec![(0.0, 0.1), (60.0, 0.3), (120.0, 0.7), (180.0, 0.9)];
        let metrics = score_hydrograph(&sim, &obs)?;
        assert!(metrics.kge < 1.0);
        assert!((metrics.peak_error_m - (-0.1)).abs() < 1e-12);
        assert!((metrics.peak_timing_error_s - 60.0).abs() < 1e-12);
        Ok(())
    }

    #[test]
    fn flood_extent_contingency_scores_match_definitions() -> Result<()> {
        let pred = vec![0.5, 0.2, 0.05, 0.0, 0.9];
        let obs = vec![true, false, true, false, true];
        let active = vec![true, true, true, true, false];
        let metrics = score_flood_extent(&pred, &obs, &active, 0.1)?;
        assert_eq!(metrics.hits, 1);
        assert_eq!(metrics.false_alarms, 1);
        assert_eq!(metrics.misses, 1);
        assert_eq!(metrics.correct_negatives, 1);
        assert!((metrics.csi - (1.0 / 3.0)).abs() < 1e-12);
        assert!((metrics.pod - 0.5).abs() < 1e-12);
        assert!((metrics.far - 0.5).abs() < 1e-12);
        assert!((metrics.f1 - 0.5).abs() < 1e-12);
        Ok(())
    }
}
