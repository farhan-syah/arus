# ADR 0003 — Hybrid Browser WASM & Server GPU Compute Strategy

## Status
Accepted

## Decision
Use one physics core (`hydr 0.3.0` on `numr 0.17.0`) across all execution targets rather than maintaining a separate JS/WGSL shallow-water solver.

- **Server F64 Reference**: Interactive runs execute on `arus-api` (`F64` CPU with canonical SHA-256 content-hash caching) or queued `/api/v1/jobs`. Native server GPUs accelerate `SurfaceSolverF32` via `hydr/wgpu` (Vulkan/Metal/DX12).
- **Browser WebGPU (`WebGPU · F32`)**: `crates/wasm-solver` compiles `hydr 0.3.0` (`wgpu`) + `numr 0.17.0` (`wgpu`) to `wasm32-unknown-unknown` and runs `SurfaceSolverF32<WgpuRuntime>` via `WgpuClient::new_async`, `advance_controlled_async`, `state_async`, and `SurfaceF32OpenMasks` inside a Web Worker (`apps/web/assets/sim-worker.js`) using Transferable `ArrayBuffer` (never `SharedArrayBuffer`, which breaks cross-origin MapLibre/Terrarium tiles).
- **Browser WASM CPU (`WASM · F32`)**: Automatically falls back to `SurfaceSolverF32<CpuRuntime>` inside the same Web Worker when WebGPU is absent, and falls back to `POST /api/v1/simulations` (`Server · F64`) when WASM assets are unavailable or `Server F64 Reference` is selected.

## Consequences
- Zero duplicated shallow-water equations between client and server.
- Certified `F64` calibration, archival checkpoints, and deterministic `presentation=1` broadcasts remain pinned to server `F64`.
