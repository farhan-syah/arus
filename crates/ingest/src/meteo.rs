//! Deterministic parser for archived Open-Meteo single-model and multi-model hourly JSON blobs.
//!
//! Preserves missing entries as `None` (never coerced to `0.0`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Result};

/// One model's hourly precipitation and optional topsoil moisture series extracted from an archived blob.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HourlyModelSeries {
    pub model: String,
    pub times: Vec<String>,
    /// Hourly precipitation in `mm` (`None` preserves missing `null` entries).
    pub precipitation_mm: Vec<Option<f64>>,
    /// Optional hourly `soil_moisture_0_to_7cm` in `m³/m³` (`None` preserves missing `null` entries).
    pub soil_moisture_0_to_7cm: Vec<Option<f64>>,
}

/// Parsed multi-model Open-Meteo hourly forecast payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultiModelHourlyForecast {
    pub latitude: f64,
    pub longitude: f64,
    pub timezone: String,
    pub series: Vec<HourlyModelSeries>,
}

fn parse_opt_f64_array(arr: &[Value], field: &str) -> Result<Vec<Option<f64>>> {
    let mut out = Vec::with_capacity(arr.len());
    for entry in arr {
        if entry.is_null() {
            out.push(None);
        } else if let Some(n) = entry.as_f64().filter(|n| n.is_finite() && *n >= 0.0) {
            out.push(Some(n));
        } else {
            return Err(Error::Acquisition(format!(
                "invalid numeric value in hourly field `{field}`"
            )));
        }
    }
    Ok(out)
}

/// Parses an archived Open-Meteo hourly JSON blob (single-model or multi-model) into typed per-model series.
pub fn parse_open_meteo_hourly(bytes: &[u8]) -> Result<MultiModelHourlyForecast> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|e| Error::Acquisition(format!("invalid Open-Meteo JSON: {e}")))?;
    let latitude = value
        .get("latitude")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && (-90.0..=90.0).contains(v))
        .ok_or_else(|| Error::Acquisition("missing or invalid latitude".to_owned()))?;
    let longitude = value
        .get("longitude")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && (-180.0..=180.0).contains(v))
        .ok_or_else(|| Error::Acquisition("missing or invalid longitude".to_owned()))?;
    let timezone = value
        .get("timezone")
        .and_then(Value::as_str)
        .unwrap_or("GMT")
        .to_owned();

    let hourly = value
        .get("hourly")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::Acquisition("missing hourly object".to_owned()))?;
    let times_val = hourly
        .get("time")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Acquisition("missing hourly.time array".to_owned()))?;
    let mut times = Vec::with_capacity(times_val.len());
    for t in times_val {
        let s = t
            .as_str()
            .ok_or_else(|| Error::Acquisition("non-string entry in hourly.time".to_owned()))?;
        times.push(s.to_owned());
    }
    if times.is_empty() {
        return Err(Error::Acquisition("hourly.time array is empty".to_owned()));
    }

    // Collect model names: either single un-suffixed "precipitation" or suffixed "precipitation_<model>"
    let mut model_keys: Vec<(String, String, String)> = Vec::new();
    for key in hourly.keys() {
        if key == "precipitation" {
            model_keys.push((
                "default".to_owned(),
                "precipitation".to_owned(),
                "soil_moisture_0_to_7cm".to_owned(),
            ));
        } else if let Some(model) = key.strip_prefix("precipitation_") {
            model_keys.push((
                model.to_owned(),
                key.clone(),
                format!("soil_moisture_0_to_7cm_{model}"),
            ));
        }
    }
    model_keys.sort_by(|a, b| a.0.cmp(&b.0));

    if model_keys.is_empty() {
        return Err(Error::Acquisition(
            "no precipitation series found in hourly object".to_owned(),
        ));
    }

    let mut series = Vec::with_capacity(model_keys.len());
    for (model, precip_key, soil_key) in model_keys {
        let precip_arr = hourly
            .get(&precip_key)
            .and_then(Value::as_array)
            .ok_or_else(|| Error::Acquisition(format!("missing array `{precip_key}`")))?;
        if precip_arr.len() != times.len() {
            return Err(Error::Acquisition(format!(
                "length mismatch for `{precip_key}` ({} != {})",
                precip_arr.len(),
                times.len()
            )));
        }
        let precipitation_mm = parse_opt_f64_array(precip_arr, &precip_key)?;

        let soil_moisture_0_to_7cm = match hourly.get(&soil_key).and_then(Value::as_array) {
            Some(arr) => {
                if arr.len() != times.len() {
                    return Err(Error::Acquisition(format!(
                        "length mismatch for `{soil_key}` ({} != {})",
                        arr.len(),
                        times.len()
                    )));
                }
                parse_opt_f64_array(arr, &soil_key)?
            }
            None => vec![None; times.len()],
        };

        series.push(HourlyModelSeries {
            model,
            times: times.clone(),
            precipitation_mm,
            soil_moisture_0_to_7cm,
        });
    }

    Ok(MultiModelHourlyForecast {
        latitude,
        longitude,
        timezone,
        series,
    })
}
