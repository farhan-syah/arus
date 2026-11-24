//! Deterministic ingest-to-hydrology bridge: translates hourly rainfall & antecedent soil moisture
//! into 72–120h `RunoffRunSpec` manifests and extracts peak 3–6h windows for 2D `hydr` refinement.

use arus_domain::RunClassification;
use arus_model_api::{ModelError, Result};
use serde::{Deserialize, Serialize};

use crate::spec::{
    RunoffInfiltrationSpec, RunoffIntervalSpec, RunoffRoutingSpec, RunoffRunReport, RunoffRunSpec,
    SubcatchmentSpec,
};

/// Antecedent moisture state derived from `past_days` rainfall and topsoil moisture observations.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AntecedentState {
    /// Cumulative observed/reanalysis rain prior to forecast start (`mm`).
    pub past_rain_mm: f64,
    /// Latest non-null topsoil volumetric moisture (`m³/m³`, e.g. `soil_moisture_0_to_7cm`).
    pub latest_soil_moisture_m3_per_m3: Option<f64>,
    /// Effective Modified Green-Ampt initial moisture deficit passed to `hydr` (`m³/m³`).
    pub effective_maximum_deficit: f64,
}

/// Peak runoff window detected across a multi-day 1D hydrology run for targeted 2D `hydr` refinement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeakRunoffWindow {
    pub start_hour: usize,
    pub end_hour: usize,
    pub window_hours: usize,
    pub window_outlet_m3: f64,
    pub window_rain_mm: f64,
    pub peak_hourly_outlet_m3: f64,
}

/// Derives an effective Green-Ampt moisture deficit from antecedent rainfall (`mm`) and optional
/// topsoil volumetric moisture (`m³/m³`, porosity ~`0.52`). Saturated soil (`0.506 m³/m³`) yields a
/// small deficit (`~0.03`) so infiltration capacity reflects wet catchment conditions.
#[must_use]
pub fn derive_antecedent_state(
    past_rain_mm: f64,
    latest_soil_moisture: Option<f64>,
    dry_maximum_deficit: f64,
) -> AntecedentState {
    let base_max = dry_maximum_deficit.clamp(0.05, 0.50);
    let rain_reduction = (past_rain_mm.max(0.0) / 120.0).clamp(0.0, 0.75) * base_max;
    let deficit_from_rain = (base_max - rain_reduction).max(0.03);

    let effective = match latest_soil_moisture {
        Some(sm) if sm.is_finite() && sm >= 0.0 => {
            let porosity = 0.52_f64;
            let deficit_from_soil = (porosity - sm).clamp(0.03, base_max);
            deficit_from_soil.min(deficit_from_rain)
        }
        _ => deficit_from_rain,
    };

    AntecedentState {
        past_rain_mm: past_rain_mm.max(0.0),
        latest_soil_moisture_m3_per_m3: latest_soil_moisture,
        effective_maximum_deficit: effective,
    }
}

/// Translates an hourly precipitation series (`mm/hr`, preserving `None` as explicit missing count)
/// into a deterministic 1D `RunoffRunSpec` over the full multi-day horizon.
pub fn build_runoff_spec_from_hourly(
    run_id: impl Into<String>,
    basin_id: impl Into<String>,
    area_km2: f64,
    impervious_fraction: f64,
    hourly_rain_mm: &[Option<f64>],
    antecedent: AntecedentState,
) -> Result<RunoffRunSpec> {
    if !area_km2.is_finite() || area_km2 <= 0.0 {
        return Err(ModelError::InvalidInput(format!(
            "area_km2={area_km2} must be finite and > 0"
        )));
    }
    if hourly_rain_mm.is_empty() {
        return Err(ModelError::InvalidInput(
            "hourly_rain_mm must not be empty".to_owned(),
        ));
    }
    let total_area_m2 = area_km2 * 1_000_000.0;
    let width_m = total_area_m2.sqrt();
    let imp = impervious_fraction.clamp(0.02, 0.95);

    let intervals: Vec<RunoffIntervalSpec> = hourly_rain_mm
        .iter()
        .enumerate()
        .map(|(i, opt_mm)| {
            let mm = opt_mm.unwrap_or(0.0).max(0.0);
            RunoffIntervalSpec {
                end_time_s: ((i + 1) as f64) * 3600.0,
                rain_rate_m_per_s: mm / 3_600_000.0,
                potential_evaporation_m_per_s: 0.0,
            }
        })
        .collect();

    Ok(RunoffRunSpec {
        run_id: run_id.into(),
        classification: RunClassification::NumericalDemonstration,
        subcatchments: vec![SubcatchmentSpec {
            id: basin_id.into(),
            total_area_m2,
            width_m,
            slope: 0.008,
            impervious_fraction: imp,
            zero_storage_impervious_fraction: 0.25,
            impervious_manning: 0.015,
            pervious_manning: 0.15,
            impervious_depression_m: 0.0015,
            pervious_depression_m: 0.004,
            infiltration: Some(RunoffInfiltrationSpec {
                suction_m: 0.11,
                conductivity_m_per_s: 3.0e-6,
                maximum_deficit: antecedent.effective_maximum_deficit,
            }),
            routing: RunoffRoutingSpec::DirectToOutlet,
        }],
        intervals,
    })
}

/// Scans a 1D `RunoffRunReport` to find the peak contiguous `window_hours` (e.g. 6 h) window
/// suitable for triggering a targeted 2D `hydr` flood simulation.
pub fn detect_peak_window(
    report: &RunoffRunReport,
    hourly_rain_mm: &[Option<f64>],
    window_hours: usize,
) -> Result<PeakRunoffWindow> {
    let n = report.steps.len();
    if n == 0 || window_hours == 0 {
        return Err(ModelError::InvalidInput(
            "runoff report and window_hours must be non-empty".to_owned(),
        ));
    }
    let w = window_hours.min(n);
    let mut best_start = 0;
    let mut best_outlet = -1.0_f64;
    for start in 0..=(n - w) {
        let sum: f64 = report.steps[start..start + w]
            .iter()
            .map(|s| s.outlet_m3)
            .sum();
        if sum > best_outlet {
            best_outlet = sum;
            best_start = start;
        }
    }
    let peak_hourly_outlet_m3 = report.steps[best_start..best_start + w]
        .iter()
        .map(|s| s.outlet_m3)
        .fold(0.0_f64, f64::max);
    let window_rain_mm: f64 = hourly_rain_mm
        .get(best_start..best_start + w)
        .unwrap_or(&[])
        .iter()
        .filter_map(|v| *v)
        .sum();

    Ok(PeakRunoffWindow {
        start_hour: best_start,
        end_hour: best_start + w,
        window_hours: w,
        window_outlet_m3: best_outlet.max(0.0),
        window_rain_mm,
        peak_hourly_outlet_m3,
    })
}
