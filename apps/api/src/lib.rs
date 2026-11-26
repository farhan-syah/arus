//! Public Axum HTTP API serving immutable archived ARUS runs, interactive simulations, and the 3D web portal.

pub mod config;
pub mod context;
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
    MAPLIBRE_CSS, MAPLIBRE_JS, SIM_WORKER_JS, STUDIO_CSS, STUDIO_JS, StudioSeoMetadata,
    render_index_page_html, render_run_page_html, render_sources_page_html,
    render_studio_page_html_with_seo,
};
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::config::ServerConfig;
use crate::context::load_context_async;
use crate::sim::{
    CompareSimulationsResponse, CreateSimulationRequest, InteractiveStore, JobStore,
    MAX_BODY_BYTES, RunGridResponse, SimulationJobRecord, SimulationSummaryResponse,
    export_envelope_csv, export_max_depth_ascii,
};

/// Shared state backing the ARUS API router.
#[derive(Debug, Clone)]
pub struct ApiState {
    archive: Arc<Archive>,
    catalog_path: Option<PathBuf>,
    context_dir: Option<PathBuf>,
    interactive: Arc<InteractiveStore>,
    jobs: Arc<JobStore>,
    config: Arc<ServerConfig>,
}

impl ApiState {
    /// Opens an archive root for read-only API serving plus bounded in-memory interactive runs.
    pub fn open(archive_root: &std::path::Path) -> Result<Self, arus_assets::Error> {
        Ok(Self {
            archive: Arc::new(Archive::open(archive_root)?),
            catalog_path: None,
            context_dir: None,
            interactive: Arc::new(InteractiveStore::default()),
            jobs: Arc::new(JobStore::default()),
            config: Arc::new(ServerConfig::default()),
        })
    }

    /// Attaches a public source catalog JSON path served at `/api/v1/catalog/sources`.
    #[must_use]
    pub fn with_catalog(mut self, catalog_path: PathBuf) -> Self {
        self.catalog_path = Some(catalog_path);
        self
    }

    /// Attaches a validated runtime `ServerConfig` enforced on API routes and exposed at `/api/v1/config`.
    #[must_use]
    pub fn with_config(mut self, config: ServerConfig) -> Self {
        self.config = Arc::new(config);
        self
    }

    /// Attaches an external basin context directory served at `/api/v1/context`.
    #[must_use]
    pub fn with_context_dir(mut self, context_dir: PathBuf) -> Self {
        self.context_dir = Some(context_dir);
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
        .route("/robots.txt", get(serve_robots_txt))
        .route("/sitemap.xml", get(serve_sitemap_xml))
        .route("/site.webmanifest", get(serve_webmanifest))
        .route("/assets/og-card.png", get(serve_og_card_png))
        .route("/archive", get(render_index_html))
        .route("/sources", get(render_sources_html))
        .route("/assets/maplibre-gl.js", get(serve_maplibre_js))
        .route("/assets/maplibre-gl.css", get(serve_maplibre_css))
        .route("/assets/studio.js", get(serve_studio_js))
        .route("/assets/sim-worker.js", get(serve_sim_worker_js))
        .route("/assets/wasm/{file}", get(serve_wasm_asset))
        .route("/assets/studio.css", get(serve_studio_css))
        .route("/assets/fonts/inter-400.woff2", get(serve_font_inter_400))
        .route("/assets/fonts/inter-500.woff2", get(serve_font_inter_500))
        .route("/assets/fonts/inter-600.woff2", get(serve_font_inter_600))
        .route("/assets/fonts/jbmono-400.woff2", get(serve_font_jbmono_400))
        .route("/assets/fonts/jbmono-500.woff2", get(serve_font_jbmono_500))
        .route("/health", get(health))
        .route("/api/v1/config", get(get_config))
        .route("/api/v1/context", get(get_context))
        .route("/api/v1/boundaries", get(get_boundaries))
        .route("/api/v1/live/basins", get(list_live_basins))
        .route("/api/v1/live/{basin}/grid", get(get_live_basin_grid))
        .route(
            "/api/v1/live/{basin}/frames.bin",
            get(get_live_basin_frames_bin),
        )
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
        .route("/api/v1/jobs", get(list_jobs).post(post_job))
        .route("/api/v1/jobs/{job_id}", get(get_job))
        .route("/api/v1/jobs/{job_id}/cancel", post(cancel_job))
        .route(
            "/api/v1/export/mp4",
            post(post_export_mp4).layer(DefaultBodyLimit::max(64 * 1024 * 1024)),
        )
        .route(
            "/api/v1/telemetry/error",
            post(post_client_error).layer(DefaultBodyLimit::max(4 * 1024)),
        )
        .route("/runs/{run_id}", get(render_run_html))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state)
}

async fn render_studio_html(
    State(state): State<ApiState>,
    Query(query): Query<LangQuery>,
) -> Html<String> {
    let locale = parse_locale(query.lang.as_deref());
    let mut seo = StudioSeoMetadata::default();
    if !state.config.instance_title.trim().is_empty() {
        seo.title = state.config.instance_title.clone();
    }
    if let Some(desc) = &state.config.meta_description {
        seo.description = desc.clone();
    }
    if let Some(kw) = &state.config.meta_keywords {
        seo.keywords = kw.clone();
    }
    seo.public_base_url = state.config.public_base_url.clone();
    Html(render_studio_page_html_with_seo(locale, &seo))
}

async fn serve_robots_txt(State(state): State<ApiState>) -> Response {
    let sitemap_line = state
        .config
        .public_base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|b| format!("Sitemap: {}/sitemap.xml\n", b.trim_end_matches('/')))
        .unwrap_or_default();
    let body = format!("User-agent: *\nAllow: /\nDisallow: /api/\n{sitemap_line}");
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::CACHE_CONTROL, "public, max-age=3600")
        .body(Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn serve_sitemap_xml(State(state): State<ApiState>) -> Response {
    let base = state
        .config
        .public_base_url
        .as_deref()
        .map(|s| s.trim().trim_end_matches('/'))
        .filter(|s| !s.is_empty())
        .unwrap_or("https://localhost");
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n\
         <url><loc>{base}/</loc><changefreq>hourly</changefreq><priority>1.0</priority></url>\n\
         <url><loc>{base}/sources</loc><changefreq>daily</changefreq><priority>0.6</priority></url>\n\
         </urlset>\n"
    );
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
        .header(header::CACHE_CONTROL, "public, max-age=3600")
        .body(Body::from(xml))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn serve_webmanifest(State(state): State<ApiState>) -> Response {
    let manifest = serde_json::json!({
        "name": state.config.instance_title,
        "short_name": "ARUS",
        "description": state.config.meta_description.as_deref().unwrap_or("Live flood outlook and 2D shallow-water flood simulation studio."),
        "start_url": "/",
        "display": "standalone",
        "background_color": "#f8fafc",
        "theme_color": "#0284c7",
        "icons": [
            { "src": "/assets/og-card.png", "sizes": "1200x630", "type": "image/png" }
        ]
    });
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            "application/manifest+json; charset=utf-8",
        )
        .header(header::CACHE_CONTROL, "public, max-age=3600")
        .body(Body::from(manifest.to_string()))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn serve_og_card_png(State(state): State<ApiState>) -> Response {
    if let Some(dir) = &state.context_dir {
        let candidates = [
            dir.join("og-card.png"),
            dir.parent()
                .map(|p| p.join("data/og-card.png"))
                .unwrap_or_default(),
        ];
        for path in candidates {
            if let Ok(bytes) = tokio::fs::read(&path).await {
                return Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "image/png")
                    .header(header::CACHE_CONTROL, "public, max-age=86400")
                    .body(Body::from(bytes))
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
            }
        }
    }
    StatusCode::NOT_FOUND.into_response()
}

async fn serve_maplibre_js() -> Response {
    immutable_asset(
        "application/javascript; charset=utf-8",
        MAPLIBRE_JS.as_bytes(),
    )
}

async fn serve_maplibre_css() -> Response {
    immutable_asset("text/css; charset=utf-8", MAPLIBRE_CSS.as_bytes())
}

async fn serve_studio_js(headers: axum::http::HeaderMap) -> Response {
    revalidated_asset(
        "application/javascript; charset=utf-8",
        STUDIO_JS.as_bytes(),
        &headers,
    )
}

async fn serve_sim_worker_js(headers: axum::http::HeaderMap) -> Response {
    revalidated_asset(
        "application/javascript; charset=utf-8",
        SIM_WORKER_JS.as_bytes(),
        &headers,
    )
}

async fn serve_wasm_asset(Path(file): Path<String>, headers: axum::http::HeaderMap) -> Response {
    let content_type = match file.as_str() {
        "arus_wasm_solver.js" => "application/javascript; charset=utf-8",
        "arus_wasm_solver_bg.wasm" => "application/wasm",
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let base_dir = std::env::var_os("ARUS_WASM_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("apps/web/assets/wasm"));
    let path = base_dir.join(&file);
    match tokio::fs::read(&path).await {
        Ok(bytes) => revalidated_asset(content_type, &bytes, &headers),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn serve_studio_css(headers: axum::http::HeaderMap) -> Response {
    revalidated_asset("text/css; charset=utf-8", STUDIO_CSS.as_bytes(), &headers)
}

async fn serve_font_inter_400() -> Response {
    immutable_asset("font/woff2", FONT_INTER_400)
}

async fn serve_font_inter_500() -> Response {
    immutable_asset("font/woff2", FONT_INTER_500)
}

async fn serve_font_inter_600() -> Response {
    immutable_asset("font/woff2", FONT_INTER_600)
}

async fn serve_font_jbmono_400() -> Response {
    immutable_asset("font/woff2", FONT_JBMONO_400)
}

async fn serve_font_jbmono_500() -> Response {
    immutable_asset("font/woff2", FONT_JBMONO_500)
}

fn fnv1a_etag(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("\"{hash:016x}-{}\"", bytes.len())
}

fn revalidated_asset(
    content_type: &'static str,
    body: &[u8],
    req_headers: &axum::http::HeaderMap,
) -> Response {
    let etag = fnv1a_etag(body);
    if req_headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim() == etag)
    {
        return Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(header::ETAG, etag)
            .header(header::CACHE_CONTROL, "public, max-age=0, must-revalidate")
            .body(Body::empty())
            .unwrap_or_else(|_| StatusCode::NOT_MODIFIED.into_response());
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::ETAG, etag)
        .header(header::CACHE_CONTROL, "public, max-age=0, must-revalidate")
        .body(Body::from(body.to_vec()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn immutable_asset(content_type: &'static str, body: &'static [u8]) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn interactive_limits(cfg: &ServerConfig) -> sim::ExecutionLimits {
    let mut lim = sim::ExecutionLimits::interactive();
    lim.max_cells = cfg.limits.max_interactive_cells.min(lim.max_cells);
    lim.timeout_secs = cfg.limits.sim_timeout_secs.clamp(5, lim.timeout_secs);
    lim
}

async fn post_simulation(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<CreateSimulationRequest>,
) -> Result<(StatusCode, Json<SimulationSummaryResponse>), (StatusCode, Json<ErrorResponse>)> {
    if !state.config.features.allow_simulation {
        let internal_ok = std::env::var("ARUS_INTERNAL_TOKEN")
            .ok()
            .filter(|t| !t.is_empty())
            .is_some_and(|expected| {
                headers
                    .get("x-arus-internal-token")
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|got| got == expected)
            });
        if !internal_ok {
            return Err(forbidden(
                "Interactive simulation is disabled on this server",
            ));
        }
    }
    if request.ascii_dem.is_some() && !state.config.features.allow_byod_dem_upload {
        return Err(forbidden(
            "BYOD ASCII DEM upload is disabled on this server",
        ));
    }
    if !request.interventions.is_empty() && !state.config.features.allow_interventions {
        return Err(forbidden(
            "terrain interventions are disabled on this server",
        ));
    }
    let limits = interactive_limits(&state.config);
    let sim = state
        .interactive
        .run_simulation_with_limits(request, None, limits)
        .await
        .map_err(map_sim_error)?;
    Ok((StatusCode::CREATED, Json(sim.summary.clone())))
}

async fn post_compare(
    State(state): State<ApiState>,
    Json(request): Json<CreateSimulationRequest>,
) -> Result<Json<CompareSimulationsResponse>, (StatusCode, Json<ErrorResponse>)> {
    if !state.config.features.allow_interventions {
        return Err(forbidden(
            "terrain interventions are disabled on this server",
        ));
    }
    if request.ascii_dem.is_some() && !state.config.features.allow_byod_dem_upload {
        return Err(forbidden(
            "BYOD ASCII DEM upload is disabled on this server",
        ));
    }
    if request.interventions.is_empty() && request.baseline_interventions.is_empty() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorResponse {
                error: "compare requires at least one terrain intervention".to_owned(),
            }),
        ));
    }
    let mut baseline_req = request.clone();
    baseline_req.interventions = request.baseline_interventions.clone();
    baseline_req.baseline_interventions.clear();
    let mut int_req = request;
    int_req.baseline_interventions.clear();
    let limits = interactive_limits(&state.config);
    let baseline = state
        .interactive
        .run_simulation_with_limits(baseline_req, None, limits)
        .await
        .map_err(map_sim_error)?;
    let intervention = state
        .interactive
        .run_simulation_with_limits(int_req, None, limits)
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
    if !state.config.features.allow_gis_export {
        return Err(forbidden("GIS export is disabled on this server"));
    }
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
    if !state.config.features.allow_gis_export {
        return Err(forbidden("GIS export is disabled on this server"));
    }
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

async fn post_export_mp4(
    State(state): State<ApiState>,
    req: axum::extract::Request,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    // Reject disabled MP4 export BEFORE allocating up to 64 MiB of request body
    if !state.config.features.allow_mp4_export {
        return Err(forbidden(
            "server-side MP4 export is disabled on this server",
        ));
    }
    let body = axum::body::to_bytes(req.into_body(), 64 * 1024 * 1024)
        .await
        .map_err(|e| {
            (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(ErrorResponse {
                    error: format!("mp4 body too large: {e}"),
                }),
            )
        })?;
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
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let Some(path) = state.catalog_path.as_deref() else {
        return Err(not_found(
            "no public source catalog is configured".to_owned(),
        ));
    };
    let catalog = Catalog::load(path).map_err(internal_error)?;
    let body = serde_json::to_vec(&catalog).map_err(internal_error)?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "public, max-age=300")
        .body(Body::from(body))
        .map_err(internal_error)
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

#[derive(Debug, Deserialize)]
struct ContextQuery {
    basin: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClientErrorPayload {
    kind: String,
    message: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    engine: Option<String>,
}

async fn post_client_error(
    State(state): State<ApiState>,
    Json(payload): Json<ClientErrorPayload>,
) -> StatusCode {
    let ts = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    let entry = serde_json::json!({
        "ts": ts,
        "kind": payload.kind.chars().take(48).collect::<String>(),
        "message": payload.message.chars().take(500).collect::<String>(),
        "source": payload.source.map(|s| s.chars().take(160).collect::<String>()),
        "profile": payload.profile.map(|s| s.chars().take(24).collect::<String>()),
        "engine": payload.engine.map(|s| s.chars().take(32).collect::<String>()),
    });
    let log_path = state.archive.root().join("client-errors.jsonl");
    let line = format!("{entry}\n");
    let _ = tokio::task::spawn_blocking(move || {
        use std::io::Write;
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            let _ = file.write_all(line.as_bytes());
        }
    })
    .await;
    StatusCode::NO_CONTENT
}

async fn get_config(State(state): State<ApiState>) -> Response {
    let body = serde_json::to_vec(&*state.config).unwrap_or_default();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "public, max-age=300")
        .body(Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn get_boundaries(State(state): State<ApiState>) -> Response {
    let Some(dir) = state.context_dir.clone() else {
        return Json(serde_json::json!({ "type": "FeatureCollection", "features": [] }))
            .into_response();
    };
    let boundaries_dir = dir.join("boundaries");
    let features = tokio::task::spawn_blocking(move || {
        let mut all: Vec<serde_json::Value> = Vec::new();
        let Ok(entries) = std::fs::read_dir(&boundaries_dir) else {
            return all;
        };
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("geojson"))
            .collect();
        paths.sort();
        for path in paths {
            if let Ok(bytes) = std::fs::read(&path)
                && let Ok(val) = serde_json::from_slice::<serde_json::Value>(&bytes)
                && let Some(arr) = val.get("features").and_then(|f| f.as_array())
            {
                all.extend(arr.iter().cloned());
            }
        }
        all
    })
    .await
    .unwrap_or_default();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "public, max-age=3600")
        .body(Body::from(
            serde_json::json!({ "type": "FeatureCollection", "features": features }).to_string(),
        ))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn list_live_basins(State(state): State<ApiState>) -> Response {
    let Some(dir) = state.context_dir.clone() else {
        return Json(serde_json::json!({ "basins": [] })).into_response();
    };
    let live_dir = dir.join("live");
    let basins = tokio::task::spawn_blocking(move || {
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&live_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str()
                    && context::validate_basin_id(name).is_ok()
                    && entry.path().join("grid.json").is_file()
                    && entry.path().join("frames.bin").is_file()
                {
                    out.push(name.to_owned());
                }
            }
        }
        out.sort();
        out
    })
    .await
    .unwrap_or_default();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "public, max-age=120")
        .body(Body::from(
            serde_json::json!({ "basins": basins }).to_string(),
        ))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn get_live_basin_grid(
    State(state): State<ApiState>,
    Path(basin): Path<String>,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    crate::context::validate_basin_id(&basin).map_err(|e| map_sim_error((400, e)))?;
    let Some(dir) = state.context_dir.clone() else {
        return Err(not_found("no live context directory configured".to_owned()));
    };
    let path = dir.join("live").join(&basin).join("grid.json");
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| not_found(format!("live grid for basin {basin} not cached")))?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "public, max-age=120")
        .body(Body::from(bytes))
        .map_err(internal_error)
}

async fn get_live_basin_frames_bin(
    State(state): State<ApiState>,
    Path(basin): Path<String>,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    crate::context::validate_basin_id(&basin).map_err(|e| map_sim_error((400, e)))?;
    let Some(dir) = state.context_dir.clone() else {
        return Err(not_found("no live context directory configured".to_owned()));
    };
    let path = dir.join("live").join(&basin).join("frames.bin");
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| not_found(format!("live frames for basin {basin} not cached")))?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CACHE_CONTROL, "public, max-age=120")
        .body(Body::from(bytes))
        .map_err(internal_error)
}

async fn get_context(
    State(state): State<ApiState>,
    Query(query): Query<ContextQuery>,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let Some(dir) = state.context_dir.clone() else {
        return Err(not_found(
            "no regional context directory is configured".to_owned(),
        ));
    };
    let resp = load_context_async(dir, query.basin)
        .await
        .map_err(map_sim_error)?;
    let body = serde_json::to_vec(&resp).map_err(internal_error)?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "public, max-age=60")
        .body(Body::from(body))
        .map_err(internal_error)
}

fn has_internal_token(headers: &axum::http::HeaderMap) -> bool {
    std::env::var("ARUS_INTERNAL_TOKEN")
        .ok()
        .filter(|t| !t.is_empty())
        .is_some_and(|expected| {
            headers
                .get("x-arus-internal-token")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|got| got == expected)
        })
}

async fn post_job(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<CreateSimulationRequest>,
) -> Result<(StatusCode, Json<SimulationJobRecord>), (StatusCode, Json<ErrorResponse>)> {
    if !state.config.features.allow_async_jobs && !has_internal_token(&headers) {
        return Err(forbidden(
            "async high-resolution jobs are disabled on this server",
        ));
    }
    if request.ascii_dem.is_some() && !state.config.features.allow_byod_dem_upload {
        return Err(forbidden(
            "BYOD ASCII DEM upload is disabled on this server",
        ));
    }
    let record = state
        .jobs
        .submit(Arc::clone(&state.interactive), request)
        .map_err(|(code, error)| {
            (
                StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_REQUEST),
                Json(ErrorResponse { error }),
            )
        })?;
    Ok((StatusCode::ACCEPTED, Json(record)))
}

async fn list_jobs(
    State(state): State<ApiState>,
) -> Result<Json<Vec<SimulationJobRecord>>, (StatusCode, Json<ErrorResponse>)> {
    if !state.config.features.allow_async_jobs {
        return Err(forbidden(
            "async high-resolution jobs are disabled on this server",
        ));
    }
    Ok(Json(state.jobs.list()))
}

async fn get_job(
    State(state): State<ApiState>,
    headers: axum::http::HeaderMap,
    Path(job_id): Path<String>,
) -> Result<Json<SimulationJobRecord>, (StatusCode, Json<ErrorResponse>)> {
    if !state.config.features.allow_async_jobs && !has_internal_token(&headers) {
        return Err(forbidden(
            "async high-resolution jobs are disabled on this server",
        ));
    }
    state
        .jobs
        .get(&job_id)
        .map(Json)
        .ok_or_else(|| not_found(format!("job {job_id} not found")))
}

async fn cancel_job(
    State(state): State<ApiState>,
    Path(job_id): Path<String>,
) -> Result<Json<SimulationJobRecord>, (StatusCode, Json<ErrorResponse>)> {
    if !state.config.features.allow_async_jobs {
        return Err(forbidden(
            "async high-resolution jobs are disabled on this server",
        ));
    }
    state
        .jobs
        .cancel(&job_id)
        .map(Json)
        .ok_or_else(|| not_found(format!("job {job_id} not found")))
}

fn forbidden(message: &str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::FORBIDDEN,
        Json(ErrorResponse {
            error: message.to_owned(),
        }),
    )
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
    /// Optional server runtime config JSON file (feature toggles, default view, limits).
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Optional directory containing published basin context JSON files served at `/api/v1/context`.
    #[arg(long)]
    pub context_dir: Option<PathBuf>,
    /// Socket address to bind.
    #[arg(long, default_value = "127.0.0.1:8080")]
    pub bind: String,
}

#[cfg(test)]
mod job_queue_tests {
    use super::*;
    use crate::sim::JobStatus;

    #[tokio::test]
    async fn async_job_completes_and_populates_run_cache() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let state = ApiState::open(dir.path()).map_err(|e| e.to_string())?;

        let req: CreateSimulationRequest = serde_json::from_str(
            r#"{"cols":8,"rows":8,"dx_m":50.0,"rainfall_mm_per_hr":80.0,"duration_s":30.0}"#,
        )
        .map_err(|e| e.to_string())?;

        let submitted = state
            .jobs
            .submit(Arc::clone(&state.interactive), req)
            .map_err(|(_, m)| m)?;
        assert_eq!(submitted.status, JobStatus::Queued);

        let mut final_rec = submitted.clone();
        for _ in 0..100 {
            if let Some(rec) = state.jobs.get(&submitted.job_id) {
                final_rec = rec;
                if matches!(
                    final_rec.status,
                    JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled
                ) {
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        assert_eq!(final_rec.status, JobStatus::Completed);
        assert!((final_rec.progress_pct - 100.0).abs() < 1e-3);
        let summary = final_rec
            .summary
            .as_ref()
            .ok_or_else(|| "missing summary on completed job".to_owned())?;
        assert!(summary.budget_assessment.passed);
        assert!(state.interactive.get(&final_rec.run_id).is_some());
        Ok(())
    }

    #[tokio::test]
    async fn hosted_config_enforces_feature_toggles_with_403() -> Result<(), String> {
        use axum::http::Request;
        use tower::ServiceExt;

        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let state = ApiState::open(dir.path())
            .map_err(|e| e.to_string())?
            .with_config(ServerConfig::hosted_default());
        let app = router(state);

        // GET /api/v1/config returns hosted mode and feature flags
        let cfg_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/config")
                    .body(Body::empty())
                    .map_err(|e| e.to_string())?,
            )
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(cfg_resp.status(), StatusCode::OK);

        // POST /api/v1/jobs returns 403 Forbidden under hosted default config
        let job_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/jobs")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"cols":8,"rows":8,"dx_m":50.0,"rainfall_mm_per_hr":80.0,"duration_s":30.0}"#,
                    ))
                    .map_err(|e| e.to_string())?,
            )
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(job_resp.status(), StatusCode::FORBIDDEN);

        // POST /api/v1/export/mp4 returns 403 Forbidden under hosted default config
        let mp4_resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/export/mp4")
                    .body(Body::from(vec![0u8; 8]))
                    .map_err(|e| e.to_string())?,
            )
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(mp4_resp.status(), StatusCode::FORBIDDEN);
        Ok(())
    }

    #[tokio::test]
    async fn physics_toggles_content_hash_cache_and_compression_guard_work() -> Result<(), String> {
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        let state = ApiState::open(dir.path()).map_err(|e| e.to_string())?;

        // 1. Over-compressed 168h over 360s (> 240x) must return 422 instead of truncating mass
        let mut too_compressed: CreateSimulationRequest =
            serde_json::from_str(r#"{"cols":6,"rows":6,"dx_m":50.0,"duration_s":360.0}"#)
                .map_err(|e| e.to_string())?;
        too_compressed.hourly_rain_mm = vec![5.0; 168];
        let err = state
            .interactive
            .run_simulation(too_compressed, None)
            .await
            .err()
            .ok_or_else(|| "expected 422 for >240x time compression".to_owned())?;
        assert_eq!(err.0, 422);

        // 2. Identical request hits content-hash cache and returns same run_id
        let base_req: CreateSimulationRequest = serde_json::from_str(
            r#"{"cols":6,"rows":6,"dx_m":50.0,"rainfall_mm_per_hr":80.0,"duration_s":30.0,"start_dry":false}"#,
        )
        .map_err(|e| e.to_string())?;
        let first = state
            .interactive
            .run_simulation(base_req.clone(), None)
            .await
            .map_err(|(_, m)| m)?;
        let second = state
            .interactive
            .run_simulation(base_req.clone(), None)
            .await
            .map_err(|(_, m)| m)?;
        assert_eq!(first.summary.run_id, second.summary.run_id);

        // 3. start_dry: true produces 0.0 m at t = 0 and a distinct cache entry
        let mut dry_req = base_req;
        dry_req.start_dry = true;
        let dry_run = state
            .interactive
            .run_simulation(dry_req, None)
            .await
            .map_err(|(_, m)| m)?;
        assert_ne!(dry_run.summary.run_id, first.summary.run_id);
        let max_t0_dry = dry_run.series.frames[0]
            .depth_m
            .iter()
            .copied()
            .fold(0.0_f32, f32::max);
        assert!(max_t0_dry < 1e-6);
        assert!(dry_run.summary.budget_assessment.passed);
        Ok(())
    }

    #[tokio::test]
    async fn hourly_forcing_preserves_flash_spike_and_recession() -> Result<(), String> {
        let state = ApiState::open(std::path::Path::new("/tmp/arus-flash-test-empty"))
            .map_err(|e| e.to_string())?;
        let mut hourly = vec![0.0_f64; 72];
        hourly[10] = 65.0; // Single 1-hour convective cloudburst inside a 3-day run
        let mut req: CreateSimulationRequest = serde_json::from_str(
            r#"{"cols":8,"rows":8,"dx_m":60.0,"rainfall_mm_per_hr":0.0,"duration_s":1200.0,"drainage_mm_per_hr":4.0,"evaporation_mm_per_day":4.0,"start_dry":true}"#,
        )
        .map_err(|e| e.to_string())?;
        req.hourly_rain_mm = hourly;
        let sim = state
            .interactive
            .run_simulation(req, None)
            .await
            .map_err(|(_, m)| m)?;
        assert!(sim.summary.budget_assessment.passed);
        let peak_m = sim
            .series
            .envelope
            .max_depth_m
            .iter()
            .copied()
            .fold(0.0_f32, f32::max);
        let final_m = sim
            .series
            .frames
            .last()
            .map(|f| f.depth_m.iter().copied().fold(0.0_f32, f32::max))
            .unwrap_or(0.0);
        assert!(
            peak_m > 0.10 && final_m < peak_m * 0.80,
            "expected flash spike + recession: peak={peak_m:.4} m, final={final_m:.4} m"
        );
        Ok(())
    }

    #[tokio::test]
    async fn coastal_sea_outfall_drains_sloped_shoreline() -> Result<(), String> {
        let state = ApiState::open(std::path::Path::new("/tmp/arus-coast-test-empty"))
            .map_err(|e| e.to_string())?;
        let (cols, rows) = (10usize, 8usize);
        // Columns 0..2 are masked sea (None); columns 2..10 slope gently upward from 1 m to 8 m
        let mut elevs = Vec::with_capacity(cols * rows);
        for _r in 0..rows {
            for c in 0..cols {
                if c < 2 {
                    elevs.push(None);
                } else {
                    elevs.push(Some((c - 1) as f64));
                }
            }
        }
        let mut req: CreateSimulationRequest = serde_json::from_str(
            r#"{"cols":10,"rows":8,"dx_m":100.0,"rainfall_mm_per_hr":0.0,"duration_s":600.0,"drainage_mm_per_hr":2.0,"evaporation_mm_per_day":3.0,"start_dry":true,"tide_mode":"off"}"#,
        )
        .map_err(|e| e.to_string())?;
        req.terrain_elevations_m = Some(elevs);
        req.terrain_provenance = Some("BrowserTerrariumScreening".to_owned());
        req.hourly_rain_mm = vec![8.0; 24];
        let sim = state
            .interactive
            .run_simulation(req, None)
            .await
            .map_err(|(_, m)| m)?;
        assert!(sim.summary.budget_assessment.passed);
        let last = sim.series.frames.last().ok_or("missing final frame")?;
        let max_shore = (0..rows)
            .map(|r| last.depth_m[r * cols + 2])
            .fold(0.0_f32, f32::max);
        assert!(
            max_shore < 0.05,
            "shoreline cells should drain into masked sea instead of piling up: max_shore={max_shore:.4} m"
        );
        Ok(())
    }
}
