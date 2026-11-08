# ARUS

ARUS is a scaffold for location-specific flood forecasting, monitoring, and scenario simulation in Malaysia.

## Status

- Scope: Initial repository setup only.
- Applications: Native executables print scaffold status and exit.
- Libraries: Responsibility descriptions and declared internal dependency boundaries.
- Forecast scope: Onset, depth evolution, peak, duration, and recession below declared thresholds.
- Integrations: Dioxus, Axum, NodeDB Lite, GIS, and numerical dependencies remain unconfigured.
- Capabilities: No feeds, services, forecasts, simulations, or optimisation run yet.

## Requirements

- Rust 1.98.0, pinned in `rust-toolchain.toml`.
- Rust 1.96 minimum, declared in the workspace manifest.
- Rustfmt and Clippy, declared in the toolchain file.
- Product dependency compatibility remains unchecked.

## Checks

```sh
sh scripts/check.sh
```

The script checks repository paths, formatting, workspace compilation, and strict Clippy. Local execution defaults to offline mode.

Format modified Rust source before running checks.

```sh
cargo fmt --all
```

The scaffold has no external dependencies. Cargo still requires the pinned toolchain and declared components.
GitHub Actions runs the same script with network access enabled.

## Layout

| Path                                                   | Responsibility                                                        |
| ------------------------------------------------------ | --------------------------------------------------------------------- |
| `apps/web`                                             | Future Dioxus browser application                                     |
| `apps/api`                                             | Future Axum public API and database owner                             |
| `apps/worker`                                          | Future numerical job process                                          |
| `apps/cli`                                             | Future operator commands                                              |
| `crates/domain`, `crates/api-types`                    | Domain concepts and public transport contracts                        |
| `crates/model-api`, `crates/assets`, `crates/jobs`      | Numerical backend boundaries, immutable assets, and job execution       |
| `crates/forecasting`, `crates/evaluation`               | Forecast publication and independent event assessment                  |
| `crates/ingest`, `crates/storage`, `crates/geo`        | Ingestion, persistence, and geographic processing                     |
| `crates/hydrology`, `crates/hydraulics`                | Runoff, routing, and surface-flow models                              |
| `crates/scenarios`, `crates/optimisation`, `crates/ml` | Scenario operations, design search, and optional numerical prediction |
| `tests/fixtures`, `benchmarks/manifests`               | Redistributable samples and scientific case manifests                 |
| `docs/`                                               | Public documentation                                                  |

## Local development

Copy the Cargo config template before adding local dependency overrides.

```sh
cp .cargo/config.toml.example .cargo/config.toml
```

- `.cargo/config.toml` stays ignored. Activate path overrides during the dependency spike.
- The template uses example paths for NodeDB and rust-llm checkouts.
- Local `nodedb-lite` uses development NodeDB packages. Check its complete override graph before integration.
- Local `solvr` requires `numr 0.14`. Local `numr` is `0.15.2`.
- Keep incompatible numerical adapters separate. Do not force shared tensor types.
- `data/`, `artifacts/`, and `scratch/` hold ignored local runtime files.
- `Cargo.lock` stays tracked for reproducible workspace resolution.

## Next gate

- Audit candidate catchments and source redistribution rights.
- Resolve dependencies and check native, browser, and numerical builds separately.
- Check NodeDB Lite durability and query capabilities before implementing repositories.
