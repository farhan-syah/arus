//! Hydrological runoff adapters over `hydr::runoff`.
//!
//! Keeps `hydr` and `numr` types behind the hydrology adapter boundary while exposing
//! deterministic CPU `F64` three-subarea subcatchment runoff execution and JSON manifests.

pub mod bridge;
pub mod runoff;
pub mod spec;

pub use bridge::{
    AntecedentState, PeakRunoffWindow, build_runoff_spec_from_hourly, derive_antecedent_state,
    detect_peak_window,
};
pub use runoff::{SubcatchmentEngine, SubcatchmentStepSummary};
pub use spec::{
    RunoffInfiltrationSpec, RunoffIntervalSpec, RunoffRoutingSpec, RunoffRunReport, RunoffRunSpec,
    SubcatchmentSpec,
};
