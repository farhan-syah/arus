//! Native numerical worker for ARUS hydraulic executions.

use std::path::PathBuf;
use std::process::ExitCode;

use arus_api_types::{
    API_SCHEMA_VERSION, ApiEpisodeSummary, ApiLocationOutlook, ApiLocationStatus,
};
use arus_assets::Archive;
use arus_domain::RunClassification;
use arus_evaluation::{BudgetAssessment, assess_water_budget};
use arus_forecasting::{
    EpisodePolicy, EpisodeTracker, LocationEvolution, LocationSupportStatus, OnsetStatus,
    RecessionStatus,
};
use arus_hydraulics::{
    LocationProbe, SurfaceEngine, all_component_capabilities, run_closed_basin_demo,
    run_native_worker_demo, surface_capabilities,
};
use arus_jobs::{CancellationToken, StepBudget};
use arus_model_api::PersistedCheckpoint;
use clap::{Parser, Subcommand};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "arus-worker", about = "Native numerical worker backed by hydr")]
struct Arguments {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print capability declarations from hydr.
    Capabilities {
        /// Print all 23 hydr component declarations instead of only the F64 surface solver.
        #[arg(long)]
        all: bool,
    },
    /// Run synthetic CPU F64 numerical demonstrations and check conservation and replay.
    Demo {
        /// Optional archive directory for persisting exact binary checkpoints.
        #[arg(long)]
        archive: Option<PathBuf>,
    },
    /// Run a synthetic CPU F64 surface demonstration and emit JSON API location outlooks
    /// and numerical budget assessment.
    Outlook {
        /// Depth exceedance threshold in metres.
        #[arg(long, default_value_t = 0.003)]
        threshold_m: f64,
        /// Optional archive directory for persisting the exact binary checkpoint.
        #[arg(long)]
        archive: Option<PathBuf>,
    },
}

#[derive(Debug, Serialize)]
struct OutlookEnvelope {
    classification: RunClassification,
    budget_assessment: BudgetAssessment,
    checkpoint: Option<PersistedCheckpoint>,
    outlooks: Vec<ApiLocationOutlook>,
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
        Command::Capabilities { all } => {
            let json = if all {
                serde_json::to_string_pretty(&all_component_capabilities())
            } else {
                serde_json::to_string_pretty(&surface_capabilities())
            }
            .map_err(|error| error.to_string())?;
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
        Command::Outlook {
            threshold_m,
            archive,
        } => run_outlook_demo(threshold_m, archive),
    }
}

fn run_outlook_demo(threshold_m: f64, archive: Option<PathBuf>) -> Result<(), String> {
    let policy = EpisodePolicy::new(threshold_m, 0.0, 10.0).map_err(|error| error.to_string())?;
    let mut engine = SurfaceEngine::demo_closed_basin_60s().map_err(|error| error.to_string())?;
    let probes = vec![
        LocationProbe {
            id: "basin-center".to_owned(),
            x_m: 25.0,
            y_m: 25.0,
        },
        LocationProbe {
            id: "outside-domain".to_owned(),
            x_m: 200.0,
            y_m: 25.0,
        },
    ];
    let mut trackers: Vec<EpisodeTracker> = probes
        .iter()
        .map(|probe| EpisodeTracker::new(probe.id.clone(), policy))
        .collect();
    for (probe, tracker) in probes.iter().zip(trackers.iter_mut()) {
        let initial = engine
            .cell_at(probe.x_m, probe.y_m)
            .map_err(|error| error.to_string())?;
        tracker
            .observe(engine.time_s(), initial)
            .map_err(|error| error.to_string())?;
    }

    let token = CancellationToken::new();
    let mut callback_error: Option<String> = None;
    engine
        .advance_with_probes(
            60.0,
            StepBudget::new(10_000),
            &token,
            &probes,
            |_, samples| {
                for (tracker, sample) in trackers.iter_mut().zip(samples.iter()) {
                    if let Err(error) = tracker.observe(sample.time_s, sample.sample) {
                        callback_error = Some(error.to_string());
                    }
                }
            },
        )
        .map_err(|error| error.to_string())?;
    if let Some(error) = callback_error {
        return Err(error);
    }

    let budget_assessment = assess_water_budget(engine.water_budget());
    let checkpoint = match archive {
        Some(path) => {
            let opened = Archive::open(&path).map_err(|error| error.to_string())?;
            let package = engine.checkpoint().map_err(|error| error.to_string())?;
            Some(
                package
                    .persist(&opened)
                    .map_err(|error| error.to_string())?,
            )
        }
        None => None,
    };
    let outlooks = trackers
        .into_iter()
        .map(|tracker| to_api_outlook(tracker.finish(), RunClassification::NumericalDemonstration))
        .collect();

    let envelope = OutlookEnvelope {
        classification: RunClassification::NumericalDemonstration,
        budget_assessment,
        checkpoint,
        outlooks,
    };
    let json = serde_json::to_string_pretty(&envelope).map_err(|error| error.to_string())?;
    println!("{json}");
    Ok(())
}

fn to_api_outlook(
    evolution: LocationEvolution,
    classification: RunClassification,
) -> ApiLocationOutlook {
    let support = match evolution.support {
        LocationSupportStatus::Supported => ApiLocationStatus::Supported,
        LocationSupportStatus::Unsupported => ApiLocationStatus::Unsupported,
    };
    let episodes = evolution
        .episodes
        .into_iter()
        .map(|ep| {
            let (onset_status, onset_time_s) = match ep.onset {
                OnsetStatus::Observed { time_s } => ("observed".to_owned(), time_s),
                OnsetStatus::LeftCensored { first_observed_s } => {
                    ("left-censored".to_owned(), first_observed_s)
                }
            };
            let (recession_status, recession_crossing_s, recession_confirmation_s) = match ep
                .recession
            {
                RecessionStatus::Confirmed {
                    crossing_time_s,
                    confirmation_time_s,
                } => (
                    "confirmed".to_owned(),
                    Some(crossing_time_s),
                    Some(confirmation_time_s),
                ),
                RecessionStatus::PendingAtHorizon {
                    crossing_time_s, ..
                } => ("pending-at-horizon".to_owned(), Some(crossing_time_s), None),
                RecessionStatus::RightCensored { .. } => ("right-censored".to_owned(), None, None),
            };
            ApiEpisodeSummary {
                onset_status,
                onset_time_s,
                initial_inundation: ep.initial_inundation,
                peak_depth_m: ep.peak_depth_m,
                peak_time_s: ep.peak_time_s,
                peak_at_horizon_end: ep.peak_at_horizon_end,
                exceedance_duration_s: ep.exceedance_duration_s,
                recession_status,
                recession_crossing_s,
                recession_confirmation_s,
                rebound_count: ep.rebound_count,
            }
        })
        .collect();

    ApiLocationOutlook {
        schema_version: API_SCHEMA_VERSION.to_owned(),
        location_id: evolution.location_id,
        classification,
        support,
        threshold_m: evolution.policy.threshold_m,
        horizon_start_s: evolution.horizon_start_s,
        horizon_end_s: evolution.horizon_end_s,
        max_depth_m: evolution.max_depth_m,
        episodes,
    }
}
