//! Binary entrypoint for the ARUS Axum HTTP API server.

use std::process::ExitCode;

use arus_api::config::ServerConfig;
use arus_api::{ApiArgs, ApiState, router};
use clap::Parser;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> ExitCode {
    let args = ApiArgs::parse();
    match serve(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("arus-api error: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn serve(args: ApiArgs) -> Result<(), String> {
    let mut state = ApiState::open(&args.archive).map_err(|error| error.to_string())?;
    if args.catalog.exists() {
        state = state.with_catalog(args.catalog);
    }
    if let Some(config_path) = args.config {
        let cfg = ServerConfig::load_json(&config_path)?;
        state = state.with_config(cfg);
    }
    if let Some(context_dir) = args.context_dir {
        state = state.with_context_dir(context_dir);
    }
    let listener = TcpListener::bind(&args.bind)
        .await
        .map_err(|error| format!("bind {} failed: {error}", args.bind))?;
    axum::serve(listener, router(state))
        .await
        .map_err(|error| error.to_string())
}
