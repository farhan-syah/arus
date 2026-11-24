//! Deterministic sequential CPU `F64` reference runtime for `hydr`.

use numr::runtime::cpu::{CpuClient, CpuDevice, CpuRuntime, ParallelismConfig};

/// Reference runtime type used for deterministic CPU `F64` execution.
pub type ReferenceRuntime = CpuRuntime;

/// Builds a single-threaded CPU client for reproducible `F64` execution.
pub fn reference_cpu_client() -> CpuClient {
    CpuClient::new(CpuDevice::new()).with_parallelism(ParallelismConfig::new(Some(1), None))
}
