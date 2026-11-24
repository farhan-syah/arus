use arus_api::{ApiState, router};
use arus_api_types::{API_SCHEMA_VERSION, ApiLocationOutlook, ApiLocationStatus};
use arus_assets::Archive;
use arus_domain::{Provenance, RunClassification};
use arus_storage::{RunRecord, RunStore};
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use tempfile::TempDir;
use tower::ServiceExt;

#[tokio::test]
async fn api_serves_health_runs_and_outlooks_from_archive() -> Result<(), String> {
    let temp = TempDir::new().map_err(|e| e.to_string())?;
    let archive = Archive::open(temp.path()).map_err(|e| e.to_string())?;
    let outlooks = vec![ApiLocationOutlook {
        schema_version: API_SCHEMA_VERSION.to_owned(),
        location_id: "basin-center".to_owned(),
        classification: RunClassification::NumericalDemonstration,
        support: ApiLocationStatus::Supported,
        threshold_m: 0.003,
        horizon_start_s: Some(0.0),
        horizon_end_s: Some(60.0),
        max_depth_m: Some(0.006),
        episodes: Vec::new(),
    }];
    let outlook_bytes = serde_json::to_vec(&outlooks).map_err(|e| e.to_string())?;
    let outlook_blob = archive
        .put_blob(&outlook_bytes)
        .map_err(|e| e.to_string())?;
    let store = RunStore::new(&archive);
    store
        .record_run(&RunRecord {
            run_id: "api-run-1".to_owned(),
            provenance: Provenance {
                backend: "hydr".to_owned(),
                contract: "hydr-swe-numr-cpu-2".to_owned(),
                classification: RunClassification::NumericalDemonstration,
                note: "api integration test".to_owned(),
            },
            accepted_time_s: 60.0,
            conservation_passed: true,
            checkpoint_blob: None,
            outlook_blob: Some(outlook_blob),
        })
        .map_err(|e| e.to_string())?;

    let catalog_path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../catalog/sources.json");
    let state = ApiState::open(temp.path())
        .map_err(|e| e.to_string())?
        .with_catalog(catalog_path);
    let app = router(state);

    let catalog_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/catalog/sources")
                .body(Body::empty())
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(catalog_resp.status(), StatusCode::OK);
    let catalog_bytes = to_bytes(catalog_resp.into_body(), 1024 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    let catalog_json: serde_json::Value =
        serde_json::from_slice(&catalog_bytes).map_err(|e| e.to_string())?;
    assert!(
        catalog_json["sources"]
            .as_array()
            .is_some_and(|arr| arr.len() >= 10)
    );

    let health_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(health_resp.status(), StatusCode::OK);

    let runs_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/runs")
                .body(Body::empty())
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(runs_resp.status(), StatusCode::OK);
    let runs_bytes = to_bytes(runs_resp.into_body(), 1024 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    let runs_json: serde_json::Value =
        serde_json::from_slice(&runs_bytes).map_err(|e| e.to_string())?;
    assert_eq!(runs_json["latest_run_id"], "api-run-1");

    let outlook_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/runs/api-run-1/outlook")
                .body(Body::empty())
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(outlook_resp.status(), StatusCode::OK);
    let outlook_body = to_bytes(outlook_resp.into_body(), 1024 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    let decoded: Vec<ApiLocationOutlook> =
        serde_json::from_slice(&outlook_body).map_err(|e| e.to_string())?;
    assert_eq!(decoded, outlooks);

    let studio_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/?lang=en")
                .body(Body::empty())
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(studio_resp.status(), StatusCode::OK);
    let studio_bytes = to_bytes(studio_resp.into_body(), 1024 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    let studio_html = String::from_utf8(studio_bytes.to_vec()).map_err(|e| e.to_string())?;
    assert!(studio_html.contains("id=\"map\""));
    assert!(studio_html.contains("/assets/maplibre-gl.js"));
    assert!(studio_html.contains("/assets/studio.js"));

    let index_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/archive?lang=en")
                .body(Body::empty())
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(index_resp.status(), StatusCode::OK);
    let index_bytes = to_bytes(index_resp.into_body(), 1024 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    let index_html = String::from_utf8(index_bytes.to_vec()).map_err(|e| e.to_string())?;
    assert!(index_html.contains("api-run-1"));
    assert!(index_html.contains("No operational forecast"));

    let sources_html_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/sources?lang=bm")
                .body(Body::empty())
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(sources_html_resp.status(), StatusCode::OK);
    let sources_html_bytes = to_bytes(sources_html_resp.into_body(), 1024 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    let sources_html = String::from_utf8(sources_html_bytes.to_vec()).map_err(|e| e.to_string())?;
    assert!(sources_html.contains("Sumber Data Awam"));
    assert!(sources_html.contains("met-weather-forecast"));

    let html_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/runs/api-run-1?lang=bm")
                .body(Body::empty())
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(html_resp.status(), StatusCode::OK);
    let html_bytes = to_bytes(html_resp.into_body(), 1024 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    let html = String::from_utf8(html_bytes.to_vec()).map_err(|e| e.to_string())?;
    assert!(html.contains("Demonstrasi berangka"));
    assert!(html.contains("basin-center"));

    let missing_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/runs/does-not-exist")
                .body(Body::empty())
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(missing_resp.status(), StatusCode::NOT_FOUND);

    let pub_resp = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/published")
                .body(Body::empty())
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(pub_resp.status(), StatusCode::NOT_FOUND);
    Ok(())
}

#[tokio::test]
async fn interactive_simulation_frames_compare_exports_and_bounds_work() -> Result<(), String> {
    let temp = TempDir::new().map_err(|e| e.to_string())?;
    let state = ApiState::open(temp.path()).map_err(|e| e.to_string())?;
    let app = router(state);

    // 1. Rejects F32 interactive streaming cleanly with 422.
    let bad_f32 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/simulations")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"precision":"f32"}"#))
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(bad_f32.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // 2. Rejects oversized grid (> 16384 cells) cleanly with 422.
    let bad_size = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/simulations")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"cols":200,"rows":200}"#))
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(bad_size.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // 3. Runs a bounded 10x10 F64 peak-storm simulation and fetches grid, binary frames, and exports.
    let sim_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/simulations")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"center_lon":101.535,"center_lat":3.031,"cols":10,"rows":10,"dx_m":20.0,"rainfall_mm_per_hr":180.0,"hyetograph":"peak-storm","duration_s":30.0,"boundary":{"kind":"closed"}}"#,
                ))
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(sim_resp.status(), StatusCode::CREATED);
    let sim_bytes = to_bytes(sim_resp.into_body(), 1024 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    let sim_json: serde_json::Value =
        serde_json::from_slice(&sim_bytes).map_err(|e| e.to_string())?;
    assert_eq!(sim_json["classification"], "numerical-demonstration");
    assert_eq!(sim_json["budget_assessment"]["passed"], true);
    let run_id = sim_json["run_id"]
        .as_str()
        .ok_or_else(|| "missing run_id".to_owned())?
        .to_owned();
    let frame_count = sim_json["frame_count"]
        .as_u64()
        .ok_or_else(|| "missing frame_count".to_owned())? as usize;
    assert!(frame_count >= 2);

    let grid_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/runs/{run_id}/grid"))
                .body(Body::empty())
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(grid_resp.status(), StatusCode::OK);

    let bin_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/runs/{run_id}/frames.bin"))
                .body(Body::empty())
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(bin_resp.status(), StatusCode::OK);
    let bin_bytes = to_bytes(bin_resp.into_body(), 8 * 1024 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(&bin_bytes[0..8], b"ARUSFRM1");
    assert_eq!(bin_bytes.len(), 20 + frame_count * (4 + 100 * 3 * 4));

    let asc_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/runs/{run_id}/export/max-depth.asc"))
                .body(Body::empty())
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(asc_resp.status(), StatusCode::OK);

    let csv_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/runs/{run_id}/export/outlooks.csv"))
                .body(Body::empty())
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(csv_resp.status(), StatusCode::OK);

    // 4. Paired baseline vs intervention comparison computes cut/fill earthwork and delta_max_depth_m.
    let cmp_resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/compare")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"cols":8,"rows":8,"dx_m":20.0,"rainfall_mm_per_hr":180.0,"duration_s":20.0,"boundary":{"kind":"closed"},"interventions":[{"col":3,"row":3,"delta_m":-0.5,"reason":"detention pond"},{"col":4,"row":4,"delta_m":0.6,"reason":"flood bund"}]}"#,
                ))
                .map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(cmp_resp.status(), StatusCode::OK);
    let cmp_bytes = to_bytes(cmp_resp.into_body(), 1024 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    let cmp_json: serde_json::Value =
        serde_json::from_slice(&cmp_bytes).map_err(|e| e.to_string())?;
    assert_eq!(cmp_json["conservation_valid"], true);
    assert!(
        cmp_json["earthwork"]["cut_volume_m3"]
            .as_f64()
            .is_some_and(|v| v > 0.0)
    );
    assert!(
        cmp_json["earthwork"]["fill_volume_m3"]
            .as_f64()
            .is_some_and(|v| v > 0.0)
    );
    Ok(())
}
