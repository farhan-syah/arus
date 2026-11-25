# ADR 0003 — Hybrid Browser WASM & Server GPU Compute Strategy

## Status
Accepted

## Decision
Use one physics core (`hydr 0.2.0`) across all execution targets rather than maintaining a separate JS/WGSL shallow-water solver.

- **Today**: Interactive runs execute on `arus-api` (`F64` CPU with canonical SHA-256 content-hash caching) or queued `/api/v1/jobs`. Native server GPUs can accelerate `SurfaceEngineF32` via `hydr/wgpu` (Vulkan/Metal/DX12).
- **Browser WASM CPU**: Runs `hydr` (`default-features = false`, no `rayon`) in a Web Worker using Transferable `ArrayBuffer` (never `SharedArrayBuffer`, which breaks cross-origin MapLibre/Terrarium tiles).
- **Browser WebGPU prerequisite**: `numr 0.16.0` `WgpuClient` currently uses `pollster::block_on` and blocking `mpsc::recv()` for per-step CFL readback, which deadlocks on `wasm32`. Browser WebGPU offload unlocks once `numr` ships `WgpuClient::new_async` and `read_buffer_async`, and `hydr` exposes `SurfaceSolverF32::advance_to_async`.

## Consequences
- Zero duplicated shallow-water equations between client and server.
- Certified `F64` calibration, archival checkpoints, and deterministic `presentation=1` broadcasts remain pinned to server `F64`.
