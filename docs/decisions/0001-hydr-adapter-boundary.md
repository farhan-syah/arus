# ADR 0001 — `hydr` Adapter Boundary & Screening Qualification Gate

## Status
Accepted

## Decision
Keep all `hydr` and `numr` types encapsulated inside `crates/hydraulics` and `crates/hydrology`. Shared domain contracts (`arus-domain`, `arus-api-types`, `apps/web`) never import solver types.

Every uncalibrated simulation carries `RunClassification::NumericalDemonstration` (`DEMO • SCREENING ONLY`). Operational forecast publication requires independent calibration (`arus-worker calibrate` on `EvaluationPartition::Calibration`) and blind `LockedTest` evaluation.

## Consequences
- Browser and API transport schemas remain stable across `hydr` major upgrades.
- Worker execution completion alone can never authorise operational publication.
