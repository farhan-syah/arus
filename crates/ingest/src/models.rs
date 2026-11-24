use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub sources: Vec<Source>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub title: String,
    pub provider: String,
    pub description: String,
    pub data_class: DataClass,
    pub access: AccessClass,
    pub connector: ConnectorKind,
    pub documentation_urls: Vec<String>,
    pub license: License,
    pub coverage: String,
    pub limitations: Vec<String>,
    pub reviewed_on: String,
    pub refresh_seconds: Option<u64>,
    pub provider_min_interval_seconds: u64,
    pub max_response_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct License {
    pub name: String,
    pub url: String,
    pub noncommercial_only: bool,
    pub redistribution: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataClass {
    Observation,
    SatelliteEstimate,
    Forecast,
    StitchedForecastHistory,
    Reanalysis,
    Terrain,
    Features,
    Imagery,
    GeneralGuidance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessClass {
    Public,
    PublicViewOnly,
    Registration,
    RequestOnly,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorKind {
    DataGovWeather,
    OpenMeteoForecast,
    OpenMeteoSingleRun,
    OpenMeteoHistory,
    OpenMeteoMarine,
    OpenMeteoFlood,
    CdseCatalogue,
    CatalogOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Collections {
    pub schema_version: u32,
    pub collections: Vec<Collection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Collection {
    pub id: String,
    pub source_id: String,
    pub description: String,
    pub enabled: bool,
    pub accept_noncommercial: bool,
    pub parameters: serde_json::Value,
}
