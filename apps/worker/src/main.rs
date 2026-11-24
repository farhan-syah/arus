//! Native numerical worker for ARUS hydraulic executions.

use std::path::PathBuf;
use std::process::ExitCode;

use arus_assets::Archive;
use arus_hydraulics::{run_closed_basin_demo, run_native_worker_demo, surface_capabilities};
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "arus-worker", about = "Native numerical worker backed by hydr")]
struct Arguments {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print surface-solver capability declarations from hydr.
    Capabilities,
    /// Run synthetic CPU F64 numerical demonstrations and check conservation and replay.
    Demo {
        /// Optional archive directory for persisting exact binary checkpoints.
        #[arg(long)]
        archive: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let args = Arguments::parse();
    match execute(args.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("arus-worker error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn execute(command: Command) -> Result<(), String> {
    match command {
        Command::Capabilities => {
            let caps = surface_capabilities();
            let json = serde_json::to_string_pretty(&caps).map_err(|error| error.to_string())?;
            println!("{json}");
            Ok(())
        }
        Command::Demo { archive } => {
            let closed = run_closed_basin_demo().map_err(|error| error.to_string())?;
            let opened = match archive {
                Some(path) => Some(Archive::open(&path).map_err(|error| error.to_string())?),
                None => None,
            };
            let native =
                run_native_worker_demo(opened.as_ref()).map_err(|error| error.to_string())?;
            println!(
                "classification: {}",
                native.execution.provenance.classification.id()
            );
            println!(
                "backend: {} ({})",
                native.capabilities.backend, native.capabilities.contract
            );
            println!(
                "closed_basin: t={} s residual={:.3e} m3 bound={:.3e} m3 checkpoint={} bytes",
                closed.accepted_time_s,
                closed.budget.residual_m3,
                closed.budget.residual_bound_m3,
                closed.checkpoint_bytes
            );
            println!(
                "native_worker: t={} s cancelled_at={} s steps={} boundary_bitwise={} splice_bitwise={} residual={:.3e} m3",
                native.execution.accepted_time_s,
                native.cancelled_at_s,
                native.observed_steps,
                native.boundary_bitwise_match,
                native.forecast_splice_bitwise_match,
                native.execution.budget.residual_m3
            );
            if let Some(checkpoint) = native.persisted_checkpoint {
                println!(
                    "checkpoint: schema={} sha256={} bytes={}",
                    checkpoint.header.schema, checkpoint.blob.sha256, checkpoint.blob.bytes
                );
            }
            Ok(())
        }
    }
}
