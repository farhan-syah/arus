//! Generic multi-model NWP ensemble aggregation and divergence / agreement scoring.

use serde::{Deserialize, Serialize};

/// One model member's hydrological and meteorological forecast summary over the forecast horizon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelForecastMember {
    pub model: String,
    pub total_rain_mm: f64,
    pub peak_24h_mm: f64,
    pub peak_hourly_mm: f64,
    pub total_runoff_m3: f64,
    pub peak_window_start_h: usize,
    pub peak_window_end_h: usize,
    pub missing_precip_hours: usize,
    pub antecedent_deficit: f64,
}

/// Qualitative model agreement level derived from cross-model spread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgreementLevel {
    High,
    Moderate,
    Low,
}

/// Aggregated multi-model ensemble summary preserving full spread rather than collapsing to one claim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnsembleForecastSummary {
    pub member_count: usize,
    pub agreement: AgreementLevel,
    pub min_total_rain_mm: f64,
    pub median_total_rain_mm: f64,
    pub max_total_rain_mm: f64,
    pub rain_spread_mm: f64,
    pub min_peak_24h_mm: f64,
    pub median_peak_24h_mm: f64,
    pub max_peak_24h_mm: f64,
    pub peak_24h_spread_ratio: f64,
    pub earliest_peak_hour: usize,
    pub worst_case_model: String,
    pub members: Vec<ModelForecastMember>,
}

fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        values[mid]
    } else {
        0.5 * (values[mid - 1] + values[mid])
    }
}

/// Computes multi-model spread, median, worst-case member, and `AgreementLevel`.
///
/// When models strongly diverge (e.g. peak 24h spread ratio `>= 0.50` and spread `>= 25 mm`),
/// returns `AgreementLevel::Low` so downstream UIs and operators see uncertainty explicitly.
#[must_use]
pub fn summarize_ensemble(members: Vec<ModelForecastMember>) -> EnsembleForecastSummary {
    if members.is_empty() {
        return EnsembleForecastSummary {
            member_count: 0,
            agreement: AgreementLevel::Low,
            min_total_rain_mm: 0.0,
            median_total_rain_mm: 0.0,
            max_total_rain_mm: 0.0,
            rain_spread_mm: 0.0,
            min_peak_24h_mm: 0.0,
            median_peak_24h_mm: 0.0,
            max_peak_24h_mm: 0.0,
            peak_24h_spread_ratio: 0.0,
            earliest_peak_hour: 0,
            worst_case_model: String::new(),
            members,
        };
    }

    let totals: Vec<f64> = members.iter().map(|m| m.total_rain_mm).collect();
    let peaks: Vec<f64> = members.iter().map(|m| m.peak_24h_mm).collect();
    let min_total = totals.iter().copied().fold(f64::INFINITY, f64::min);
    let max_total = totals.iter().copied().fold(0.0_f64, f64::max);
    let min_peak = peaks.iter().copied().fold(f64::INFINITY, f64::min);
    let max_peak = peaks.iter().copied().fold(0.0_f64, f64::max);
    let peak_spread = (max_peak - min_peak).max(0.0);
    let spread_ratio = if max_peak > 1e-6 {
        peak_spread / max_peak
    } else {
        0.0
    };

    let agreement = if members.len() == 1 || (spread_ratio >= 0.50 && peak_spread >= 25.0) {
        AgreementLevel::Low
    } else if spread_ratio >= 0.25 && peak_spread >= 10.0 {
        AgreementLevel::Moderate
    } else {
        AgreementLevel::High
    };

    let earliest_peak_hour = members
        .iter()
        .map(|m| m.peak_window_start_h)
        .min()
        .unwrap_or(0);
    let worst_case_model = members
        .iter()
        .max_by(|a, b| a.peak_24h_mm.total_cmp(&b.peak_24h_mm))
        .map(|m| m.model.clone())
        .unwrap_or_default();

    EnsembleForecastSummary {
        member_count: members.len(),
        agreement,
        min_total_rain_mm: min_total,
        median_total_rain_mm: median(totals),
        max_total_rain_mm: max_total,
        rain_spread_mm: (max_total - min_total).max(0.0),
        min_peak_24h_mm: min_peak,
        median_peak_24h_mm: median(peaks),
        max_peak_24h_mm: max_peak,
        peak_24h_spread_ratio: spread_ratio,
        earliest_peak_hour,
        worst_case_model,
        members,
    }
}
