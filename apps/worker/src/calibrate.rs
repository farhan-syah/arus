//! Automated 2D hydraulic parameter calibration against observed gauge hydrographs
//! and optional spatial flood footprints using `hydr 0.2.0` and `arus-evaluation`.

use std::path::Path;

use arus_evaluation::{
    BudgetAssessment, EvaluationPartition, FloodExtentSkillMetrics, HydrographSkillMetrics,
    assess_water_budget, score_flood_extent, score_hydrograph,
};
use arus_hydraulics::{LandSpec, SurfaceRunSpec};
use arus_jobs::{CancellationToken, StepBudget};
use arus_model_api::CellSample;
use serde::{Deserialize, Serialize};

/// Declarative calibration manifest for `arus-worker calibrate`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationSpec {
    /// Baseline 2D hydraulic run spec whose Manning `n` and optional soil conductivity are tuned.
    pub base_spec: SurfaceRunSpec,
    /// Must be `Calibration` or `Development`. `LockedTest` is rejected to prevent test leakage.
    pub partition: EvaluationPartition,
    /// Probe ID inside `base_spec.probes` matched against `observed_hydrograph`.
    pub probe_id: String,
    /// Observed `(time_s, depth_m)` hydrograph pairs in ascending time order.
    pub observed_hydrograph: Vec<[f64; 2]>,
    /// Candidate uniform Manning `n` values to evaluate.
    pub manning_candidates: Vec<f64>,
    /// Optional candidate Green-Ampt hydraulic conductivity values (`m/s`).
    /// When empty, `base_spec.land` is kept unchanged.
    #[serde(default)]
    pub conductivity_candidates_m_per_s: Vec<f64>,
    /// Optional observed binary flood mask (row-major, length = `cols * rows`) for spatial CSI scoring.
    #[serde(default)]
    pub observed_flood_mask: Option<Vec<bool>>,
    /// Depth threshold (`m`) used for spatial flood-extent scoring when `observed_flood_mask` is set.
    #[serde(default = "default_extent_threshold")]
    pub extent_threshold_m: f64,
}

const fn default_extent_threshold() -> f64 {
    0.10
}

/// Evaluation outcome for one candidate parameter tuple.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationTrial {
    pub trial_index: usize,
    pub manning_n: f64,
    pub conductivity_m_per_s: Option<f64>,
    pub objective_score: f64,
    pub hydrograph: HydrographSkillMetrics,
    pub flood_extent: Option<FloodExtentSkillMetrics>,
    pub budget_assessment: BudgetAssessment,
}

/// Full reproducible report emitted by `arus-worker calibrate`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationReport {
    pub partition: EvaluationPartition,
    pub probe_id: String,
    pub trials_evaluated: usize,
    pub best_trial: CalibrationTrial,
    pub ranked_trials: Vec<CalibrationTrial>,
}

/// Loads a JSON `CalibrationSpec` from disk and runs parameter calibration.
pub fn run_calibration_file(spec_path: &Path) -> Result<CalibrationReport, String> {
    let raw = std::fs::read_to_string(spec_path).map_err(|e| {
        format!(
            "failed to read calibration spec {}: {e}",
            spec_path.display()
        )
    })?;
    let spec: CalibrationSpec =
        serde_json::from_str(&raw).map_err(|e| format!("invalid calibration spec JSON: {e}"))?;
    run_calibration(&spec)
}

/// Executes grid-based 2D hydraulic calibration against an observed hydrograph and optional flood mask.
pub fn run_calibration(spec: &CalibrationSpec) -> Result<CalibrationReport, String> {
    if spec.partition == EvaluationPartition::LockedTest {
        return Err(
            "calibration refuses EvaluationPartition::LockedTest to prevent test-set leakage"
                .to_owned(),
        );
    }
    if spec.manning_candidates.is_empty() {
        return Err("manning_candidates must be non-empty".to_owned());
    }
    if spec.observed_hydrograph.is_empty() {
        return Err("observed_hydrograph must be non-empty".to_owned());
    }
    if !spec.base_spec.probes.iter().any(|p| p.id == spec.probe_id) {
        return Err(format!(
            "probe_id '{}' not found in base_spec.probes",
            spec.probe_id
        ));
    }

    let obs_pairs: Vec<(f64, f64)> = spec
        .observed_hydrograph
        .iter()
        .map(|&[t, d]| (t, d))
        .collect();
    let cond_list: Vec<Option<f64>> = if spec.conductivity_candidates_m_per_s.is_empty() {
        vec![None]
    } else {
        spec.conductivity_candidates_m_per_s
            .iter()
            .copied()
            .map(Some)
            .collect()
    };

    let mut trials = Vec::new();
    let mut trial_index = 0_usize;

    for &manning_n in &spec.manning_candidates {
        for &cond_opt in &cond_list {
            trial_index += 1;
            let mut candidate = spec.base_spec.clone();
            candidate.uniform_manning_n = manning_n;
            if let Some(k) = cond_opt {
                candidate.land = match candidate.land {
                    LandSpec::UniformModifiedGreenAmpt {
                        suction_m,
                        maximum_deficit,
                        ..
                    } => LandSpec::UniformModifiedGreenAmpt {
                        suction_m,
                        conductivity_m_per_s: k,
                        maximum_deficit,
                    },
                    LandSpec::UniformImpervious => LandSpec::UniformModifiedGreenAmpt {
                        suction_m: 0.11,
                        conductivity_m_per_s: k,
                        maximum_deficit: 0.30,
                    },
                };
            }

            let probes = candidate.location_probes();
            let mut engine = candidate
                .build_engine()
                .map_err(|e| format!("trial {trial_index} build failed: {e}"))?;
            let token = CancellationToken::new();
            let budget = StepBudget::new(candidate.max_steps);
            let mut sim_series: Vec<(f64, f64)> = vec![(0.0, 0.0)];

            engine
                .advance_with_probes(
                    candidate.target_time_s,
                    budget,
                    &token,
                    &probes,
                    |_step, samples| {
                        for sample in samples {
                            if sample.id == spec.probe_id {
                                let depth = match sample.sample {
                                    CellSample::Wet { depth_m, .. } => depth_m,
                                    CellSample::Dry | CellSample::Unsupported => 0.0,
                                };
                                sim_series.push((sample.time_s, depth));
                            }
                        }
                    },
                )
                .map_err(|e| format!("trial {trial_index} advance failed: {e}"))?;

            let hydrograph = score_hydrograph(&sim_series, &obs_pairs)
                .map_err(|e| format!("trial {trial_index} hydrograph score failed: {e}"))?;
            let budget_assessment = assess_water_budget(engine.water_budget());

            let flood_extent = if let Some(ref obs_mask) = spec.observed_flood_mask {
                let pred_depths = engine
                    .current_depths_m()
                    .map_err(|e| format!("trial {trial_index} cell read failed: {e}"))?;
                let active_mask = candidate.terrain.active_mask();
                Some(
                    score_flood_extent(
                        &pred_depths,
                        obs_mask,
                        &active_mask,
                        spec.extent_threshold_m,
                    )
                    .map_err(|e| format!("trial {trial_index} extent score failed: {e}"))?,
                )
            } else {
                None
            };

            let csi_bonus = flood_extent.map_or(0.0, |ext| 0.25 * ext.csi);
            let objective_score = if budget_assessment.passed {
                hydrograph.kge + csi_bonus
            } else {
                f64::NEG_INFINITY
            };

            trials.push(CalibrationTrial {
                trial_index,
                manning_n,
                conductivity_m_per_s: cond_opt,
                objective_score,
                hydrograph,
                flood_extent,
                budget_assessment,
            });
        }
    }

    trials.sort_by(|a, b| {
        b.objective_score
            .total_cmp(&a.objective_score)
            .then_with(|| a.hydrograph.rmse_m.total_cmp(&b.hydrograph.rmse_m))
    });

    let best_trial = trials
        .first()
        .cloned()
        .ok_or_else(|| "no calibration trials evaluated".to_owned())?;

    Ok(CalibrationReport {
        partition: spec.partition,
        probe_id: spec.probe_id.clone(),
        trials_evaluated: trials.len(),
        best_trial,
        ranked_trials: trials,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use arus_domain::{GridSpec, RunClassification, SpatialReference};
    use arus_geo::TerrainGrid;
    use arus_hydraulics::{ProbeSpec, RainIntervalSpec};

    fn synthetic_sloping_spec(manning_n: f64) -> Result<SurfaceRunSpec, String> {
        let cols = 6;
        let rows = 5;
        let mut cells = Vec::with_capacity(cols * rows);
        for _r in 0..rows {
            for c in 0..cols {
                cells.push(Some(2.0 - 0.15 * (c as f64)));
            }
        }
        let terrain = TerrainGrid::new(
            GridSpec {
                columns: cols,
                rows,
                dx_m: 10.0,
                dy_m: 10.0,
            },
            SpatialReference {
                projected_crs: "EPSG:3375".to_owned(),
                vertical_datum: "NGVD".to_owned(),
                origin_m: [0.0, 0.0],
            },
            "synthetic-calib",
            cells,
        )
        .map_err(|e| e.to_string())?;

        Ok(SurfaceRunSpec {
            run_id: "calib-base".to_owned(),
            classification: RunClassification::NumericalDemonstration,
            terrain,
            uniform_manning_n: manning_n,
            land: LandSpec::UniformImpervious,
            initial_depth_m: 0.0,
            open_faces: Vec::new(),
            forcing: vec![
                RainIntervalSpec {
                    start_s: 0.0,
                    end_s: 40.0,
                    uniform_rain_m_per_s: 1.5e-4,
                    boundaries: Vec::new(),
                },
                RainIntervalSpec {
                    start_s: 40.0,
                    end_s: 80.0,
                    uniform_rain_m_per_s: 0.0,
                    boundaries: Vec::new(),
                },
            ],
            target_time_s: 80.0,
            max_steps: 2_000,
            probes: vec![ProbeSpec {
                id: "outlet".to_owned(),
                x_m: 55.0,
                y_m: 25.0,
            }],
            threshold_m: 0.005,
            hysteresis_m: 0.001,
            recession_persistence_s: 10.0,
            coastal: None,
        })
    }

    #[test]
    fn calibration_recovers_true_manning_n_and_rejects_locked_test() -> Result<(), String> {
        // Generate synthetic "observed" hydrograph with true manning_n = 0.050
        let truth_spec = synthetic_sloping_spec(0.050)?;
        let probes = truth_spec.location_probes();
        let mut truth_engine = truth_spec.build_engine().map_err(|e| e.to_string())?;
        let mut obs = Vec::new();
        truth_engine
            .advance_with_probes(
                truth_spec.target_time_s,
                StepBudget::new(truth_spec.max_steps),
                &CancellationToken::new(),
                &probes,
                |_step, samples| {
                    for s in samples {
                        if s.id == "outlet" {
                            let d = match s.sample {
                                CellSample::Wet { depth_m, .. } => depth_m,
                                CellSample::Dry | CellSample::Unsupported => 0.0,
                            };
                            obs.push([s.time_s, d]);
                        }
                    }
                },
            )
            .map_err(|e| e.to_string())?;

        // Reject LockedTest partition
        let locked = CalibrationSpec {
            base_spec: synthetic_sloping_spec(0.030)?,
            partition: EvaluationPartition::LockedTest,
            probe_id: "outlet".to_owned(),
            observed_hydrograph: obs.clone(),
            manning_candidates: vec![0.020, 0.050, 0.100],
            conductivity_candidates_m_per_s: Vec::new(),
            observed_flood_mask: None,
            extent_threshold_m: 0.01,
        };
        assert!(run_calibration(&locked).is_err());

        // Calibrate on Calibration partition and recover 0.050 with KGE ~ 1.0
        let cal = CalibrationSpec {
            partition: EvaluationPartition::Calibration,
            ..locked
        };
        let report = run_calibration(&cal)?;
        assert_eq!(report.trials_evaluated, 3);
        assert!((report.best_trial.manning_n - 0.050).abs() < 1e-12);
        assert!((report.best_trial.hydrograph.kge - 1.0).abs() < 1e-9);
        assert!(report.best_trial.budget_assessment.passed);
        Ok(())
    }
}
