//! Server runtime configuration and feature toggles for local vs hosted ARUS deployments.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::sim::{JOB_TIMEOUT_SECS, MAX_INTERACTIVE_CELLS, MAX_JOB_CELLS, SIM_TIMEOUT_SECS};

/// Deployment mode distinguishing self-hosted power-user runs from public hosted servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeMode {
    #[default]
    Local,
    Hosted,
}

/// Default Studio UI profile exposed to visitors before browser-local overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UiProfile {
    Public,
    Researcher,
}

/// Interactive simulation compute mode (`hybrid` allows browser WASM offload with server fallback).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionMode {
    #[default]
    Hybrid,
    ServerOnly,
}

/// Feature toggles enforced server-side by `arus-api` and mirrored in Studio UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureFlags {
    #[serde(default = "default_true")]
    pub allow_simulation: bool,
    #[serde(default = "default_true")]
    pub allow_interventions: bool,
    #[serde(default = "default_true")]
    pub allow_byod_dem_upload: bool,
    #[serde(default = "default_true")]
    pub allow_mp4_export: bool,
    #[serde(default = "default_true")]
    pub allow_async_jobs: bool,
    #[serde(default = "default_true")]
    pub allow_direct_nwp_fetch: bool,
    #[serde(default = "default_true")]
    pub allow_gis_export: bool,
}

const fn default_true() -> bool {
    true
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            allow_simulation: true,
            allow_interventions: true,
            allow_byod_dem_upload: true,
            allow_mp4_export: true,
            allow_async_jobs: true,
            allow_direct_nwp_fetch: true,
            allow_gis_export: true,
        }
    }
}

impl FeatureFlags {
    /// Conservative defaults for public-facing hosted deployments.
    pub const fn hosted_default() -> Self {
        Self {
            allow_simulation: true,
            allow_interventions: true,
            allow_byod_dem_upload: false,
            allow_mp4_export: false,
            allow_async_jobs: false,
            allow_direct_nwp_fetch: false,
            allow_gis_export: true,
        }
    }
}

/// Resource limits advertised via `GET /api/v1/config`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerLimits {
    #[serde(default = "default_max_interactive_cells")]
    pub max_interactive_cells: usize,
    #[serde(default = "default_max_job_cells")]
    pub max_job_cells: usize,
    #[serde(default = "default_sim_timeout_secs")]
    pub sim_timeout_secs: u64,
    #[serde(default = "default_job_timeout_secs")]
    pub job_timeout_secs: u64,
}

const fn default_max_interactive_cells() -> usize {
    MAX_INTERACTIVE_CELLS
}
const fn default_max_job_cells() -> usize {
    MAX_JOB_CELLS
}
const fn default_sim_timeout_secs() -> u64 {
    SIM_TIMEOUT_SECS
}
const fn default_job_timeout_secs() -> u64 {
    JOB_TIMEOUT_SECS
}

impl Default for ServerLimits {
    fn default() -> Self {
        Self {
            max_interactive_cells: MAX_INTERACTIVE_CELLS,
            max_job_cells: MAX_JOB_CELLS,
            sim_timeout_secs: SIM_TIMEOUT_SECS,
            job_timeout_secs: JOB_TIMEOUT_SECS,
        }
    }
}

/// Default map viewport applied when neither URL query parameters nor user browser preferences override it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefaultViewConfig {
    #[serde(default = "default_lat")]
    pub lat: f64,
    #[serde(default = "default_lon")]
    pub lon: f64,
    #[serde(default = "default_zoom")]
    pub zoom: f64,
    #[serde(default = "default_basemap")]
    pub basemap: String,
}

const fn default_lat() -> f64 {
    3.031
}
const fn default_lon() -> f64 {
    101.535
}
const fn default_zoom() -> f64 {
    14.2
}
fn default_basemap() -> String {
    "topo".to_owned()
}

impl Default for DefaultViewConfig {
    fn default() -> Self {
        Self {
            lat: default_lat(),
            lon: default_lon(),
            zoom: default_zoom(),
            basemap: default_basemap(),
        }
    }
}

/// Preset location entry advertised via `GET /api/v1/config` for deployment-specific quick jumps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresetLocation {
    pub id: String,
    pub name: String,
    pub lon: f64,
    pub lat: f64,
    pub zoom: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary_query: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_basin: Option<String>,
}

/// Server runtime configuration loaded by `arus-api --config <path>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub mode: RuntimeMode,
    #[serde(default)]
    pub default_ui_profile: Option<UiProfile>,
    #[serde(default)]
    pub execution_mode: ExecutionMode,
    #[serde(default = "default_instance_title")]
    pub instance_title: String,
    #[serde(default)]
    pub default_view: DefaultViewConfig,
    #[serde(default)]
    pub features: FeatureFlags,
    #[serde(default)]
    pub limits: ServerLimits,
    #[serde(default)]
    pub disclaimer: Option<String>,
    #[serde(default)]
    pub public_base_url: Option<String>,
    #[serde(default)]
    pub meta_description: Option<String>,
    #[serde(default)]
    pub meta_keywords: Option<String>,
    #[serde(default)]
    pub alert_bounds: Option<[f64; 4]>,
    #[serde(default)]
    pub presets: Vec<PresetLocation>,
}

const fn default_schema_version() -> u32 {
    1
}
fn default_instance_title() -> String {
    "ARUS Flood Studio".to_owned()
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            schema_version: 1,
            mode: RuntimeMode::Local,
            default_ui_profile: None,
            execution_mode: ExecutionMode::Hybrid,
            instance_title: default_instance_title(),
            default_view: DefaultViewConfig::default(),
            features: FeatureFlags::default(),
            limits: ServerLimits::default(),
            disclaimer: None,
            public_base_url: None,
            meta_description: None,
            meta_keywords: None,
            alert_bounds: None,
            presets: Vec::new(),
        }
    }
}

impl ServerConfig {
    /// Resolves the effective UI profile (`Hosted -> Public`, `Local -> Researcher`) when omitted.
    #[must_use]
    pub const fn effective_ui_profile(&self) -> UiProfile {
        match self.default_ui_profile {
            Some(profile) => profile,
            None => match self.mode {
                RuntimeMode::Hosted => UiProfile::Public,
                RuntimeMode::Local => UiProfile::Researcher,
            },
        }
    }

    /// Preset for public-facing hosted deployments with expensive endpoints gated.
    pub fn hosted_default() -> Self {
        Self {
            mode: RuntimeMode::Hosted,
            features: FeatureFlags::hosted_default(),
            disclaimer: Some(
                "Screening simulation only. Follow official national hydrological and meteorological warnings."
                    .to_owned(),
            ),
            ..Self::default()
        }
    }

    /// Reads and validates a JSON `ServerConfig` file from disk.
    pub fn load_json(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read server config {}: {e}", path.display()))?;
        let cfg: Self = serde_json::from_str(&raw)
            .map_err(|e| format!("invalid server config JSON {}: {e}", path.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Validates coordinate bounds, schema version, and positive limits.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "unsupported server config schema_version {} (expected 1)",
                self.schema_version
            ));
        }
        if !self.default_view.lat.is_finite()
            || !(-85.0..=85.0).contains(&self.default_view.lat)
            || !self.default_view.lon.is_finite()
            || !(-180.0..=180.0).contains(&self.default_view.lon)
        {
            return Err("default_view lat/lon out of valid bounds".to_owned());
        }
        if !self.default_view.zoom.is_finite() || !(1.0..=20.0).contains(&self.default_view.zoom) {
            return Err("default_view zoom must be in [1, 20]".to_owned());
        }
        if !matches!(self.default_view.basemap.as_str(), "topo" | "sat" | "dark") {
            return Err(format!(
                "default_view basemap '{}' must be 'topo', 'sat', or 'dark'",
                self.default_view.basemap
            ));
        }
        if self.limits.max_interactive_cells < 16
            || self.limits.sim_timeout_secs == 0
            || self.limits.job_timeout_secs == 0
        {
            return Err("limits must specify positive cells and timeouts".to_owned());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_default_enables_all_power_user_features() -> Result<(), String> {
        let cfg = ServerConfig::default();
        cfg.validate()?;
        assert_eq!(cfg.mode, RuntimeMode::Local);
        assert!(cfg.features.allow_interventions);
        assert!(cfg.features.allow_byod_dem_upload);
        assert!(cfg.features.allow_mp4_export);
        assert!(cfg.features.allow_async_jobs);
        assert!(cfg.features.allow_direct_nwp_fetch);
        assert!(cfg.features.allow_gis_export);
        Ok(())
    }

    #[test]
    fn hosted_default_gates_heavy_endpoints_and_rejects_unknown_fields() -> Result<(), String> {
        let hosted = ServerConfig::hosted_default();
        hosted.validate()?;
        assert_eq!(hosted.mode, RuntimeMode::Hosted);
        assert!(!hosted.features.allow_mp4_export);
        assert!(!hosted.features.allow_async_jobs);
        assert!(!hosted.features.allow_byod_dem_upload);
        assert!(hosted.features.allow_interventions);

        let bad = r#"{"schema_version":1,"unknown_flag":true}"#;
        assert!(serde_json::from_str::<ServerConfig>(bad).is_err());
        Ok(())
    }
}
