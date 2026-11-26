//! Top-level umbrella crate for **ARUS**, an open-source 2D flood modeling,
//! hydrology, hydraulics, and forecasting platform in Rust.
//!
//! Re-exports the core ARUS domain, geospatial, hydrology, hydraulics,
//! forecasting, scenario, evaluation, and optimisation crates.

pub use arus_domain as domain;
pub use arus_evaluation as evaluation;
pub use arus_forecasting as forecasting;
pub use arus_geo as geo;
pub use arus_hydraulics as hydraulics;
pub use arus_hydrology as hydrology;
pub use arus_optimisation as optimisation;
pub use arus_scenarios as scenarios;
