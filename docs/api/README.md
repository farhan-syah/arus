# ARUS Public API (`arus-api-v1`)

`apps/api` serves the 3D Studio, archived runs, interactive `hydr 0.2.0` simulations, GIS exports, and `vidr 0.1.1` MP4 encoding via Axum.

## Run locally

```sh
cargo run --release -p arus-worker -- outlook --run-id demo-001 --archive data/runs
cargo run --release -p arus-api -- --archive data/runs --bind 127.0.0.1:8080
```

## Endpoints

| Method | Path | Response |
| --- | --- | --- |
| `GET` | `/` | 3D Flood Simulation Studio (`arus-web` + vendored MapLibre GL JS v5) |
| `GET` | `/archive?lang=en\|bm` | Dioxus SSR archive index listing archived runs and published status |
| `GET` | `/sources?lang=en\|bm` | Public source catalog page |
| `GET` | `/runs/{run_id}?lang=en\|bm` | Dioxus SSR location outlook page rendered by `arus-web` |
| `GET` | `/health` | `{ "status": "ok", "schema_version": "arus-api-v1" }` |
| `GET` | `/api/v1/catalog/sources` | Public `Catalog` from `catalog/sources.json` or `404` when unconfigured |
| `GET` | `/api/v1/published` | Current `PublishedPointer` or `404` when no operational forecast is published |
| `GET` | `/api/v1/runs` | `RunIndex` (`latest_run_id`, `run_ids`) |
| `GET` | `/api/v1/runs/{run_id}` | `RunRecord` (`provenance`, `conservation_passed`, blob refs) or `404` |
| `GET` | `/api/v1/runs/{run_id}/outlook` | `Vec<ApiLocationOutlook>` or `404` |
| `GET` | `/api/v1/runs/{run_id}/grid` | `RunGridResponse` with grid metadata, water budget, and peak depth envelope |
| `GET` | `/api/v1/runs/{run_id}/frames.bin` | Compact binary frame series (`h`, `vx`, `vy`) for 60 FPS WebGL playback |
| `GET` | `/api/v1/runs/{run_id}/export/max-depth.asc` | Projected ESRI ASCII `.asc` maximum flood depth grid |
| `GET` | `/api/v1/runs/{run_id}/export/outlooks.csv` | CSV flood envelope and location outlook summary |
| `POST` | `/api/v1/simulations` | Executes a bounded interactive 2D `hydr 0.2.0` simulation (`<= 16,384` cells, `20s` timeout) |
| `POST` | `/api/v1/compare` | Runs paired Baseline vs Intervention (`bund` / `pond`) simulations and returns `Δh` plus cut/fill volumes |
| `POST` | `/api/v1/jobs` | Enqueues an async high-resolution basin job (`<= 262,144` cells, `<= 12h` duration, `300s` timeout) |
| `GET` | `/api/v1/jobs` | Lists tracked async basin jobs with live status and progress |
| `GET` | `/api/v1/jobs/{job_id}` | Returns live progress (`progress_pct`, `steps_taken`, `accepted_time_s`) and completed `run_id` |
| `POST` | `/api/v1/jobs/{job_id}/cancel` | Requests cooperative cancellation at the next accepted solver step |
| `POST` | `/api/v1/export/mp4` | Transcodes an uploaded `.webm` capture (`<= 64 MiB`) to H.264 faststart `.mp4` via `vidr 0.1.1` |

## Interactive Simulation Inputs (`POST /api/v1/simulations`)

| Field | Type | Purpose |
| --- | --- | --- |
| `center_lat`, `center_lon`, `cols`, `rows`, `dx_m` | numbers | Domain center and Cartesian grid geometry |
| `terrain_elevations_m` / `ascii_dem` | array / object | Sampled Terrarium 30m DEM or uploaded ESRI ASCII `.asc` |
| `spatial_rain_grid` | object | `nx × ny` hourly rain (`hourly_mm`), hourly FAO-56 `et0_mm`, and initial `soil_moisture_init` |
| `hourly_rain_mm`, `hourly_tide_m` | arrays | Single-series hourly rain (`mm/hr`) and coastal sea level (`m MSL`) |
| `antecedent_rain_mm`, `observed_gauges` | number / array | Pre-state valley wetting and `t = 0` observed river-gauge depth assimilation |
| `interventions` | array | Cell elevation edits (`+delta_m` floodwall, `-delta_m` detention pond) |
| `precision` | `f64` \| `f32` | Shallow-water solver precision profile |

Every `ApiLocationOutlook` carries explicit `classification` (`numerical-demonstration`, `event-reconstruction`, `forecast-replay`, or `operational-forecast`) and `support` (`supported` or `unsupported`). Unsupported locations never appear as dry zero.
