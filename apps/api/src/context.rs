//! Generic basin-scoped server context contract (`--context-dir` + `GET /api/v1/context`).
//!
//! Allows an external private ops pipeline to atomically publish normalized spatial NWP,
//! hourly tides, 15-day ensemble quantiles, MJO regime context, and official warnings
//! without embedding any provider-specific code or credentials in ARUS.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::sim::{ObservedGaugeSeed, SpatialRainGrid};

/// Maximum JSON size accepted for one published basin context file (`2 MiB`).
pub const MAX_CONTEXT_BYTES: usize = 2 * 1024 * 1024;
/// Default age threshold (`12 h`) after which a context file is flagged `is_stale = true`.
pub const STALE_AFTER_SECS: i64 = 12 * 3600;

/// Daily ensemble rainfall quantile summary for medium-range Days 6–15 outlooks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnsembleDayQuantile {
    pub day_iso: String,
    pub p10_mm: f64,
    pub p50_mm: f64,
    pub p90_mm: f64,
}

/// Medium-range (Days 6–15) ensemble precipitation spread summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnsembleOutlook {
    pub model: String,
    pub members: usize,
    pub days: Vec<EnsembleDayQuantile>,
}

/// Wheeler-Hendon Madden-Julian Oscillation (MJO) sub-seasonal regime context (Weeks 2–4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MjoRegime {
    /// MJO phase (`1..=8`).
    pub phase: u8,
    /// RMM amplitude (`>= 1.0` = active convective envelope).
    pub amplitude: f64,
    /// Short human-readable regime label (e.g. `"Maritime Continent · Elevated Monsoon Risk"`).
    pub label: String,
    /// Optional source attribution (e.g. `"NOAA CPC RMM"`).
    #[serde(default)]
    pub source: Option<String>,
}

/// Official warning or advisory item attached to a regional context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextWarning {
    pub agency: String,
    pub level: String,
    pub summary: String,
    #[serde(default)]
    pub issued_iso: Option<String>,
}

/// Normalized basin context published by an external ops pipeline into `--context-dir`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegionalContext {
    pub schema_version: u32,
    pub basin_id: String,
    /// `[min_lon, min_lat, max_lon, max_lat]` geographic validity bounds.
    pub wgs84_bounds: [f64; 4],
    /// RFC-3339 issuance / forecast cycle timestamp.
    pub cycle_iso: String,
    /// Optional RFC-3339 expiry timestamp.
    #[serde(default)]
    pub valid_until_iso: Option<String>,
    #[serde(default)]
    pub antecedent_rain_mm: f64,
    #[serde(default)]
    pub observed_gauges: Vec<ObservedGaugeSeed>,
    #[serde(default)]
    pub active_incidents: Vec<String>,
    #[serde(default)]
    pub interventions: Vec<serde_json::Value>,
    #[serde(default)]
    pub spatial_rain_grid: Option<SpatialRainGrid>,
    #[serde(default)]
    pub hourly_tide_m: Vec<f64>,
    #[serde(default)]
    pub ensemble_15d: Option<EnsembleOutlook>,
    #[serde(default)]
    pub mjo: Option<MjoRegime>,
    #[serde(default)]
    pub warnings: Vec<ContextWarning>,
}

/// Envelope returned by `GET /api/v1/context` with freshness metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextResponse {
    pub context: RegionalContext,
    pub age_seconds: i64,
    pub is_stale: bool,
}

/// Validates a basin query ID (`[a-z0-9_-]{1,64}`) to prevent path traversal.
pub fn validate_basin_id(basin_id: &str) -> Result<(), String> {
    if basin_id.is_empty() || basin_id.len() > 64 {
        return Err("basin id must be 1..=64 ASCII characters".to_owned());
    }
    if !basin_id
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        return Err("basin id must match [a-z0-9_-]{1,64}".to_owned());
    }
    Ok(())
}

/// Resolves and loads a validated `RegionalContext` from `context_dir` without blocking Tokio.
pub async fn load_context_async(
    context_dir: PathBuf,
    basin: Option<String>,
) -> Result<ContextResponse, (u16, String)> {
    let file_name = match basin {
        Some(ref id) => {
            validate_basin_id(id).map_err(|e| (400, e))?;
            format!("{id}.json")
        }
        None => "latest.json".to_owned(),
    };

    tokio::task::spawn_blocking(move || load_context_sync(&context_dir, &file_name))
        .await
        .map_err(|e| (500, format!("context loader join error: {e}")))?
}

fn load_context_sync(
    context_dir: &Path,
    file_name: &str,
) -> Result<ContextResponse, (u16, String)> {
    let canonical_dir = std::fs::canonicalize(context_dir)
        .map_err(|e| (404, format!("context directory unavailable: {e}")))?;
    let candidate = canonical_dir.join(file_name);
    let canonical_file = std::fs::canonicalize(&candidate)
        .map_err(|_| (404, format!("context file '{file_name}' not found")))?;
    if !canonical_file.starts_with(&canonical_dir) {
        return Err((400, "context path escapes configured directory".to_owned()));
    }

    let meta = std::fs::metadata(&canonical_file)
        .map_err(|e| (500, format!("failed to stat context file: {e}")))?;
    if meta.len() as usize > MAX_CONTEXT_BYTES {
        return Err((
            413,
            format!("context file exceeds {} byte limit", MAX_CONTEXT_BYTES),
        ));
    }

    let bytes = std::fs::read(&canonical_file)
        .map_err(|e| (500, format!("failed to read context file: {e}")))?;
    let ctx: RegionalContext = serde_json::from_slice(&bytes)
        .map_err(|e| (422, format!("invalid regional context JSON: {e}")))?;
    validate_context(&ctx).map_err(|e| (422, e))?;

    let now = OffsetDateTime::now_utc();
    let cycle_dt = OffsetDateTime::parse(&ctx.cycle_iso, &Rfc3339)
        .map_err(|e| (422, format!("invalid cycle_iso RFC-3339 timestamp: {e}")))?;
    let age_seconds = (now - cycle_dt).whole_seconds().max(0);

    let expired = match ctx.valid_until_iso.as_deref() {
        Some(until) => {
            let until_dt = OffsetDateTime::parse(until, &Rfc3339).map_err(|e| {
                (
                    422,
                    format!("invalid valid_until_iso RFC-3339 timestamp: {e}"),
                )
            })?;
            now > until_dt
        }
        None => false,
    };
    let is_stale = expired || age_seconds > STALE_AFTER_SECS;

    Ok(ContextResponse {
        context: ctx,
        age_seconds,
        is_stale,
    })
}

fn validate_context(ctx: &RegionalContext) -> Result<(), String> {
    if ctx.schema_version != 1 {
        return Err(format!(
            "unsupported regional context schema_version {} (expected 1)",
            ctx.schema_version
        ));
    }
    validate_basin_id(&ctx.basin_id)?;
    let [min_lon, min_lat, max_lon, max_lat] = ctx.wgs84_bounds;
    if !min_lon.is_finite()
        || !min_lat.is_finite()
        || !max_lon.is_finite()
        || !max_lat.is_finite()
        || min_lon >= max_lon
        || min_lat >= max_lat
        || !(-180.0..=180.0).contains(&min_lon)
        || !(-180.0..=180.0).contains(&max_lon)
        || !(-85.0..=85.0).contains(&min_lat)
        || !(-85.0..=85.0).contains(&max_lat)
    {
        return Err(
            "wgs84_bounds must be finite ordered [min_lon, min_lat, max_lon, max_lat]".to_owned(),
        );
    }
    if let Some(mjo) = &ctx.mjo
        && (!(1..=8).contains(&mjo.phase) || !mjo.amplitude.is_finite() || mjo.amplitude < 0.0)
    {
        return Err("mjo.phase must be in 1..=8 and amplitude finite nonnegative".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basin_id_rejects_path_traversal() {
        assert!(validate_basin_id("sg-setiu_01").is_ok());
        assert!(validate_basin_id("../etc/passwd").is_err());
        assert!(validate_basin_id("Setiu").is_err());
        assert!(validate_basin_id("").is_err());
    }

    #[tokio::test]
    async fn context_loader_reads_valid_file_and_detects_staleness() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let stale_json = r#"{
            "schema_version": 1,
            "basin_id": "demo-basin",
            "wgs84_bounds": [102.5, 5.2, 102.9, 5.8],
            "cycle_iso": "2020-01-01T00:00:00Z",
            "hourly_tide_m": [0.4, 1.2, 1.9],
            "mjo": { "phase": 4, "amplitude": 1.6, "label": "Maritime Continent · Active" }
        }"#;
        std::fs::write(dir.path().join("latest.json"), stale_json).map_err(|e| e.to_string())?;

        let resp = load_context_async(dir.path().to_path_buf(), None)
            .await
            .map_err(|(_, m)| m)?;
        assert_eq!(resp.context.basin_id, "demo-basin");
        assert!(resp.is_stale);
        assert!(resp.age_seconds > STALE_AFTER_SECS);
        Ok(())
    }
}
