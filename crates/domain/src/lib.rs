//! Shared domain concepts, spatial references, and run provenance.
//!
//! This crate stays free of persistence, native GIS, and numerical solver dependencies.

use serde::{Deserialize, Serialize};

/// Caller-attested projected CRS, vertical datum, and grid origin in metres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpatialReference {
    pub projected_crs: String,
    pub vertical_datum: String,
    pub origin_m: [f64; 2],
}

/// Cartesian grid dimensions in canonical SI metres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GridSpec {
    pub columns: usize,
    pub rows: usize,
    pub dx_m: f64,
    pub dy_m: f64,
}

impl GridSpec {
    /// Returns total row-major cell count if representable.
    pub fn cell_count(self) -> Option<usize> {
        self.columns.checked_mul(self.rows)
    }
}

/// Explicit purpose classification for a numerical run.
///
/// Numerical demonstrations and reconstructions never imply operational forecast approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunClassification {
    NumericalDemonstration,
    EventReconstruction,
    ForecastReplay,
    OperationalForecast,
}

impl RunClassification {
    /// Returns the stable kebab-case identifier.
    pub const fn id(self) -> &'static str {
        match self {
            Self::NumericalDemonstration => "numerical-demonstration",
            Self::EventReconstruction => "event-reconstruction",
            Self::ForecastReplay => "forecast-replay",
            Self::OperationalForecast => "operational-forecast",
        }
    }
}

/// Immutable provenance metadata attached to a numerical execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub backend: String,
    pub contract: String,
    pub classification: RunClassification,
    pub note: String,
}
