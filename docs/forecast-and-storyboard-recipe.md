# ARUS Forecast & Storyboard Operator Recipe

This recipe covers data collection, the 4-stage forecast pipeline, deep-linked Studio playback, and pure-Rust H.264 storyboard recording.

## 1. Prepare a Basin Collection File

Create a collection file (such as `basin-collections.json`) for multi-model rainfall, soil moisture, coastal tide, GloFAS discharge, and official warnings.

Allowed Open-Meteo parameters enforced by `arus-ingest`:

| Feed | Allowed Parameters |
| --- | --- |
| `models` | `gfs_global`, `ecmwf_ifs025`, `icon_seamless`, `gem_global`, `jma_seamless` |
| Weather `hourly` | `precipitation` (required), `soil_moisture_0_to_7cm`, `soil_moisture_0_to_1cm` |
| Marine `hourly` | `wave_height`, `sea_level_height_msl`, `ocean_current_velocity` |
| GloFAS `daily` | `river_discharge`, `river_discharge_mean`, `river_discharge_median`, `river_discharge_max`, `river_discharge_min`, `river_discharge_p25`, `river_discharge_p75` |

```json
{
  "schema_version": 1,
  "collections": [
    {
      "id": "malaysia-weather-warnings",
      "source_id": "met-weather-warning",
      "description": "Official METMalaysia weather warnings.",
      "enabled": true,
      "accept_noncommercial": false,
      "parameters": {
        "product": "warning",
        "limit": 100
      }
    },
    {
      "id": "setiu-multimodel-5d",
      "source_id": "openmeteo-gfs-forecast",
      "description": "Multi-model hourly rain and soil moisture at Jambatan Permaisuri, Setiu.",
      "enabled": true,
      "accept_noncommercial": false,
      "parameters": {
        "latitude": 5.4853,
        "longitude": 102.6861,
        "models": ["ecmwf_ifs025", "gfs_global", "icon_seamless", "jma_seamless"],
        "hourly": ["precipitation", "soil_moisture_0_to_7cm"],
        "past_days": 2,
        "forecast_days": 5
      }
    },
    {
      "id": "kuala-setiu-marine-5d",
      "source_id": "openmeteo-marine-forecast",
      "description": "Hourly sea-level height MSL and wave height at Kuala Setiu outlet.",
      "enabled": true,
      "accept_noncommercial": false,
      "parameters": {
        "latitude": 5.675,
        "longitude": 102.715,
        "hourly": ["sea_level_height_msl", "wave_height"],
        "past_days": 2,
        "forecast_days": 5
      }
    },
    {
      "id": "setiu-glofas-7d",
      "source_id": "openmeteo-glofas-flood",
      "description": "Daily GloFAS ensemble discharge benchmark for Sungai Setiu.",
      "enabled": true,
      "accept_noncommercial": false,
      "parameters": {
        "latitude": 5.4853,
        "longitude": 102.6861,
        "daily": ["river_discharge", "river_discharge_median", "river_discharge_max"],
        "past_days": 5,
        "forecast_days": 7
      }
    }
  ]
}
```

> [!NOTE]
> Open-Meteo's free tier is noncommercial only. For commercial deployments, use a paid Open-Meteo plan or commercial-safe sources in `catalog/sources.json` (`ecmwf-open-data`, `noaa-gefs-aws`, `dwd-icon-open-data`).

## 2. Check and Sync Data with `arus-cli`

```sh
cargo run --release -p arus-cli -- \
  --collections basin-collections.json \
  catalog check

cargo run --release -p arus-cli -- \
  --collections basin-collections.json \
  data plan --accept-noncommercial

cargo run --release -p arus-cli -- \
  --collections basin-collections.json \
  data sync --accept-noncommercial

cargo run --release -p arus-cli -- \
  --collections basin-collections.json \
  data status
```

Each receipt in `data/acquisition/receipts/<collection_id>/` stores the `sha256` hash of the raw JSON blob in `data/acquisition/blobs/`.

## 3. Define a Local Station Registry (`stations.json`)

Provide river-gauge thresholds (`normal_m <= alert_m <= warning_m <= danger_m`) in a JSON registry to compute headroom and status (`normal`, `alert`, `warning`, `danger`).

```json
{
  "schema_version": "arus-stations-v1",
  "stations": [
    {
      "station_id": "5527401",
      "name": "Sg. Setiu di Jambatan Permaisuri",
      "river": "Sungai Setiu",
      "latitude": 5.48528,
      "longitude": 102.68611,
      "normal_m": 5.5,
      "alert_m": 7.6,
      "warning_m": 8.3,
      "danger_m": 8.8
    }
  ]
}
```

## 4. Run the Forecast Bridge or 4-Stage Pipeline

### Option A: 1D hydrology bridge and ensemble agreement (`forecast-bridge`)

```sh
cargo run --release -p arus-worker -- forecast-bridge \
  --blob data/acquisition/blobs/<setiu-multimodel-sha256> \
  --basin-id sg-setiu \
  --area-km2 876.89 \
  --impervious-fraction 0.10 \
  --past-hours 48 \
  --window-hours 6
```

### Option B: 4-stage pipeline (`pipeline`)

Runs initial soil/gauge state, 72–120h 1D hydrology ensemble, Stage 3 coastal tide-lock, and Stage 4 windowed 2D `hydr 0.2.0` inundation.

```sh
cargo run --release -p arus-worker -- pipeline \
  --meteo data/acquisition/blobs/<setiu-multimodel-sha256> \
  --marine data/acquisition/blobs/<kuala-setiu-marine-sha256> \
  --stations stations.json \
  --station-id 5527401 \
  --current-stage-m 6.41 \
  --basin-id sg-setiu \
  --area-km2 876.89 \
  --impervious-fraction 0.10 \
  --past-hours 48 \
  --window-hours 6 \
  --tide-threshold-m 1.8 \
  --trigger-rain-mm 25.0 \
  --wind-m-per-s 0.0
```

Uncalibrated runs emit `screening-only` classification and report multi-model spread and agreement.

## 5. Launch Studio with Deep-Link URL Params

```sh
cargo run --release -p arus-api -- --archive data/runs --bind 127.0.0.1:8080
```

Open a pre-framed scenario with query parameters:

```text
http://127.0.0.1:8080/?lat=5.4853&lon=102.6861&zoom=14.0&pitch=48&bearing=-16&basemap=satellite&presentation=1&fixed=1&rain=45&dur=7200&soil=wet&start_iso=2026-11-25T18:00:00Z&tz=MYT&autoplay=1
```

| Parameter | Purpose |
| --- | --- |
| `lat`, `lon`, `zoom`, `pitch`, `bearing` | Initial camera target and angle |
| `basemap` | `topo`, `satellite`, or `dark` |
| `presentation`, `fixed`, `autoplay` | Hide authoring controls (`1`), lock camera (`1`), and start playback (`1`) |
| `rain`, `dur`, `wind`, `soil`, `hyetograph`, `boundary` | Hydraulic scenario overrides |
| `start_iso`, `tz` | Timeline start timestamp (ISO-8601) and timezone label |

Inside Studio, click **PNG** to save a snapshot or **Record** to capture and transcode `.mp4` via `POST /api/v1/export/mp4`.

## 6. Record a Declarative JSON Storyboard (`arus-worker record`)

Build `arus-api` and `arus-worker`, then run the pure-Rust Chromium CDP recorder (zero Python required):

```sh
cargo build --release -p arus-api -p arus-worker

./target/release/arus-worker record \
  --spec docs/storyboard-example.json \
  --out /tmp/arus-storyboard-out \
  --resolution 720p \
  --fps 15
```

| Preset | Dimensions |
| --- | --- |
| `720p` (default) | `1280 × 720` |
| `1080p` / `1k` | `1920 × 1080` |
| `2k` | `2560 × 1440` |
| `4k` | `3840 × 2160` |

### Output artifacts

`arus-worker record` writes to `--out`:

- `<name>.mp4`: Pure-Rust `vidr 0.1.1` H.264 faststart MP4 video
- `*.png`: Numbered screenshots from `screenshot` steps
- `summary.json`: Step timings, output paths, and `window.arusDirector.getState()` snapshots

### Storyboard JSON actions

Set `"init": { ... }` at the top level to apply startup parameters before the first frame. Set `"presentation": true` and `"fixed": true` for locked-camera broadcasts.

| Action | Key fields | Effect |
| --- | --- | --- |
| `jump_to` | `lat`, `lon`, `zoom`, `pitch`, `bearing`, `wait_s` | Instant camera cut (`0 ms`) |
| `fly_to` | `lat`, `lon`, `zoom`, `pitch`, `bearing`, `duration_ms`, `wait_s` | Smooth camera flight |
| `set_basemap` | `mode` (`topo` / `satellite` / `dark`), `wait_s` | Switch basemap style |
| `set_boundary` | `query`, `wait_s` | Load OSM Nominatim polygon mask via `loadBoundaryByQuery` |
| `set_scenario` | `rainfall_mm_per_hr`, `duration_s`, `antecedent_rain_mm`, `spatial_rain_grid`, `hourly_rain_mm`, `hourly_tide_m`, `observed_gauges`, `start_iso`, `tz_label` | Update full-physics hydraulic inputs |
| `fetch_nwp` | `wait_s` | Fetch live multi-model NWP panel data |
| `run_simulation` | `wait_s` | Execute interactive 2D `hydr 0.2.0` simulation |
| `set_layer` | `layer`, `wait_s` | Switch depth, hazard, or velocity overlay |
| `set_frame` | `frame`, `wait_s` | Seek playback to a specific frame index |
| `play` | `seconds` | Play flood animation for `N` seconds |
| `cross_section` | `a: [lat, lon]`, `b: [lat, lon]`, `wait_s` | Plot terrain vs water-surface profile `A → B` |
| `clear_inspector` | — | Close profile or hydrograph inspector |
| `add_intervention` | `kind` (`bund` / `pond`), `points: [[lat, lon], ...]`, `wait_s` | Paint a floodwall or detention pond |
| `clear_interventions` | — | Remove painted interventions |
| `compare` | `wait_s` | Run Baseline vs Intervention `Δh` diff |
| `set_outlook` | `title`, `badge`, `summary`, `hotspots`, `footnote` | Show or update the live per-frame Public Flood Outlook card |
| `caption` | `text`, `wait_s` | Set or clear the top banner |
| `screenshot` | `file` | Save a `.png` screenshot into `--out` |
| `wait` | `seconds` | Hold current frame for `N` seconds |

### Standalone `.webm` to `.mp4` transcoding

```sh
cargo run --release -p arus-worker -- transcode \
  --input /path/to/capture.webm \
  --output /path/to/capture.mp4 \
  --trim-start-s 0.0
```
