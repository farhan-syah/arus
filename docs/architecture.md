# Architecture

ARUS separates browser contracts, forecast orchestration, numerical execution, and independent evaluation for developers.

## Current implementation

- The CLI checks the public catalog, plans collection, archives responses, and reports acquisition status.
- `arus-ingest` implements source contracts and collection lifecycle rules.
- `arus-assets` implements immutable raw blobs, acquisition metadata, and archive locking.
- `arus-domain`, `arus-api-types`, `arus-model-api`, and `arus-jobs` define backend-agnostic provenance, transport, capability, water-budget, cancellation, and checkpoint contracts.
- `arus-hydraulics` wraps `hydr 0.1.0` on CPU `F64` (2D surface, pervious Modified Green-Ampt soil, coupled `SurfaceChannelBatch`, 23-component capabilities, exact binary checkpoint codec, and `arus-geo` terrain bridge).
- `arus-hydrology` wraps `hydr::runoff::SubcatchmentBatch` on CPU `F64`.
- `arus-forecasting`, `arus-evaluation`, `arus-geo`, `arus-scenarios`, `arus-optimisation`, `arus-ml`, and `arus-storage` implement deterministic episode tracking, censored scoring, terrain conditioning, paired scenario comparisons, Pareto mitigation screening, ML baseline gating, and archive-backed run persistence.
- `arus-worker`, `arus-api` (Axum), and `arus-web` (bilingual BM/EN browser-safe renderer) connect the vertical pipeline end to end.
- No calibrated Malaysian operational forecasts exist yet; scientific qualification requires surveyed catchment inputs and locked-event evaluation.

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
| `arus-storage`, `arus-ingest`, `arus-geo` | Persistence, source ingestion, and geographic preparation |
| `arus-hydrology`, `arus-hydraulics` | Hydrological and hydraulic model implementations |
| `arus-scenarios`, `arus-optimisation`, `arus-ml` | Conditional interventions, constrained search, and optional numerical prediction |

## Dependency direction

- The web package depends on `arus-api-types`, which depends on `arus-domain`.
- Browser dependencies exclude persistence, native GIS, workers, and numerical engines.
- Model contracts depend on domain concepts and asset references.
- Forecast orchestration depends on job and model boundaries.
- Evaluation depends on model contracts independently of operational forecast publication.
- Storage depends on domain concepts and asset references.
- `apps/api` may invoke `arus-hydraulics` and `arus-geo` inside bounded `spawn_blocking` tasks for interactive numerical demonstrations, keeping results in a bounded in-memory cache separate from the immutable `Archive`.
- Numerical adapters keep upstream engine types outside shared domain and browser contracts (`arus-domain`, `arus-api-types`, `apps/web`).

The manifests declare these boundaries. They do not enforce future scientific correctness or implement runtime isolation.

## Separate outcomes

- Execution completion records that a worker finishes its assigned computation.
- Scientific assessment records whether outputs meet declared evidence and numerical criteria.
- Publication approval records whether a forecast supports the declared area, variable, horizon, and use.

These outcomes require separate contracts when implementation starts.
Job completion alone cannot authorise forecast publication.

## Numerical backends

- Imported results and external engines can implement the model boundary.
- Native Rust hydraulic development remains a separate research track.
- Each backend must declare supported processes and unsupported conversions.
- Checkpoint reuse requires compatible engine, geometry, parameters, and input state.
- Optional numerical prediction adds no language-model requirement.

No numerical backend or cross-process protocol exists in this scaffold.
