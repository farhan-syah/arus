//! Public Axum HTTP API serving immutable archived ARUS runs and location outlooks.

use std::path::PathBuf;
use std::sync::Arc;

use arus_api_types::{API_SCHEMA_VERSION, ApiLocationOutlook};
use arus_assets::Archive;
use arus_storage::{RunIndex, RunRecord, RunStore};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

/// Shared state backing the ARUS API router.
#[derive(Debug, Clone)]
pub struct ApiState {
    archive: Arc<Archive>,
}

impl ApiState {
    /// Opens an archive root for read-only API serving.
    pub fn open(archive_root: &std::path::Path) -> Result<Self, arus_assets::Error> {
        Ok(Self {
            archive: Arc::new(Archive::open(archive_root)?),
        })
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

/// Builds the Axum router with all public ARUS endpoints.
pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/v1/runs", get(list_runs))
        .route("/api/v1/runs/{run_id}", get(get_run))
        .route("/api/v1/runs/{run_id}/outlook", get(get_run_outlook))
        .with_state(state)
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
    /// Socket address to bind.
    #[arg(long, default_value = "127.0.0.1:8080")]
    pub bind: String,
}
