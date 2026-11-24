//! Hydraulic simulation adapter over `hydr`.
//!
//! Keeps `hydr` and `numr` types behind the ARUS adapter boundary while exposing
//! checked capabilities, CPU `F64` execution, exact binary checkpoints, coupled
//! surface-channel execution, manifest-driven specs, and demonstrations.

pub mod capabilities;
pub mod codec;
pub mod coupled;
pub mod demo;
pub mod runtime;
pub mod spec;
pub mod surface;

pub use capabilities::{all_component_capabilities, check_surface_admission, surface_capabilities};
pub use codec::{decode_snapshot, encode_snapshot};
pub use coupled::{BankExchangeSummary, CoupledSurfaceChannelEngine};
pub use demo::{
    ClosedBasinDemoReport, NativeWorkerDemoReport, run_closed_basin_demo, run_native_worker_demo,
};
pub use spec::{LandSpec, ProbeSpec, RainIntervalSpec, SurfaceRunSpec};
pub use surface::{LocationProbe, ProbeSample, SurfaceEngine, domain_from_terrain};
