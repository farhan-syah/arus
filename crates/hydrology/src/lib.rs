//! Hydrological runoff adapters over `hydr::runoff`.
//!
//! Keeps `hydr` and `numr` types behind the hydrology adapter boundary while exposing
//! deterministic CPU `F64` three-subarea subcatchment runoff execution.

pub mod runoff;

pub use runoff::{SubcatchmentEngine, SubcatchmentStepSummary};
