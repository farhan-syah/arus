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

    let index_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/?lang=en")
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
