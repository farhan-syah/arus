// ARUS Browser Simulation Web Worker (Transferable ArrayBuffer, zero SharedArrayBuffer).
// Runs hydr 0.3.0 SurfaceSolverF32 on WebGPU first, falling back to WASM CPU automatically.

let wasmInitPromise = null;
let wasmApi = null;
let webGpuAdapterPromise = null;

async function ensureWasm() {
  if (wasmApi) return wasmApi;
  if (!wasmInitPromise) {
    wasmInitPromise = (async () => {
      const mod = await import('/assets/wasm/arus_wasm_solver.js');
      await mod.default({ module_or_path: '/assets/wasm/arus_wasm_solver_bg.wasm' });
      wasmApi = mod;
      return mod;
    })();
  }
  return wasmInitPromise;
}

async function hasUsableWebGpuAdapter() {
  if (!webGpuAdapterPromise) {
    webGpuAdapterPromise = (async () => {
      try {
        if (typeof navigator === 'undefined' || !navigator.gpu) return false;
        const adapter = await navigator.gpu.requestAdapter();
        return Boolean(adapter);
      } catch (_) {
        return false;
      }
    })();
  }
  return webGpuAdapterPromise;
}

self.onmessage = async (event) => {
  const { id, payloadJson, preferWebGpu } = event.data || {};
  try {
    const mod = await ensureWasm();
    let result = null;
    if (preferWebGpu !== false && (await hasUsableWebGpuAdapter())) {
      try {
        result = await mod.run_surface_webgpu(payloadJson);
      } catch (_) {
        // Automatic fallback to WASM CPU when WebGPU device creation or dispatch fails.
        result = mod.run_surface_wasm_cpu(payloadJson);
      }
    } else {
      result = mod.run_surface_wasm_cpu(payloadJson);
    }

    const framesBin = result.framesBin;
    const buffer =
      framesBin.byteOffset === 0 && framesBin.byteLength === framesBin.buffer.byteLength
        ? framesBin.buffer
        : framesBin.slice().buffer;
    self.postMessage(
      {
        id,
        ok: true,
        engine: result.engine,
        gridJson: result.gridJson,
        framesBuffer: buffer,
      },
      [buffer],
    );
  } catch (err) {
    self.postMessage({
      id,
      ok: false,
      error: err && err.message ? err.message : String(err),
    });
  }
};
