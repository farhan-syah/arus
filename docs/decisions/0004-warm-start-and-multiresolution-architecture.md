# ADR 0004: Warm-Start Initialization, State Persistence, and Multi-Resolution Catchment Architecture

Status: Accepted  
Date: 2026-11-27

## Context

ARUS runs continuous flood nowcasts and 5-day forecasts using the `hydr` 2D shallow-water (MUSCL) and single-layer Modified Green-Ampt infiltration solver. Operational deployments require an accurate initial state (`t = 0`) without accumulating multi-month forcing drift, and must scale from regional screening to local urban flood extent without uniform fine-grid explosion.

## Decisions

### 1. Hydrological Connectivity Over Arbitrary Boxes

- **Catchment boundaries define domains.** Every simulation domain covers a complete hydrologically connected catchment from headwaters to outlet.
- **No disconnected river splits.** Cutting one river into independent upstream and downstream boxes breaks inflow continuity and coastal backwater.
- **Basin boundaries are separate from compute grids.** One large catchment can contain multiple coupled computational resolutions.

### 2. Physics-Grounded Warm-Start Policy

`hydr` models 2D overland flow plus a single-layer Modified Green-Ampt soil store (`max_deficit` typically `0.02–0.12 m`). It does not model deep multi-month groundwater aquifers. Soil saturation stabilizes in days, while floodplain ponding, channel travel time, and coastal backwater govern surface convergence.

| Window | Operational role |
| --- | --- |
| `7 days` | Fast-response steep or urban catchment candidate |
| `14 days` | Standard initialization candidate |
| `21 days` | Intermediate floodplain candidate |
| `30 days` | Default operational ceiling |
| `45–60 days` | Diagnostic-only test when convergence fails |
| `90–180 days` | Out of scope for single-layer Green-Ampt (accumulates forcing drift without aquifer physics) |

#### Initialization and Convergence Criteria

- **Physically mapped soil initialization.** Volumetric soil moisture (`theta`) from an NWP land-surface model (such as ECMWF HTESSEL `0–7 cm`, `7–28 cm`, `28–100 cm`) must not be copied verbatim into Green-Ampt deficit. Convert it using soil porosity (`theta_sat`), wilting point (`theta_wp`), and configured `max_deficit`:
  - `sat_frac = clamp((theta - theta_wp) / (theta_sat - theta_wp), 0.0, 1.0)`
  - `initial_deficit = max_deficit * (1.0 - sat_frac)`
- **Dry vs saturated convergence check.** Validate warm-start duration by running each catchment from both a dry start (`deficit = max_deficit`, `h = 0`) and a saturated start (`deficit = 0`, `h = 0`).
- **Target convergence tolerances (experimental).**
  - Floodplain inundation depth difference: `< 0.02 m`
  - River-stage difference: `< 0.05 m`
  - River discharge difference: `< 5%`
  - Effective soil moisture difference: `< 0.01 m^3/m^3`
- **DEM conditioning and channel baseline.** Apply Priority-Flood sink conditioning (`eps > 0`) to coarse global DEMs so artificial closed depressions drain downstream. Subtract a matched-window climatological normal-flow run (`h_flood = max(0, h_sim - h_norm)`) to separate permanent water bodies from overbank flooding.

### 3. Continuous State Persistence (Rolling Checkpoints)

- **Replace repeated cold spin-ups with rolling checkpoints.** Once a catchment passes validation at `t = 0`, operational cycles persist and reload both `depth_grid` and per-cell infiltration state (`deficit_grid`) every forecast cycle (e.g., 6 hours).
- **Compatibility guard.** Checkpoint reuse requires matching solver version, grid geometry, terrain hash, and parameter config (per `arus-model-api`).

### 4. Four Independent Resolution Layers

A fine hydraulic mesh cannot create fine rainfall or fine terrain that is absent from inputs. ARUS manages four resolutions independently:

| Layer | Resolution strategy |
| --- | --- |
| **Meteorological forcing** | Native NWP grid (`~9 km`), blended with local rain gauges and weather radar where available |
| **Terrain (DEM)** | Highest validated bare-earth DEM available (`30 m` open global, `1–5 m` LiDAR where surveyed) |
| **Hydraulic computation** | Hierarchical multi-resolution grids + subgrid hydraulics |
| **Output visualization** | Scoped to the physical accuracy scale of the underlying run |

### 5. Hierarchical Multi-Resolution Architecture

Uniform fine grids do not scale: a `10,000 km^2` basin requires `160,000` cells at `250 m`, `1,000,000` cells at `100 m`, and `11,111,111` cells at `30 m`, with explicit CFL time steps shrinking proportionally. ARUS adopts coupled resolution levels inside each connected catchment:

| Level | Target cell size (`dx`) | Spatial coverage | Physical scope |
| --- | --- | --- | --- |
| **L0** | `500–1,000 m` | Full catchment | Rainfall-runoff generation, upstream inflow, regional screening |
| **L1** | `30–100 m` | River corridors and floodplains | Overbank extent, floodplain depth, channel-floodplain exchange |
| **L2** | `5–30 m` | Towns and critical infrastructure | Urban inundation, per-cell `storm_drain_grid`, road corridors |
| **L3** | `1–5 m` | Selected high-risk urban sites | Street-level hydraulics (requires surveyed LiDAR + drainage geometry) |

### 6. Solver Roadmap (`hydr` v0.4+)

- **Per-cell urban drainage.** Replace uniform basin drainage scalars with spatially varying `storm_drain_grid` (built-up impervious vs rural floodplain).
- **Subgrid hydraulics (HEC-RAS class).** Retain sub-cell DEM detail inside coarser computational cells via:
  - Elevation-volume storage curves per cell
  - Partial cell inundation
  - Face-level hydraulic conveyance and terrain-barrier preservation
  - Subgrid 1D channel representation
  - Conservative coarse-to-fine flux exchange and two-way nested coupling
- **Ensemble uncertainty.** Run multiple rainfall and initial-state scenarios once single-member state persistence is stable.

### 7. Scientific Validation Gate

Job completion never authorizes operational publication. Every initialization formula and resolution upgrade must pass independent evaluation across contrasting catchments (active flood, urban flash-flood, and quiet baseline) on:

- **True positives**: flooded reference points and gauges above Alert/Warning/Danger
- **True negatives**: known dry towns and quiet basins remain dry (zero artificial DEM lakes)
- **Recession behavior**: floodwater rises on rain and drains during dry or light-rain hours
- **Extent, depth, timing, and alert count**: merged alert clusters match operational scale
