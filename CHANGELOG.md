# Changelog

All notable changes to ARUS are documented in this file.

## [0.1.0] - 2026-11-26

### Added
- Initial public release of the 20-crate ARUS flood monitoring and 2D shallow-water simulation workspace.
- Interactive `Live | Simulate` Dioxus + MapLibre Studio (`arus-web`, `arus-api`) with pre-computed 5-day live outlooks, browser IndexedDB saved configs/simulations, 4-level pulsing flood beacons, top-bar alert dropdown, and land-only boundary hover/click switching.
- Lindsay (2016) least-cost depression breaching DEM conditioning and `hydr 0.3.0` 2D shallow-water hydraulic integration (`arus-hydraulics`, `arus-hydrology`, `arus-api`).
- Config-driven deployment (`arus-api --config`) with Open Graph, Twitter Card, JSON-LD, `robots.txt`, `sitemap.xml`, and PWA manifest endpoints.
- Dual-licensed under `MIT OR Apache-2.0`.
