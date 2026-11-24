//! Browser-compatible public API transport contracts.
//!
//! Keep native database, GIS, and tensor dependencies outside this crate.

use arus_domain::RunClassification;
use serde::{Deserialize, Serialize};

/// Stable schema version for ARUS public API payloads.
pub const API_SCHEMA_VERSION: &str = "arus-api-v1";

/// Browser-safe capability summary for one numerical backend component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiCapabilitySummary {
    pub backend: String,
    pub component: String,
    pub contract: String,
    pub precision: Vec<String>,
    pub boundaries: Vec<String>,
    pub sources: Vec<String>,
}

/// Explicit spatial support status for a queried location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApiLocationStatus {
    Supported,
    Unsupported,
}

/// Transport summary of one flood episode at a location.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiEpisodeSummary {
    pub onset_status: String,
    pub onset_time_s: f64,
    pub initial_inundation: bool,
    pub peak_depth_m: f64,
    pub peak_time_s: f64,
    pub peak_at_horizon_end: bool,
    pub exceedance_duration_s: f64,
    pub recession_status: String,
    pub recession_crossing_s: Option<f64>,
    pub recession_confirmation_s: Option<f64>,
    pub rebound_count: usize,
}

/// Browser-safe location outlook payload carrying explicit classification and support status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiLocationOutlook {
    pub schema_version: String,
    pub location_id: String,
    pub classification: RunClassification,
    pub support: ApiLocationStatus,
    pub threshold_m: f64,
    pub horizon_start_s: Option<f64>,
    pub horizon_end_s: Option<f64>,
    pub max_depth_m: Option<f64>,
    pub episodes: Vec<ApiEpisodeSummary>,
}

/// Browser-safe summary of one registered data source from `catalog/sources.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiSourceSummary {
    pub id: String,
    pub title: String,
    pub provider: String,
    pub access: String,
    pub data_class: String,
    pub license_name: String,
    pub coverage: String,
}
