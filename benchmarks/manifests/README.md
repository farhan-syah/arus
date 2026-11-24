# Scientific case manifests

Reproducible `SurfaceRunSpec` manifests checked by `crates/hydraulics/tests/analytical_benchmarks.rs` on `hydr 0.1.0` CPU `F64`.

| Manifest | Physical Check | Expected Behaviour |
| --- | --- | --- |
| `closed_basin_uniform_rain.json` | Impervious closed basin under constant rain (`1e-4 m/s`, `60 s`) | Uniform depth `0.006 m` within `1e-9 m`, zero velocity, water budget passes |
| `lake_at_rest_flat_bed.json` | Still water (`0.5 m` depth) over flat bed (`1.0 m` elevation) for `30 s` | Depth stays `0.5 m` within `1e-12 m`, velocity stays `0.0 m/s`, water budget passes |
| `pervious_green_ampt_infiltration.json` | Uniform rain over Modified Green-Ampt pervious soil for `60 s` | Surface depth stays below impervious `0.006 m`, total surface + soil water budget passes |
| `open_stage_drainage.json` | Initial `0.4 m` ponded water draining across subcritical `0.3 m` stage faces for `20 s` | Positive outflow (`outputs_m3 > 0`), depth drops below `0.4 m`, water budget passes |
| `subcatchment_urban_runoff.json` | Three-subarea urban subcatchment with Modified Green-Ampt soil (`arus-worker runoff`) | Positive infiltration and outlet runoff over `120 s`, residual `< 1e-9 m^3` |
| `coupled_surface_channel_bank_weir.json` | Coupled 2D floodplain + 1D rectangular channel over bank weir (`arus-worker coupled`) | Positive surface-to-channel bank transfer over `1 s`, composite water budget passes |
| `coupled_pools_structures.json` | 3 detention pools connected by sluice gate + lift pump (`arus-worker pools`) | Gate drains detention pond, pump lifts to outfall basin, total volume residual `< 1e-9 m^3` |
