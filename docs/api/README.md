# ARUS Public API (`arus-api-v1`)

`apps/api` serves immutable archived runs and location outlooks from an `arus-assets::Archive` via Axum.

## Run locally

```sh
cargo run -p arus-worker -- outlook --run-id demo-001 --archive data/runs
cargo run -p arus-api -- --archive data/runs --bind 127.0.0.1:8080
```

## Endpoints

| Method | Path | Response |
| --- | --- | --- |
| `GET` | `/health` | `{ "status": "ok", "schema_version": "arus-api-v1" }` |
| `GET` | `/api/v1/published` | Current `PublishedPointer` or `404` when no operational forecast is published |
| `GET` | `/api/v1/runs` | `RunIndex` (`latest_run_id`, `run_ids`) |
| `GET` | `/api/v1/runs/{run_id}` | `RunRecord` (`provenance`, `conservation_passed`, blob refs) or `404` |
| `GET` | `/api/v1/runs/{run_id}/outlook` | `Vec<ApiLocationOutlook>` or `404` |
| `GET` | `/runs/{run_id}?lang=en\|bm` | Accessible HTML outlook page rendered by `arus-web` |

Every `ApiLocationOutlook` carries explicit `classification` (`numerical-demonstration`, `event-reconstruction`, `forecast-replay`, or `operational-forecast`) and `support` (`supported` or `unsupported`). Unsupported locations never appear as dry zero.
