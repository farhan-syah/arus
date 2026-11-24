//! Public Axum HTTP API serving immutable archived ARUS runs and location outlooks.

use std::path::PathBuf;
use std::sync::Arc;

use arus_api_types::{API_SCHEMA_VERSION, ApiLocationOutlook};
use arus_assets::Archive;
use arus_ingest::Catalog;
use arus_storage::{PublishedPointer, RunIndex, RunRecord, RunStore};
use arus_web::{Locale, render_index_page_html, render_run_page_html};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

/// Shared state backing the ARUS API router.
#[derive(Debug, Clone)]
pub struct ApiState {
    archive: Arc<Archive>,
    catalog_path: Option<PathBuf>,
}

impl ApiState {
    /// Opens an archive root for read-only API serving.
    pub fn open(archive_root: &std::path::Path) -> Result<Self, arus_assets::Error> {
        Ok(Self {
            archive: Arc::new(Archive::open(archive_root)?),
            catalog_path: None,
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

/// Builds the Axum router with all public ARUS JSON and HTML endpoints.
pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/", get(render_index_html))
        .route("/health", get(health))
        .route("/api/v1/catalog/sources", get(get_catalog_sources))
        .route("/api/v1/published", get(get_published))
        .route("/api/v1/runs", get(list_runs))
        .route("/api/v1/runs/{run_id}", get(get_run))
        .route("/api/v1/runs/{run_id}/outlook", get(get_run_outlook))
        .route("/runs/{run_id}", get(render_run_html))
        .with_state(state)
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
