//! Acceptance tests TC-01 through TC-06 driven by general-purpose ARUS APIs
//! using the empirical 2026-11-24 Setiu multi-model snapshot as a realistic test fixture.

use std::process::Command;
use std::time::Instant;

use arus_domain::{GridSpec, RunClassification, SpatialReference};
use arus_forecasting::{
    AgreementLevel, DailyShadowForecast, DailyShadowObservation, StationError, StationRegistry,
    StationStatus, StationThreshold, assess_station, score_shadow_forecast,
};
use arus_geo::TerrainGrid;
use arus_hydraulics::{
    BoundaryAxisSpec, BoundaryFaceSpec, BoundaryForcingSpec, LandSpec, RainIntervalSpec,
    SurfaceRunSpec,
};
use arus_ingest::{
    Catalog, Collections, parse_open_meteo_flood, parse_open_meteo_hourly, parse_open_meteo_marine,
    plan,
};
use arus_jobs::StepBudget;
use tempfile::TempDir;

/// Builds a synthetic 7-day (48h past + 120h forecast = 168h) Open-Meteo multi-model JSON blob
/// matching the empirical 2026-11-24 Setiu divergence (ECMWF wet antecedent + 0.506 soil moisture,
/// GFS null soil moisture + 98.4 mm Nov 29 spike, ICON moderate).
fn build_setiu_multimodel_blob() -> Vec<u8> {
    let mut times = Vec::with_capacity(168);
    let mut ecmwf_rain = Vec::with_capacity(168);
    let mut gfs_rain = Vec::with_capacity(168);
    let mut icon_rain = Vec::with_capacity(168);
    let mut ecmwf_soil: Vec<serde_json::Value> = Vec::with_capacity(168);
    let mut gfs_soil: Vec<serde_json::Value> = Vec::with_capacity(168);
    let mut icon_soil: Vec<serde_json::Value> = Vec::with_capacity(168);

    // Daily totals (mm): [Nov23, Nov24, Nov25, Nov26, Nov27, Nov28, Nov29]
    let ecmwf_days = [37.2, 33.0, 20.1, 16.2, 9.9, 13.5, 12.9];
    let gfs_days = [6.5, 5.1, 6.3, 18.9, 4.1, 18.2, 98.4];
    let icon_days = [7.7, 5.5, 8.6, 2.4, 7.2, 11.1, 18.0];

    for day in 0..7_usize {
        let day_num = 23 + day;
        for h in 0..24_usize {
            times.push(format!("2026-11-{day_num:02}T{h:02}:00"));
            // Put GFS Nov 29 peak into hours 12..18 so 6h window triggers cleanly
            if day == 6 && (12..18).contains(&h) {
                gfs_rain.push(serde_json::json!(gfs_days[day] / 6.0));
            } else if day == 6 {
                gfs_rain.push(serde_json::json!(0.0));
            } else {
                gfs_rain.push(serde_json::json!(gfs_days[day] / 24.0));
            }
            ecmwf_rain.push(serde_json::json!(ecmwf_days[day] / 24.0));
            icon_rain.push(serde_json::json!(icon_days[day] / 24.0));

            ecmwf_soil.push(serde_json::json!(0.506));
            // GFS soil moisture is missing (null) — must NEVER be coerced to 0.0
            gfs_soil.push(serde_json::Value::Null);
            icon_soil.push(serde_json::json!(0.34));
        }
    }

    serde_json::to_vec(&serde_json::json!({
        "latitude": 5.4853,
        "longitude": 102.6861,
        "utc_offset_seconds": 0,
        "timezone": "GMT",
        "hourly_units": {
            "time": "iso8601",
            "precipitation_ecmwf_ifs025": "mm",
            "precipitation_gfs_global": "mm",
            "precipitation_icon_seamless": "mm",
            "soil_moisture_0_to_7cm_ecmwf_ifs025": "m³/m³",
            "soil_moisture_0_to_7cm_gfs_global": "m³/m³",
            "soil_moisture_0_to_7cm_icon_seamless": "m³/m³"
        },
        "hourly": {
            "time": times,
            "precipitation_ecmwf_ifs025": ecmwf_rain,
            "precipitation_gfs_global": gfs_rain,
            "precipitation_icon_seamless": icon_rain,
            "soil_moisture_0_to_7cm_ecmwf_ifs025": ecmwf_soil,
            "soil_moisture_0_to_7cm_gfs_global": gfs_soil,
            "soil_moisture_0_to_7cm_icon_seamless": icon_soil
        }
    }))
    .unwrap_or_else(|e| unreachable!("serialize fixture: {e}"))
}

#[test]
fn tc01_multimodel_open_meteo_ingest_plan_and_null_preservation() {
    let raw_catalog = include_str!("../../../catalog/sources.json");
    let catalog: Catalog =
        serde_json::from_str(raw_catalog).unwrap_or_else(|e| unreachable!("catalog: {e}"));

    let collections: Collections = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "collections": [{
            "id": "basin-multimodel-5d",
            "source_id": "openmeteo-gfs-forecast",
            "description": "Multi-model 5-day forecast with antecedent soil moisture",
            "enabled": true,
            "accept_noncommercial": true,
            "parameters": {
                "latitude": 5.4853,
                "longitude": 102.6861,
                "models": ["ecmwf_ifs025", "gfs_global", "icon_seamless"],
                "hourly": ["precipitation", "soil_moisture_0_to_7cm"],
                "past_days": 2,
                "forecast_days": 5
            }
        }]
    }))
    .unwrap_or_else(|e| unreachable!("collections: {e}"));

    let temp = TempDir::new().unwrap_or_else(|e| unreachable!("tempdir: {e}"));
    let archive =
        arus_assets::Archive::open(temp.path()).unwrap_or_else(|e| unreachable!("archive: {e}"));
    let planned = plan(
        &catalog,
        &collections,
        &archive,
        time::OffsetDateTime::UNIX_EPOCH,
    )
    .unwrap_or_else(|e| unreachable!("plan: {e}"));
    assert_eq!(planned.len(), 1);
    let url = planned[0].request_url.as_deref().unwrap_or("");
    assert!(url.contains("ecmwf_ifs025"));
    assert!(url.contains("gfs_global"));
    assert!(url.contains("icon_seamless"));
    assert!(url.contains("past_days=2"));
    assert!(url.contains("soil_moisture_0_to_7cm"));

    let parsed = parse_open_meteo_hourly(&build_setiu_multimodel_blob())
        .unwrap_or_else(|e| unreachable!("parse blob: {e}"));
    assert_eq!(parsed.series.len(), 3);
    let gfs = parsed
        .series
        .iter()
        .find(|s| s.model == "gfs_global")
        .unwrap_or_else(|| unreachable!("gfs series"));
    assert!(
        gfs.soil_moisture_0_to_7cm.iter().all(|v| v.is_none()),
        "GFS missing soil moisture must remain None, never 0.0"
    );
    let ecmwf = parsed
        .series
        .iter()
        .find(|s| s.model == "ecmwf_ifs025")
        .unwrap_or_else(|| unreachable!("ecmwf series"));
    assert_eq!(ecmwf.soil_moisture_0_to_7cm[0], Some(0.506));
}

#[test]
fn tc02_and_tc03_forecast_bridge_runs_120h_in_under_2s_and_flags_low_agreement() {
    let temp = TempDir::new().unwrap_or_else(|e| unreachable!("tempdir: {e}"));
    let blob_path = temp.path().join("multimodel.json");
    std::fs::write(&blob_path, build_setiu_multimodel_blob())
        .unwrap_or_else(|e| unreachable!("write blob: {e}"));

    let start = Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_arus-worker"))
        .args([
            "forecast-bridge",
            "--blob",
            blob_path.to_str().unwrap_or(""),
            "--basin-id",
            "setiu-permaisuri",
            "--area-km2",
            "40.0",
            "--past-hours",
            "48",
            "--window-hours",
            "6",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("run worker: {e}"));
    let elapsed = start.elapsed();

    assert!(
        output.status.success(),
        "forecast-bridge failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let max_wall_s = if cfg!(debug_assertions) { 20.0 } else { 2.0 };
    assert!(
        elapsed.as_secs_f64() < max_wall_s,
        "120h 3-model hydrology bridge took {:.3}s (limit {max_wall_s:.1}s)",
        elapsed.as_secs_f64()
    );

    let v: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|e| unreachable!("json: {e}"));
    assert_eq!(v["member_count"], 3);
    assert_eq!(v["agreement"], "low");
    assert_eq!(v["worst_case_model"], "gfs_global");
    assert!(v["max_peak_24h_mm"].as_f64().unwrap_or(0.0) > 95.0);
    assert!(v["min_peak_24h_mm"].as_f64().unwrap_or(999.0) < 25.0);
}

#[test]
fn tc04_dry_start_6h_peak_window_advances_with_free_outfall_and_conserves_mass() {
    let terrain = TerrainGrid::new(
        GridSpec {
            columns: 6,
            rows: 3,
            dx_m: 100.0,
            dy_m: 100.0,
        },
        SpatialReference {
            projected_crs: "EPSG:3375".to_owned(),
            vertical_datum: "local-datum".to_owned(),
            origin_m: [0.0, 0.0],
        },
        "synthetic-sloped-valley",
        (0..18)
            .map(|i| Some(10.0 - ((i % 6) as f64) * 0.4))
            .collect(),
    )
    .unwrap_or_else(|e| unreachable!("terrain: {e}"));

    let open_faces: Vec<BoundaryFaceSpec> = (0..3)
        .map(|row| BoundaryFaceSpec {
            axis: BoundaryAxisSpec::X,
            column: 6,
            row,
        })
        .collect();
    let boundaries: Vec<BoundaryForcingSpec> = open_faces
        .iter()
        .copied()
        .map(|face| BoundaryForcingSpec::ExteriorDry { face })
        .collect();

    let spec = SurfaceRunSpec {
        run_id: "dry-start-6h-peak".to_owned(),
        classification: RunClassification::NumericalDemonstration,
        terrain,
        uniform_manning_n: 0.035,
        land: LandSpec::UniformImpervious,
        initial_depth_m: 0.0,
        open_faces,
        forcing: vec![RainIntervalSpec {
            start_s: 0.0,
            end_s: 21_600.0, // 6-hour peak window
            uniform_rain_m_per_s: 20.8 / 3_600_000.0,
            boundaries,
        }],
        target_time_s: 21_600.0,
        max_steps: 25_000,
        probes: Vec::new(),
        threshold_m: 0.15,
        hysteresis_m: 0.02,
        recession_persistence_s: 60.0,
        coastal: None,
    };

    let mut engine = spec
        .build_engine()
        .unwrap_or_else(|e| unreachable!("build dry-start engine: {e}"));
    let advance = engine
        .advance_to(21_600.0, StepBudget::new(25_000))
        .unwrap_or_else(|e| unreachable!("advance 6h dry-start: {e}"));
    assert!((advance.accepted_time_s - 21_600.0).abs() < 1e-6);
    assert!(engine.water_budget().within_residual_bound());
}

#[test]
fn tc05_generic_station_registry_maps_thresholds_and_tags_uncalibrated_screening_only() {
    let registry = StationRegistry {
        schema_version: "arus-stations-v1".to_owned(),
        stations: vec![
            StationThreshold {
                station_id: "5527401".to_owned(),
                name: "Sg. Setiu di Jambatan Permaisuri".to_owned(),
                river: "Sg. Setiu".to_owned(),
                latitude: 5.48528,
                longitude: 102.68611,
                normal_m: 5.50,
                alert_m: 7.60,
                warning_m: 8.30,
                danger_m: 8.80,
            },
            StationThreshold {
                station_id: "5428440".to_owned(),
                name: "Sg. Nerus di Kg. Langkap".to_owned(),
                river: "Sg. Nerus".to_owned(),
                latitude: 5.387,
                longitude: 102.815,
                normal_m: 17.00,
                alert_m: 19.50,
                warning_m: 20.00,
                danger_m: 21.00,
            },
        ],
    };
    registry
        .validate()
        .unwrap_or_else(|e| unreachable!("validate registry: {e}"));

    let permaisuri = registry
        .find("5527401")
        .unwrap_or_else(|| unreachable!("find 5527401"));
    let assessment = assess_station(permaisuri, Some(8.55), false);
    assert_eq!(assessment.status, StationStatus::Warning);
    assert_eq!(assessment.label, "screening-only");
    assert_eq!(
        assessment.classification,
        RunClassification::NumericalDemonstration
    );

    let langkap = registry
        .find("5428440")
        .unwrap_or_else(|| unreachable!("find 5428440"));
    assert_eq!(langkap.classify_stage(Some(21.2)), StationStatus::Danger);
    assert_eq!(langkap.classify_stage(None), StationStatus::Unknown);
}

#[test]
fn tc06_shadow_forecast_scoring_rejects_hindsight_leakage() {
    let issued_at = "2026-11-24";
    let forecasts = vec![
        DailyShadowForecast {
            date: "2026-11-25".to_owned(),
            forecast_rain_mm: 20.1,
            forecast_status: StationStatus::Normal,
        },
        DailyShadowForecast {
            date: "2026-11-29".to_owned(),
            forecast_rain_mm: 98.4,
            forecast_status: StationStatus::Warning,
        },
    ];
    let leaked_obs = vec![DailyShadowObservation {
        date: "2026-11-24".to_owned(),
        observed_rain_mm: 33.0,
        observed_status: StationStatus::Normal,
    }];
    let err = score_shadow_forecast(issued_at, &forecasts, &leaked_obs);
    assert!(matches!(err, Err(StationError::HindsightLeakage { .. })));

    let valid_obs = vec![
        DailyShadowObservation {
            date: "2026-11-25".to_owned(),
            observed_rain_mm: 18.0,
            observed_status: StationStatus::Normal,
        },
        DailyShadowObservation {
            date: "2026-11-29".to_owned(),
            observed_rain_mm: 40.0,
            observed_status: StationStatus::Alert,
        },
    ];
    let report = score_shadow_forecast(issued_at, &forecasts, &valid_obs)
        .unwrap_or_else(|e| unreachable!("score shadow: {e}"));
    assert_eq!(report.scored_days, 2);
    assert_eq!(report.status_exact_matches, 1);
    assert!(report.peak_day_match);
    assert!(report.rain_mae_mm > 25.0);
    // Keep AgreementLevel imported/used cleanly
    assert_ne!(AgreementLevel::Low, AgreementLevel::High);
}

#[test]
fn marine_glofas_and_commercial_nwp_catalog_entries_plan_and_parse() {
    let raw_catalog = include_str!("../../../catalog/sources.json");
    let catalog: Catalog =
        serde_json::from_str(raw_catalog).unwrap_or_else(|e| unreachable!("catalog: {e}"));

    // Verify commercial-safe direct NWP entries exist and are marked commercial-safe
    for id in ["ecmwf-open-data", "noaa-gefs-aws", "dwd-icon-open-data"] {
        let src = catalog
            .sources
            .iter()
            .find(|s| s.id == id)
            .unwrap_or_else(|| unreachable!("missing catalog source `{id}`"));
        assert!(!src.license.noncommercial_only);
    }

    let collections: Collections = serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "collections": [
            {
                "id": "coastal-marine-5d",
                "source_id": "openmeteo-marine-forecast",
                "description": "Coastal wave height and MSL surge screening",
                "enabled": true,
                "accept_noncommercial": true,
                "parameters": {
                    "latitude": 5.62,
                    "longitude": 102.75,
                    "hourly": ["wave_height", "sea_level_height_msl"],
                    "forecast_days": 5,
                    "past_days": 1
                }
            },
            {
                "id": "basin-glofas-14d",
                "source_id": "openmeteo-glofas-flood",
                "description": "GloFAS ensemble river discharge screening",
                "enabled": true,
                "accept_noncommercial": true,
                "parameters": {
                    "latitude": 5.4853,
                    "longitude": 102.6861,
                    "daily": ["river_discharge", "river_discharge_max"],
                    "forecast_days": 14,
                    "past_days": 2
                }
            }
        ]
    }))
    .unwrap_or_else(|e| unreachable!("collections: {e}"));

    let temp = TempDir::new().unwrap_or_else(|e| unreachable!("tempdir: {e}"));
    let archive =
        arus_assets::Archive::open(temp.path()).unwrap_or_else(|e| unreachable!("archive: {e}"));
    let planned = plan(
        &catalog,
        &collections,
        &archive,
        time::OffsetDateTime::UNIX_EPOCH,
    )
    .unwrap_or_else(|e| unreachable!("plan marine+flood: {e}"));
    assert_eq!(planned.len(), 2);
    assert!(
        planned[0]
            .request_url
            .as_deref()
            .unwrap_or("")
            .contains("marine-api.open-meteo.com")
    );
    assert!(
        planned[1]
            .request_url
            .as_deref()
            .unwrap_or("")
            .contains("flood-api.open-meteo.com")
    );

    let marine_blob = serde_json::to_vec(&serde_json::json!({
        "latitude": 5.62,
        "longitude": 102.75,
        "hourly": {
            "time": ["2026-11-24T00:00", "2026-11-24T01:00"],
            "wave_height": [1.4, null],
            "sea_level_height_msl": [0.35, 0.42]
        }
    }))
    .unwrap_or_else(|e| unreachable!("marine json: {e}"));
    let marine =
        parse_open_meteo_marine(&marine_blob).unwrap_or_else(|e| unreachable!("parse marine: {e}"));
    assert_eq!(marine.wave_height_m, vec![Some(1.4), None]);
    assert_eq!(marine.sea_level_height_msl_m, vec![Some(0.35), Some(0.42)]);

    let flood_blob = serde_json::to_vec(&serde_json::json!({
        "latitude": 5.4853,
        "longitude": 102.6861,
        "daily": {
            "time": ["2026-11-25", "2026-11-26"],
            "river_discharge": [42.5, 68.0],
            "river_discharge_max": [null, 112.0]
        }
    }))
    .unwrap_or_else(|e| unreachable!("flood json: {e}"));
    let flood =
        parse_open_meteo_flood(&flood_blob).unwrap_or_else(|e| unreachable!("parse flood: {e}"));
    assert_eq!(flood.river_discharge_m3_per_s, vec![Some(42.5), Some(68.0)]);
    assert_eq!(flood.river_discharge_max_m3_per_s, vec![None, Some(112.0)]);
}

#[test]
fn four_stage_pipeline_chains_headroom_120h_hydrology_tide_lock_and_windowed_2d() {
    let temp = TempDir::new().unwrap_or_else(|e| unreachable!("tempdir: {e}"));
    let meteo_path = temp.path().join("meteo.json");
    let marine_path = temp.path().join("marine.json");
    let stations_path = temp.path().join("stations.json");

    std::fs::write(&meteo_path, build_setiu_multimodel_blob())
        .unwrap_or_else(|e| unreachable!("write meteo: {e}"));

    // 168h marine series with +2.12m spring tide overlapping the GFS peak window
    let mut m_times = Vec::with_capacity(168);
    let mut m_msl = Vec::with_capacity(168);
    for day in 0..7_usize {
        for h in 0..24_usize {
            m_times.push(format!("2026-11-{:02}T{h:02}:00", 23 + day));
            m_msl.push(if day == 6 && (12..18).contains(&h) {
                2.12
            } else {
                0.85
            });
        }
    }
    std::fs::write(
        &marine_path,
        serde_json::to_vec(&serde_json::json!({
            "latitude": 5.675,
            "longitude": 102.715,
            "hourly": { "time": m_times, "sea_level_height_msl": m_msl }
        }))
        .unwrap_or_else(|e| unreachable!("marine json: {e}")),
    )
    .unwrap_or_else(|e| unreachable!("write marine: {e}"));

    std::fs::write(
        &stations_path,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": "arus-stations-v1",
            "stations": [{
                "station_id": "5428440",
                "name": "Sg. Nerus di Kg. Langkap",
                "river": "Sg. Nerus",
                "latitude": 5.387,
                "longitude": 102.815,
                "normal_m": 17.0,
                "alert_m": 19.5,
                "warning_m": 20.0,
                "danger_m": 21.0
            }]
        }))
        .unwrap_or_else(|e| unreachable!("stations json: {e}")),
    )
    .unwrap_or_else(|e| unreachable!("write stations: {e}"));

    let output = Command::new(env!("CARGO_BIN_EXE_arus-worker"))
        .args([
            "pipeline",
            "--meteo",
            meteo_path.to_str().unwrap_or(""),
            "--marine",
            marine_path.to_str().unwrap_or(""),
            "--stations",
            stations_path.to_str().unwrap_or(""),
            "--station-id",
            "5428440",
            "--current-stage-m",
            "19.42",
            "--basin-id",
            "sg-nerus-langkap",
            "--area-km2",
            "40.0",
            "--past-hours",
            "48",
            "--window-hours",
            "3",
            "--tide-threshold-m",
            "1.8",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("run pipeline: {e}"));

    assert!(
        output.status.success(),
        "pipeline failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let v: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|e| unreachable!("json: {e}"));
    assert_eq!(v["label"], "screening-only");
    assert_eq!(v["stage1_initial_state"]["initial_stage_source"], "gauge");
    assert!(
        (v["stage1_initial_state"]["headroom_to_alert_m"]
            .as_f64()
            .unwrap_or(99.0)
            - 0.08)
            .abs()
            < 1e-6
    );
    assert_eq!(v["stage2_ensemble"]["agreement"], "low");
    assert_eq!(v["stage3_compound_tide"]["compound_backwater_risk"], true);
    assert_eq!(v["stage4_windowed_2d"]["triggered"], true);
    assert_eq!(v["stage4_windowed_2d"]["mass_valid"], true);
}
