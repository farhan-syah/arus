//! Generic JSON-configurable river-gauge threshold registry and anti-hindsight shadow evaluation.
//!
//! Any ARUS instance can load its own station registry JSON file without hard-coding regional data.

use arus_domain::RunClassification;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors returned when validating a station registry or scoring a shadow forecast.
#[derive(Debug, Error)]
pub enum StationError {
    #[error(
        "invalid station threshold ordering for `{station_id}`: normal <= alert <= warning <= danger required"
    )]
    InvalidThresholds { station_id: String },
    #[error(
        "hindsight leakage rejected: observation `{date}` is on or before forecast issue `{issued_at}`"
    )]
    HindsightLeakage { date: String, issued_at: String },
    #[error("empty evaluation series: at least one overlapping post-issue day is required")]
    EmptyEvaluation,
}

/// Four-tier river stage status mapped from user-supplied station thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StationStatus {
    Normal,
    Alert,
    Warning,
    Danger,
    Unknown,
}

/// Generic river-gauge station metadata and stage thresholds (metres above station datum).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StationThreshold {
    pub station_id: String,
    pub name: String,
    pub river: String,
    pub latitude: f64,
    pub longitude: f64,
    pub normal_m: f64,
    pub alert_m: f64,
    pub warning_m: f64,
    pub danger_m: f64,
}

impl StationThreshold {
    /// Checks monotonic threshold ordering (`normal <= alert <= warning <= danger`).
    pub fn validate(&self) -> Result<(), StationError> {
        if !(self.normal_m.is_finite()
            && self.alert_m.is_finite()
            && self.warning_m.is_finite()
            && self.danger_m.is_finite()
            && self.normal_m <= self.alert_m
            && self.alert_m <= self.warning_m
            && self.warning_m <= self.danger_m)
        {
            return Err(StationError::InvalidThresholds {
                station_id: self.station_id.clone(),
            });
        }
        Ok(())
    }

    /// Maps an optional water stage (`m`) to its threshold tier. `None` always maps to `Unknown`.
    #[must_use]
    pub fn classify_stage(&self, stage_m: Option<f64>) -> StationStatus {
        let Some(level) = stage_m.filter(|v| v.is_finite()) else {
            return StationStatus::Unknown;
        };
        if level >= self.danger_m {
            StationStatus::Danger
        } else if level >= self.warning_m {
            StationStatus::Warning
        } else if level >= self.alert_m {
            StationStatus::Alert
        } else {
            StationStatus::Normal
        }
    }
}

/// User-supplied collection of gauge stations loaded from JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StationRegistry {
    pub schema_version: String,
    pub stations: Vec<StationThreshold>,
}

impl StationRegistry {
    /// Validates every station threshold definition in the registry.
    pub fn validate(&self) -> Result<(), StationError> {
        for s in &self.stations {
            s.validate()?;
        }
        Ok(())
    }

    /// Looks up a station by `station_id`.
    #[must_use]
    pub fn find(&self, station_id: &str) -> Option<&StationThreshold> {
        self.stations.iter().find(|s| s.station_id == station_id)
    }
}

/// Screening-only assessment binding a forecast stage to a registered station threshold.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StationForecastAssessment {
    pub station_id: String,
    pub station_name: String,
    /// Uncalibrated runs are always tagged `screening-only` (`NumericalDemonstration`).
    pub label: String,
    pub classification: RunClassification,
    pub projected_stage_m: Option<f64>,
    pub status: StationStatus,
}

/// Builds a station assessment; uncalibrated runs are strictly tagged `screening-only`.
#[must_use]
pub fn assess_station(
    station: &StationThreshold,
    projected_stage_m: Option<f64>,
    calibrated_and_approved: bool,
) -> StationForecastAssessment {
    let (label, classification) = if calibrated_and_approved {
        (
            "operational-forecast".to_owned(),
            RunClassification::OperationalForecast,
        )
    } else {
        (
            "screening-only".to_owned(),
            RunClassification::NumericalDemonstration,
        )
    };
    StationForecastAssessment {
        station_id: station.station_id.clone(),
        station_name: station.name.clone(),
        label,
        classification,
        projected_stage_m,
        status: station.classify_stage(projected_stage_m),
    }
}

/// One daily forecast record archived at `issued_at` (`YYYY-MM-DD`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DailyShadowForecast {
    pub date: String,
    pub forecast_rain_mm: f64,
    pub forecast_status: StationStatus,
}

/// One observed daily verification record collected after `issued_at`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DailyShadowObservation {
    pub date: String,
    pub observed_rain_mm: f64,
    pub observed_status: StationStatus,
}

/// Anti-hindsight verification score comparing an archived `T0` forecast against later observations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowEvaluationReport {
    pub issued_at: String,
    pub scored_days: usize,
    pub rain_mae_mm: f64,
    pub rain_bias_mm: f64,
    pub status_exact_matches: usize,
    pub peak_day_match: bool,
}

/// Scores archived `T0` daily forecasts against observed days strictly after `issued_at`.
///
/// Rejects any observation whose ISO date `<= issued_at` to prevent hindsight leakage.
pub fn score_shadow_forecast(
    issued_at: &str,
    forecasts: &[DailyShadowForecast],
    observations: &[DailyShadowObservation],
) -> Result<ShadowEvaluationReport, StationError> {
    for obs in observations {
        if obs.date.as_str() <= issued_at {
            return Err(StationError::HindsightLeakage {
                date: obs.date.clone(),
                issued_at: issued_at.to_owned(),
            });
        }
    }

    let mut pairs = Vec::new();
    for f in forecasts {
        if f.date.as_str() <= issued_at {
            continue;
        }
        if let Some(obs) = observations.iter().find(|o| o.date == f.date) {
            pairs.push((f, obs));
        }
    }
    if pairs.is_empty() {
        return Err(StationError::EmptyEvaluation);
    }

    let n = pairs.len() as f64;
    let abs_sum: f64 = pairs
        .iter()
        .map(|(f, o)| (f.forecast_rain_mm - o.observed_rain_mm).abs())
        .sum();
    let bias_sum: f64 = pairs
        .iter()
        .map(|(f, o)| f.forecast_rain_mm - o.observed_rain_mm)
        .sum();
    let status_exact_matches = pairs
        .iter()
        .filter(|(f, o)| f.forecast_status == o.observed_status)
        .count();

    let f_peak_date = pairs
        .iter()
        .max_by(|a, b| a.0.forecast_rain_mm.total_cmp(&b.0.forecast_rain_mm))
        .map(|(f, _)| f.date.as_str());
    let o_peak_date = pairs
        .iter()
        .max_by(|a, b| a.1.observed_rain_mm.total_cmp(&b.1.observed_rain_mm))
        .map(|(_, o)| o.date.as_str());

    Ok(ShadowEvaluationReport {
        issued_at: issued_at.to_owned(),
        scored_days: pairs.len(),
        rain_mae_mm: abs_sum / n,
        rain_bias_mm: bias_sum / n,
        status_exact_matches,
        peak_day_match: f_peak_date.is_some() && f_peak_date == o_peak_date,
    })
}
