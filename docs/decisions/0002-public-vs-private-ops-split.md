# ADR 0002 — Public `arus` Core vs Private VM Operations Repository

## Status
Accepted

## Decision
Keep `arus` 100% region-agnostic and public. Move regional data-ingestion schedules, provider credentials, country-specific station registries, and production `arus.hosted.json` configs into a separate private VM ops repository.

`arus-api` connects to the private pipeline through two generic CLI flags:
- `--config <path>` (`ServerConfig`, exposed at `GET /api/v1/config`, enforcing `403` on disabled features).
- `--context-dir <path>` (`RegionalContext` JSON files, exposed at `GET /api/v1/context` with `wgs84_bounds` checks and staleness reporting).

## Consequences
- Self-hosted power users run `arus-api` locally with all features enabled by default.
- Public hosted deployments gate expensive endpoints (`jobs`, `mp4`, `byod`) server-side without forking `arus`.
