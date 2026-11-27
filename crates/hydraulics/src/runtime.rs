//! Hydraulic execution runtime for `hydr` (deterministic CPU by default, CUDA when `--features cuda`).

#[cfg(not(feature = "cuda"))]
use numr::runtime::cpu::{CpuClient, CpuDevice, CpuRuntime, ParallelismConfig};
#[cfg(feature = "cuda")]
use numr::runtime::cuda::{CudaClient, CudaDevice, CudaRuntime};

/// Active runtime type used for `hydr` execution (`CpuRuntime` by default, `CudaRuntime` with `--features cuda`).
#[cfg(not(feature = "cuda"))]
pub type ReferenceRuntime = CpuRuntime;

/// Active runtime type used for `hydr` execution (`CpuRuntime` by default, `CudaRuntime` with `--features cuda`).
#[cfg(feature = "cuda")]
pub type ReferenceRuntime = CudaRuntime;

/// Builds the active `hydr` execution client (single-threaded CPU by default, CUDA GPU when `--features cuda`).
#[cfg(not(feature = "cuda"))]
pub fn reference_cpu_client() -> CpuClient {
    CpuClient::new(CpuDevice::new()).with_parallelism(ParallelismConfig::new(Some(1), None))
}

/// Builds the active `hydr` CUDA client (device ordinal via `ARUS_CUDA_DEVICE`, default `0`).
#[cfg(feature = "cuda")]
pub fn reference_cpu_client() -> CudaClient {
    let ordinal = std::env::var("ARUS_CUDA_DEVICE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    CudaClient::new(CudaDevice::new(ordinal)).expect("failed to initialize CUDA device")
}
