//! Binary entrypoint for the ARUS Axum HTTP API server.

use std::process::ExitCode;

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
    let state = ApiState::open(&args.archive).map_err(|error| error.to_string())?;
    let listener = TcpListener::bind(&args.bind)
        .await
        .map_err(|error| format!("bind {} failed: {error}", args.bind))?;
    axum::serve(listener, router(state))
        .await
        .map_err(|error| error.to_string())
}
