# Architecture

ARUS separates browser contracts, forecast orchestration, numerical execution, and independent evaluation for developers.

## Current implementation

- The CLI checks the public catalog, plans collection, archives responses, and reports acquisition status.
- `arus-ingest` implements source contracts and collection lifecycle rules.
- `arus-assets` implements immutable raw blobs, acquisition metadata, and archive locking.
- Other application entry points and scientific libraries remain scaffold boundaries.
- No hydraulic models, flood predictions, or forecast publication rules exist yet.

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
- Numerical adapters keep upstream engine types outside shared domain and browser contracts.

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
