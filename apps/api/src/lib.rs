//! Public Axum HTTP API serving immutable archived ARUS runs, interactive simulations, and the 3D web portal.

pub mod sim;
pub mod video;

use std::path::PathBuf;
use std::sync::Arc;

use arus_api_types::{API_SCHEMA_VERSION, ApiLocationOutlook, ApiSourceSummary};
use arus_assets::Archive;
use arus_ingest::{AccessClass, Catalog, DataClass};
use arus_storage::{PublishedPointer, RunIndex, RunRecord, RunStore};
use arus_web::{
    FONT_INTER_400, FONT_INTER_500, FONT_INTER_600, FONT_JBMONO_400, FONT_JBMONO_500, Locale,
    MAPLIBRE_CSS, MAPLIBRE_JS, STUDIO_CSS, STUDIO_JS, render_index_page_html, render_run_page_html,
    render_sources_page_html, render_studio_page_html,
};
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::sim::{
    CompareSimulationsResponse, CreateSimulationRequest, InteractiveStore, MAX_BODY_BYTES,
    RunGridResponse, SimulationSummaryResponse, export_envelope_csv, export_max_depth_ascii,
};

/// Shared state backing the ARUS API router.
#[derive(Debug, Clone)]
pub struct ApiState {
    archive: Arc<Archive>,
    catalog_path: Option<PathBuf>,
    interactive: Arc<InteractiveStore>,
}

impl ApiState {
    /// Opens an archive root for read-only API serving plus bounded in-memory interactive runs.
    pub fn open(archive_root: &std::path::Path) -> Result<Self, arus_assets::Error> {
        Ok(Self {
            archive: Arc::new(Archive::open(archive_root)?),
            catalog_path: None,
            interactive: Arc::new(InteractiveStore::default()),
        })
    }

    /// Attaches a public source catalog JSON path served at `/api/v1/catalog/sources`.
    #[must_use]
    pub fn with_catalog(mut self, catalog_path: PathBuf) -> Self {
        self.catalog_path = Some(catalog_path);
        self
    }
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    schema_version: &'static str,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Deserialize)]
struct LangQuery {
    lang: Option<String>,
}

/// Builds the Axum router with all public ARUS JSON, binary frame, export, and HTML endpoints.
pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/", get(render_studio_html))
        .route("/archive", get(render_index_html))
        .route("/sources", get(render_sources_html))
        .route("/assets/maplibre-gl.js", get(serve_maplibre_js))
        .route("/assets/maplibre-gl.css", get(serve_maplibre_css))
        .route("/assets/studio.js", get(serve_studio_js))
        .route("/assets/studio.css", get(serve_studio_css))
        .route("/assets/fonts/inter-400.woff2", get(serve_font_inter_400))
        .route("/assets/fonts/inter-500.woff2", get(serve_font_inter_500))
        .route("/assets/fonts/inter-600.woff2", get(serve_font_inter_600))
        .route("/assets/fonts/jbmono-400.woff2", get(serve_font_jbmono_400))
        .route("/assets/fonts/jbmono-500.woff2", get(serve_font_jbmono_500))
        .route("/health", get(health))
        .route("/api/v1/catalog/sources", get(get_catalog_sources))
        .route("/api/v1/published", get(get_published))
        .route("/api/v1/runs", get(list_runs))
        .route("/api/v1/runs/{run_id}", get(get_run))
        .route("/api/v1/runs/{run_id}/outlook", get(get_run_outlook))
        .route("/api/v1/runs/{run_id}/grid", get(get_run_grid))
        .route("/api/v1/runs/{run_id}/frames.bin", get(get_run_frames_bin))
        .route(
            "/api/v1/runs/{run_id}/export/max-depth.asc",
            get(export_run_max_depth_asc),
        )
        .route(
            "/api/v1/runs/{run_id}/export/outlooks.csv",
            get(export_run_outlooks_csv),
        )
        .route("/api/v1/simulations", post(post_simulation))
        .route("/api/v1/compare", post(post_compare))
        .route(
            "/api/v1/export/mp4",
            post(post_export_mp4).layer(DefaultBodyLimit::max(64 * 1024 * 1024)),
        )
        .route("/runs/{run_id}", get(render_run_html))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state)
}

async fn render_studio_html(Query(query): Query<LangQuery>) -> Html<String> {
    let locale = parse_locale(query.lang.as_deref());
    Html(render_studio_page_html(locale))
}

async fn serve_maplibre_js() -> Response {
    static_asset("application/javascript; charset=utf-8", MAPLIBRE_JS)
}

async fn serve_maplibre_css() -> Response {
    static_asset("text/css; charset=utf-8", MAPLIBRE_CSS)
}

async fn serve_studio_js() -> Response {
    static_asset("application/javascript; charset=utf-8", STUDIO_JS)
}

async fn serve_studio_css() -> Response {
    static_asset("text/css; charset=utf-8", STUDIO_CSS)
}

async fn serve_font_inter_400() -> Response {
    static_bytes("font/woff2", FONT_INTER_400)
}

async fn serve_font_inter_500() -> Response {
    static_bytes("font/woff2", FONT_INTER_500)
}

async fn serve_font_inter_600() -> Response {
    static_bytes("font/woff2", FONT_INTER_600)
}

async fn serve_font_jbmono_400() -> Response {
    static_bytes("font/woff2", FONT_JBMONO_400)
}

async fn serve_font_jbmono_500() -> Response {
    static_bytes("font/woff2", FONT_JBMONO_500)
}

fn static_asset(content_type: &'static str, body: &'static str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "public, max-age=3600")
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn static_bytes(content_type: &'static str, body: &'static [u8]) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

async fn post_simulation(
    State(state): State<ApiState>,
    Json(request): Json<CreateSimulationRequest>,
) -> Result<(StatusCode, Json<SimulationSummaryResponse>), (StatusCode, Json<ErrorResponse>)> {
    let sim = state
        .interactive
        .run_simulation(request, None)
        .await
        .map_err(map_sim_error)?;
    Ok((StatusCode::CREATED, Json(sim.summary.clone())))
}

async fn post_compare(
    State(state): State<ApiState>,
    Json(request): Json<CreateSimulationRequest>,
) -> Result<Json<CompareSimulationsResponse>, (StatusCode, Json<ErrorResponse>)> {
    if request.interventions.is_empty() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorResponse {
                error: "compare requires at least one terrain intervention".to_owned(),
            }),
        ));
    }
    let mut baseline_req = request.clone();
    baseline_req.interventions.clear();
    let baseline = state
        .interactive
        .run_simulation(baseline_req, None)
        .await
        .map_err(map_sim_error)?;
    let intervention = state
        .interactive
        .run_simulation(request, None)
        .await
        .map_err(map_sim_error)?;

    let conservation_valid =
        baseline.summary.budget_assessment.passed && intervention.summary.budget_assessment.passed;
    let base_max = &baseline.series.envelope.max_depth_m;
    let int_max = &intervention.series.envelope.max_depth_m;
    let mut delta_max_depth_m = Vec::with_capacity(base_max.len());
    let mut improved_cells = 0_usize;
    let mut worsened_cells = 0_usize;
    let mut max_reduction_m = 0.0_f32;
    let mut max_worsening_m = 0.0_f32;

    for (&b, &i) in base_max.iter().zip(int_max.iter()) {
        let d = i - b;
        delta_max_depth_m.push(d);
        if d < -0.002 {
            improved_cells += 1;
            max_reduction_m = max_reduction_m.max(-d);
        } else if d > 0.002 {
            worsened_cells += 1;
            max_worsening_m = max_worsening_m.max(d);
        }
    }

    Ok(Json(CompareSimulationsResponse {
        baseline_run_id: baseline.summary.run_id.clone(),
        intervention_run_id: intervention.summary.run_id.clone(),
        conservation_valid,
        earthwork: intervention.summary.earthwork,
        improved_cells,
        worsened_cells,
        max_reduction_m,
        max_worsening_m,
        delta_max_depth_m,
    }))
}

async fn get_run_grid(
    State(state): State<ApiState>,
    Path(run_id): Path<String>,
) -> Result<Json<RunGridResponse>, (StatusCode, Json<ErrorResponse>)> {
    let sim = state
        .interactive
        .get(&run_id)
        .ok_or_else(|| not_found(format!("interactive grid for run {run_id} not found")))?;
    Ok(Json(RunGridResponse {
        run_id: sim.summary.run_id.clone(),
        classification: sim.summary.classification,
        accepted_time_s: sim.summary.accepted_time_s,
        steps_taken: sim.summary.steps_taken,
        frame_count: sim.summary.frame_count,
        water_budget: sim.summary.water_budget,
        budget_assessment: sim.summary.budget_assessment.clone(),
        earthwork: sim.summary.earthwork,
        metadata: sim.series.metadata.clone(),
        envelope: sim.series.envelope.clone(),
    }))
}

async fn get_run_frames_bin(
    State(state): State<ApiState>,
    Path(run_id): Path<String>,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let sim = state
        .interactive
        .get(&run_id)
        .ok_or_else(|| not_found(format!("binary frames for run {run_id} not found")))?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from(sim.frames_bin.clone()))
        .map_err(internal_error)
}

async fn export_run_max_depth_asc(
    State(state): State<ApiState>,
    Path(run_id): Path<String>,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let sim = state
        .interactive
        .get(&run_id)
        .ok_or_else(|| not_found(format!("run {run_id} not found for ASCII export")))?;
    let asc = export_max_depth_ascii(&sim).map_err(internal_error)?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{run_id}-max-depth.asc\""),
        )
        .body(Body::from(asc))
        .map_err(internal_error)
}

async fn export_run_outlooks_csv(
    State(state): State<ApiState>,
    Path(run_id): Path<String>,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let sim = state
        .interactive
        .get(&run_id)
        .ok_or_else(|| not_found(format!("run {run_id} not found for CSV export")))?;
    let csv = export_envelope_csv(&sim);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{run_id}-envelope.csv\""),
        )
        .body(Body::from(csv))
        .map_err(internal_error)
}

async fn post_export_mp4(body: Bytes) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let webm = body.to_vec();
    let mp4 = tokio::task::spawn_blocking(move || video::transcode_webm_bytes_to_mp4(&webm))
        .await
        .map_err(|e| internal_error(format!("mp4 export worker join failed: {e}")))?
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })))?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "video/mp4")
        .header(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"arus-recording.mp4\"",
        )
        .body(Body::from(mp4))
        .map_err(internal_error)
}

fn map_sim_error((code, message): (u16, String)) -> (StatusCode, Json<ErrorResponse>) {
    let status = StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(ErrorResponse { error: message }))
}

async fn render_sources_html(
    State(state): State<ApiState>,
    Query(query): Query<LangQuery>,
) -> Result<Html<String>, (StatusCode, Json<ErrorResponse>)> {
    let Some(path) = state.catalog_path.as_deref() else {
        return Err(not_found(
            "no public source catalog is configured".to_owned(),
        ));
    };
    let catalog = Catalog::load(path).map_err(internal_error)?;
    let summaries: Vec<ApiSourceSummary> = catalog
        .sources
        .into_iter()
        .map(|s| ApiSourceSummary {
            id: s.id,
            title: s.title,
            provider: s.provider,
            access: access_label(s.access).to_owned(),
            data_class: data_class_label(s.data_class).to_owned(),
            license_name: s.license.name,
            coverage: s.coverage,
        })
        .collect();
    let locale = parse_locale(query.lang.as_deref());
    Ok(Html(render_sources_page_html(&summaries, locale)))
}

const fn access_label(access: AccessClass) -> &'static str {
    match access {
        AccessClass::Public => "public",
        AccessClass::PublicViewOnly => "public_view_only",
        AccessClass::Registration => "registration",
        AccessClass::RequestOnly => "request_only",
        AccessClass::Unknown => "unknown",
    }
}

const fn data_class_label(class: DataClass) -> &'static str {
    match class {
        DataClass::Observation => "observation",
        DataClass::SatelliteEstimate => "satellite_estimate",
        DataClass::Forecast => "forecast",
        DataClass::StitchedForecastHistory => "stitched_forecast_history",
        DataClass::Reanalysis => "reanalysis",
        DataClass::Terrain => "terrain",
        DataClass::Features => "features",
        DataClass::Imagery => "imagery",
        DataClass::GeneralGuidance => "general_guidance",
    }
}

async fn get_catalog_sources(
    State(state): State<ApiState>,
) -> Result<Json<Catalog>, (StatusCode, Json<ErrorResponse>)> {
    let Some(path) = state.catalog_path.as_deref() else {
        return Err(not_found(
            "no public source catalog is configured".to_owned(),
        ));
    };
    let catalog = Catalog::load(path).map_err(internal_error)?;
    Ok(Json(catalog))
}

async fn render_index_html(
    State(state): State<ApiState>,
    Query(query): Query<LangQuery>,
) -> Result<Html<String>, (StatusCode, Json<ErrorResponse>)> {
    let store = RunStore::new(&state.archive);
    let index = store.load_index().map_err(internal_error)?;
    let published = store.load_published().map_err(internal_error)?;
    let locale = parse_locale(query.lang.as_deref());
    let html = render_index_page_html(
        &index.run_ids,
        index.latest_run_id.as_deref(),
        published.as_ref().map(|p| p.run_id.as_str()),
        locale,
    );
    Ok(Html(html))
}

fn parse_locale(lang: Option<&str>) -> Locale {
    match lang {
        Some("bm" | "ms") => Locale::BahasaMalaysia,
        _ => Locale::English,
    }
}

async fn get_published(
    State(state): State<ApiState>,
) -> Result<Json<PublishedPointer>, (StatusCode, Json<ErrorResponse>)> {
    let store = RunStore::new(&state.archive);
    match store.load_published().map_err(internal_error)? {
        Some(pointer) => Ok(Json(pointer)),
        None => Err(not_found(
            "no operational forecast is currently published".to_owned(),
        )),
    }
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        schema_version: API_SCHEMA_VERSION,
    })
}

async fn list_runs(
    State(state): State<ApiState>,
) -> Result<Json<RunIndex>, (StatusCode, Json<ErrorResponse>)> {
    let store = RunStore::new(&state.archive);
    let index = store.load_index().map_err(internal_error)?;
    Ok(Json(index))
}

async fn get_run(
    State(state): State<ApiState>,
    Path(run_id): Path<String>,
) -> Result<Json<RunRecord>, (StatusCode, Json<ErrorResponse>)> {
    let store = RunStore::new(&state.archive);
    let record = store.get_run(&run_id).map_err(internal_error)?;
    match record {
        Some(record) => Ok(Json(record)),
        None => Err(not_found(format!("run {run_id} not found"))),
    }
}

async fn get_run_outlook(
    State(state): State<ApiState>,
    Path(run_id): Path<String>,
) -> Result<Json<Vec<ApiLocationOutlook>>, (StatusCode, Json<ErrorResponse>)> {
    let store = RunStore::new(&state.archive);
    let Some(record) = store.get_run(&run_id).map_err(internal_error)? else {
        return Err(not_found(format!("run {run_id} not found")));
    };
    let Some(blob) = record.outlook_blob else {
        return Err(not_found(format!("run {run_id} has no persisted outlook")));
    };
    let bytes = state
        .archive
        .read_blob(&blob.sha256)
        .map_err(internal_error)?;
    let outlooks: Vec<ApiLocationOutlook> =
        serde_json::from_slice(&bytes).map_err(internal_error)?;
    Ok(Json(outlooks))
}

async fn render_run_html(
    State(state): State<ApiState>,
    Path(run_id): Path<String>,
    Query(query): Query<LangQuery>,
) -> Result<Html<String>, (StatusCode, Json<ErrorResponse>)> {
    let store = RunStore::new(&state.archive);
    let Some(record) = store.get_run(&run_id).map_err(internal_error)? else {
        return Err(not_found(format!("run {run_id} not found")));
    };
    let Some(blob) = record.outlook_blob else {
        return Err(not_found(format!("run {run_id} has no persisted outlook")));
    };
    let bytes = state
        .archive
        .read_blob(&blob.sha256)
        .map_err(internal_error)?;
    let outlooks: Vec<ApiLocationOutlook> =
        serde_json::from_slice(&bytes).map_err(internal_error)?;
    let locale = parse_locale(query.lang.as_deref());
    Ok(Html(render_run_page_html(&run_id, &outlooks, locale)))
}

fn not_found(message: String) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse { error: message }),
    )
}

fn internal_error<E: std::fmt::Display>(error: E) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: error.to_string(),
        }),
    )
}

/// CLI arguments for `arus-api`.
#[derive(Debug, clap::Parser)]
#[command(name = "arus-api", about = "ARUS public HTTP API server")]
pub struct ApiArgs {
    /// Archive directory containing persisted runs and outlook blobs.
    #[arg(long, default_value = "data/runs")]
    pub archive: PathBuf,
    /// Optional public source catalog JSON file served at `/api/v1/catalog/sources`.
    #[arg(long, default_value = "catalog/sources.json")]
    pub catalog: PathBuf,
    /// Socket address to bind.
    #[arg(long, default_value = "127.0.0.1:8080")]
    pub bind: String,
}
