# ARUS

**Open-source, physics-first 3D flood forecasting and drainage scenario studio—built for Malaysia, ready for anywhere on Earth.**

![ARUS 3D Flood Simulation Studio](docs/screenshots/studio-3d-flood.png)

## What You Get

- **Global 3D Globe & Terrain Studio**: Pan, zoom, tilt, and rotate anywhere on Earth (with flagship Malaysia presets like Shah Alam, Kuala Lumpur, and Kota Bharu plus global basins) over 3D terrain and buildings.
- **Live 60 FPS Flood Depth & Flow Motion**: Watch real `hydr` 2D shallow-water depth extrusions, animated velocity particle streamlines, **DEFRA FD2320** flood hazard (`HR = h × (v + 0.5)`), and live cursor telemetry (`z`, `h`, `WSE`, `|v|`).
- **Interactive Cross-Section & Hydrograph Inspector**: Draw any profile line `A → B` across a valley or street to inspect terrain bed `z(s)` vs animated water surface `z(s) + h(s)`, or click any cell for its onset/peak/recession hydrograph.
- **On-Map Bund & Detention Pond What-If Testing**: Paint floodwalls (`+0.8 m`) or detention ponds (`-0.8 m`) directly onto the 3D map, calculate earthwork cut/fill (`m³`), and run **Baseline vs Intervention** comparisons highlighting both protected (teal) and downstream worsened (orange) zones.
- **Bring-Your-Own-Data (BYOD) & 1-Click GIS Exports**: Upload local projected ESRI ASCII `.asc` DEMs, tune real hydraulic physics (rainfall `mm/hr`, peak vs uniform storm, Manning `n`, Modified Green-Ampt infiltration, open boundaries), and export `max-depth.asc` or `envelope.csv` for QGIS/reports.
- **Scientific Honesty Built In**: Every interactive run displays its classification (`DEMO • SCREENING ONLY`), live mass-conservation residual (`PASS / FAIL`), explicit `nodata` coverage boundaries, and bilingual `EN` / `BM` controls.

## Screenshots

### Interactive Cross-Section Profile (`z` vs `z + h`) & Live Telemetry

Draw a line across any flooded corridor to plot real-time bed elevation and water surface elevation alongside hover telemetry.

![ARUS Cross-Section Profile Inspector](docs/screenshots/studio-profile-inspector.png)

### Baseline vs Intervention `Δh` Impact Map (Bund + Detention Pond)

Paint floodwalls or detention ponds directly on the map and compare maximum flood depth before and after—with earthwork cut/fill volumes and colorblind-safe protected vs worsened zones.

![ARUS Baseline vs Intervention Diff Map](docs/screenshots/studio-intervention-diff.png)

## Quick Start

Requires Rust `1.98.0` (pinned in `rust-toolchain.toml`).

```sh
# Launch the 3D Flood Simulation Studio and JSON/Binary API
cargo run --release -p arus-api -- --archive data/runs --bind 127.0.0.1:8080
```

Open `http://127.0.0.1:8080/?lang=en` (or `?lang=bm`) in your browser.

## What To Expect Today

- **Ready to run**: Full-screen 3D Flood Simulation Studio, real-time `hydr 0.1.0` `F64` 2D shallow-water solver, Modified Green-Ampt infiltration, 1D–2D river bank coupling, coupled detention pools (weirs, orifices, sluice gates, pumps, HDS-5 culverts), urban subcatchment runoff, paired scenario comparison, `.asc`/`.csv` GIS exports, embedded `nodedb-lite` storage, and offline-capable vendored MapLibre GL JS v5 assets.
- **Not yet operational**: Live calibrated operational flood forecasts require surveyed local bare-earth LiDAR, local drainage geometry, official gauge/radar archives, and independent locked-event evaluation. Until approved, every run carries a persistent `DEMO • SCREENING ONLY` badge.

## Documentation & Verification

- [Architecture & Workspace Boundaries](docs/architecture.md)
- [Public HTTP API (`arus-api-v1`)](docs/api/README.md)
- [Analytical & Coupled Benchmarks](benchmarks/manifests/README.md)
- [Data Acquisition Guide](docs/data-acquisition.md)

```sh
sh scripts/check.sh --offline
```
