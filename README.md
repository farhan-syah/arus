# ARUS

ARUS is a scaffold for location-specific flood forecasting, monitoring, and scenario simulation in Malaysia.

## Status

- Scope: Public data catalog and repeatable raw-data acquisition.
- CLI: Inspect sources, check recipes, collect snapshots, and report acquisition status.
- Other applications: Native executables print scaffold status and exit.
- Forecast scope: Onset, depth evolution, peak, duration, and recession below declared thresholds.
- Acquisition: Government weather, numerical forecast recipes, and satellite metadata discovery.
- Product integrations: Dioxus, Axum, NodeDB Lite, GIS, and numerical engines remain unconfigured.
- Capabilities: No flood predictions, hydraulic simulations, or optimisation run yet.

## Requirements

- Rust 1.98.0, pinned in `rust-toolchain.toml`.
- Rust 1.96 minimum, declared in the workspace manifest.
- Rustfmt and Clippy, declared in the toolchain file.
- Product dependency compatibility remains unchecked.

## Checks

```sh
sh scripts/check.sh
```

The script checks repository paths, formatting, workspace compilation, tests, and strict Clippy. Local execution defaults to offline mode.

Format modified Rust source before running checks.

```sh
cargo fmt --all
```

Offline checks require cached dependencies and the pinned toolchain. Acquisition requires network access to the selected public provider.
GitHub Actions runs the same script with network access enabled.

## Data acquisition

```sh
cargo run -p arus-cli -- catalog check
cargo run -p arus-cli -- data plan
cargo run -p arus-cli -- data refresh
cargo run -p arus-cli -- data status
```

The [source catalog](catalog/README.md) records access, licences, collection recipes, and unresolved data gaps.
The [acquisition guide](docs/data-acquisition.md) covers historical collection, refresh scheduling, receipts, and recovery.

- Default enabled collections archive official categorical weather forecasts and warnings.
- Numerical rainfall recipes require an explicit noncommercial-use declaration where provider terms require it.
- Raw responses and acquisition state stay under ignored `data/acquisition/`.
- Successful collection does not establish complete coverage or scientific suitability.

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
- Check site-specific historical coverage and independent flood observations.
- Resolve product dependencies and check browser and numerical builds separately.
- Check NodeDB Lite durability and query capabilities before implementing repositories.
