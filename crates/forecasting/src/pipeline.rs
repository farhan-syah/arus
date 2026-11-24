//! General-purpose 4-stage flood forecasting pipeline types and Stage 1 / Stage 3 evaluators:
//! Stage 1 (Initial State & Gauge Headroom) -> Stage 2 (72-120h 1D Hydrology Ensemble) ->
//! Stage 3 (Coastal Compound Tide Lock) -> Stage 4 (Windowed 2D Hydraulics & Recession).

use arus_domain::RunClassification;
use serde::{Deserialize, Serialize};

use crate::ensemble::EnsembleForecastSummary;
use crate::stations::{StationStatus, StationThreshold};

/// Stage 1: Initial basin wetness and river-gauge headroom before forecast start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InitialBasinState {
    pub initial_stage_source: String,
    pub station_id: Option<String>,
    pub station_name: Option<String>,
    pub current_stage_m: Option<f64>,
    pub current_status: StationStatus,
    /// Remaining vertical headroom (`threshold - current_stage_m`); negative means already exceeded.
    pub headroom_to_alert_m: Option<f64>,
    pub headroom_to_warning_m: Option<f64>,
    pub headroom_to_danger_m: Option<f64>,
    pub worst_antecedent_deficit: f64,
}

/// Computes Stage 1 initial state from an optional station threshold and live stage reading.
/// When no gauge is provided, tags `initial_stage_source = "estimated-glofas-warmup"`.
#[must_use]
pub fn build_initial_basin_state(
    station: Option<&StationThreshold>,
    current_stage_m: Option<f64>,
    worst_antecedent_deficit: f64,
) -> InitialBasinState {
    match (station, current_stage_m.filter(|v| v.is_finite())) {
        (Some(st), Some(level)) => InitialBasinState {
            initial_stage_source: "gauge".to_owned(),
            station_id: Some(st.station_id.clone()),
            station_name: Some(st.name.clone()),
            current_stage_m: Some(level),
            current_status: st.classify_stage(Some(level)),
            headroom_to_alert_m: Some(st.alert_m - level),
            headroom_to_warning_m: Some(st.warning_m - level),
            headroom_to_danger_m: Some(st.danger_m - level),
            worst_antecedent_deficit,
        },
        (Some(st), None) => InitialBasinState {
            initial_stage_source: "estimated-glofas-warmup".to_owned(),
            station_id: Some(st.station_id.clone()),
            station_name: Some(st.name.clone()),
            current_stage_m: None,
            current_status: StationStatus::Unknown,
            headroom_to_alert_m: None,
            headroom_to_warning_m: None,
            headroom_to_danger_m: None,
            worst_antecedent_deficit,
        },
        (None, _) => InitialBasinState {
            initial_stage_source: "estimated-glofas-warmup".to_owned(),
            station_id: None,
            station_name: None,
            current_stage_m: None,
            current_status: StationStatus::Unknown,
            headroom_to_alert_m: None,
            headroom_to_warning_m: None,
            headroom_to_danger_m: None,
            worst_antecedent_deficit,
        },
    }
}

/// Stage 3: Compound coastal tide-lock check overlapping peak river runoff with sea-level height.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompoundTideLock {
    pub window_start_h: usize,
    pub window_end_h: usize,
    pub tide_threshold_msl_m: f64,
    pub max_tide_msl_m: Option<f64>,
    pub peak_tide_time: Option<String>,
    pub compound_backwater_risk: bool,
}

/// Evaluates whether hourly `sea_level_height_msl` during the Stage 2 peak runoff window reaches
/// or exceeds `tide_threshold_msl_m` (e.g. `+1.8 m MSL`), flagging compound backwater risk.
#[must_use]
pub fn evaluate_compound_tide_lock(
    window_start_h: usize,
    window_end_h: usize,
    past_hours: usize,
    marine_times: &[String],
    sea_level_msl_m: &[Option<f64>],
    tide_threshold_msl_m: f64,
) -> CompoundTideLock {
    let start_idx = past_hours.saturating_add(window_start_h);
    let end_idx = past_hours
        .saturating_add(window_end_h)
        .min(sea_level_msl_m.len());

    let mut max_tide: Option<f64> = None;
    let mut peak_time: Option<String> = None;
    if start_idx < end_idx {
        for (idx, item) in sea_level_msl_m
            .iter()
            .enumerate()
            .take(end_idx)
            .skip(start_idx)
        {
            if let Some(val) = *item
                && max_tide.is_none_or(|cur| val > cur)
            {
                max_tide = Some(val);
                peak_time = marine_times.get(idx).cloned();
            }
        }
    }
    let compound_backwater_risk = max_tide.is_some_and(|v| v >= tide_threshold_msl_m);

    CompoundTideLock {
        window_start_h,
        window_end_h,
        tide_threshold_msl_m,
        max_tide_msl_m: max_tide,
        peak_tide_time: peak_time,
        compound_backwater_risk,
    }
}

/// Stage 4: Windowed 2D `hydr 0.2.0` inundation and recession (`waktu surut`) summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Windowed2DOutcome {
    pub triggered: bool,
    pub trigger_reason: String,
    pub driver_model: String,
    pub window_hours: usize,
    pub max_depth_m: f64,
    pub onset_time_s: Option<f64>,
    pub recession_status: String,
    pub mass_residual_m3: f64,
    pub mass_valid: bool,
}

/// Complete output of the general-purpose 4-stage ARUS flood prediction pipeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FourStagePipelineReport {
    pub basin_id: String,
    pub label: String,
    pub classification: RunClassification,
    pub stage1_initial_state: InitialBasinState,
    pub stage2_ensemble: EnsembleForecastSummary,
    pub stage3_compound_tide: Option<CompoundTideLock>,
    pub stage4_windowed_2d: Windowed2DOutcome,
}
