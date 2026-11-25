# Architecture

ARUS separates browser contracts, forecast orchestration, numerical execution, and independent evaluation across workspace crates.

## Current implementation

- **Acquisition (`arus-cli`, `arus-ingest`, `arus-assets`)**: Checks the catalog, plans collection, archives payloads with SHA-256 receipts, and locks archives.
- **Contracts (`arus-domain`, `arus-api-types`, `arus-model-api`, `arus-jobs`)**: Defines backend-agnostic provenance, transport, capability, water-budget, cancellation, and checkpoint contracts.
- **Hydraulics (`arus-hydraulics`)**: Wraps `hydr 0.2.0` `F64` and `F32` MUSCL surface solvers, Modified Green-Ampt soil, coupled 1D–2D channels, and coastal forcing. Supports Preissmann-slot pipes, detention pools, HDS-5 culverts, 25-component capabilities, and exact binary checkpoints.
- **Hydrology (`arus-hydrology`)**: Wraps `hydr::runoff::SubcatchmentBatch` and translates hourly NWP series into antecedent soil states and peak runoff windows.
- **Forecasting & Evaluation (`arus-forecasting`, `arus-evaluation`, `arus-geo`)**: Runs episode tracking, censored scoring, and Priority-Flood DEM conditioning.
- **Scenarios & Storage (`arus-scenarios`, `arus-optimisation`, `arus-ml`, `arus-storage`)**: Runs paired diffs, Pareto screening, ML baseline gating, and `nodedb-lite` archive persistence.
- **Worker & Studio (`arus-worker`, `arus-api`, `arus-web`)**: Connects the 4-stage forecast pipeline and interactive 3D Studio. Exports pure-Rust `vidr 0.1.1` H.264 `.mp4` via `arus-worker record` and `POST /api/v1/export/mp4`.
- **Operational Status**: No calibrated operational forecasts exist yet. Scientific qualification requires surveyed bare-earth LiDAR, drainage geometry, and locked-event evaluation.

## Boundaries

| Package | Responsibility |
| --- | --- |
| `arus-domain` | Shared quantities, quality, identity, and provenance |
| `arus-api-types` | Browser-compatible transport contracts |
| `arus-assets` | Immutable manifests and bulk asset references |
| `arus-model-api` | Backend capabilities, result contracts, and compatible checkpoints |
| `arus-jobs` | Worker ownership, execution attempts, cancellation, and resource budgets |
| `arus-forecasting` | Forecast cycles, location evolution, eligibility, and ordered publication |
| `arus-evaluation` | Reconstruction, as-issued replay, independent scoring, and censored outcomes |
| `arus-storage`, `arus-ingest`, `arus-geo` | Persistence, source ingestion, and terrain preparation |
| `arus-hydrology`, `arus-hydraulics` | Hydrological and hydraulic numerical solvers |
| `arus-scenarios`, `arus-optimisation`, `arus-ml` | Conditional interventions, constrained search, and optional numerical prediction |

## Dependency direction

- `apps/web` depends on `arus-api-types`, which depends on `arus-domain`.
- Browser packages exclude persistence, native GIS, workers, and numerical engines.
- Model contracts depend on domain concepts and asset references.
- Forecast orchestration depends on job and model boundaries.
- Evaluation depends on model contracts independently of operational forecast publication.
- Storage depends on domain concepts and asset references.
- `apps/api` runs `arus-hydraulics` and `arus-geo` in bounded `spawn_blocking` tasks (`<= 16,384` cells, `20s` timeout). Interactive runs stay in an in-memory LRU cache separate from `Archive`.
- Numerical adapters keep upstream `hydr` types outside shared domain and browser contracts (`arus-domain`, `arus-api-types`, `apps/web`).

## Separate outcomes

- **Execution completion**: Records that a worker finishes its assigned computation.
- **Scientific assessment**: Records whether outputs meet declared evidence and water-budget criteria.
- **Publication approval**: Records whether a forecast supports the declared area, variable, horizon, and use.

Job completion alone cannot authorise forecast publication.

## Numerical backends

- Imported results and external engines can implement the model boundary.
- Each backend must declare supported processes and unsupported conversions.
- Checkpoint reuse requires compatible engine, geometry, parameters, and input state.
- Optional numerical prediction adds no language-model requirement.
