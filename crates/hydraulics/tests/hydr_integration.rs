use arus_assets::Archive;
use arus_domain::RunClassification;
use arus_hydraulics::{
    decode_snapshot, encode_snapshot, run_closed_basin_demo, run_native_worker_demo,
    surface_capabilities,
};
use arus_model_api::CellSample;
use hydr::solver::{CONTRACT_VERSION, SNAPSHOT_SCHEMA};
use tempfile::TempDir;

#[test]
fn surface_capabilities_report_hydr_contract_and_schema() {
    let caps = surface_capabilities();
    assert_eq!(caps.backend, "hydr");
    assert_eq!(caps.contract, CONTRACT_VERSION);
    assert_eq!(caps.snapshot_schema, SNAPSHOT_SCHEMA);
    assert!(caps.precision.iter().any(|item| item == "f64"));
    assert!(
        caps.runtimes
            .iter()
            .any(|rt| rt.runtime == "cpu" && rt.status == "reference")
    );
}

#[test]
fn closed_basin_demo_conserves_water_and_round_trips_checkpoint() -> Result<(), String> {
    let report = run_closed_basin_demo().map_err(|error| error.to_string())?;
    assert_eq!(report.accepted_time_s, 60.0);
    assert!(report.budget.within_residual_bound());
    assert!(report.budget.within_adjustment_limit());
    assert!(report.checkpoint_bytes > 0);
    match report.center_sample {
        CellSample::Wet { depth_m, .. } => {
            assert!((depth_m - 0.006).abs() < 1e-12);
        }
        other => return Err(format!("expected wet center cell, got {other:?}")),
    }
    Ok(())
}

#[test]
fn native_worker_demo_persists_checkpoint_and_matches_bitwise() -> Result<(), String> {
    let temp = TempDir::new().map_err(|error| error.to_string())?;
    let archive = Archive::open(temp.path()).map_err(|error| error.to_string())?;
    let report = run_native_worker_demo(Some(&archive)).map_err(|error| error.to_string())?;

    assert_eq!(
        report.execution.provenance.classification,
        RunClassification::NumericalDemonstration
    );
    assert!(report.boundary_bitwise_match);
    assert!(report.forecast_splice_bitwise_match);
    assert!(report.execution.budget.within_residual_bound());
    assert!(report.execution.budget.within_adjustment_limit());
    assert!(report.persisted_checkpoint.is_some());
    Ok(())
}

#[test]
fn codec_rejects_corrupted_payload() -> Result<(), String> {
    let temp = TempDir::new().map_err(|error| error.to_string())?;
    let archive = Archive::open(temp.path()).map_err(|error| error.to_string())?;
    let report = run_native_worker_demo(Some(&archive)).map_err(|error| error.to_string())?;
    let persisted = report
        .persisted_checkpoint
        .ok_or_else(|| "missing persisted checkpoint".to_owned())?;
    let mut package =
        arus_model_api::CheckpointPackage::load(&archive, &persisted).map_err(|e| e.to_string())?;

    let decoded = decode_snapshot(&package).map_err(|error| error.to_string())?;
    let reencoded = encode_snapshot(&decoded).map_err(|error| error.to_string())?;
    assert_eq!(package.payload, reencoded.payload);

    package.payload.pop();
    assert!(decode_snapshot(&package).is_err());
    Ok(())
}

#[test]
fn codec_preserves_signed_zero_and_positive_infinity_bits() -> Result<(), String> {
    let temp = TempDir::new().map_err(|error| error.to_string())?;
    let archive = Archive::open(temp.path()).map_err(|error| error.to_string())?;
    let report = run_native_worker_demo(Some(&archive)).map_err(|error| error.to_string())?;
    let persisted = report
        .persisted_checkpoint
        .ok_or_else(|| "missing persisted checkpoint".to_owned())?;
    let package =
        arus_model_api::CheckpointPackage::load(&archive, &persisted).map_err(|e| e.to_string())?;

    let mut decoded = decode_snapshot(&package).map_err(|error| error.to_string())?;
    decoded.ledger.residual = -0.0;
    if let Some(step) = decoded.last_step.as_mut() {
        step.cfl_limit = f64::INFINITY;
        step.depletion_limit = f64::INFINITY;
    }

    let encoded = encode_snapshot(&decoded).map_err(|error| error.to_string())?;
    let round_tripped = decode_snapshot(&encoded).map_err(|error| error.to_string())?;
    assert_eq!(
        round_tripped.ledger.residual.to_bits(),
        (-0.0_f64).to_bits()
    );
    let step = round_tripped
        .last_step
        .ok_or_else(|| "expected last_step".to_owned())?;
    assert!(step.cfl_limit.is_infinite() && step.cfl_limit.is_sign_positive());
    assert!(step.depletion_limit.is_infinite() && step.depletion_limit.is_sign_positive());
    Ok(())
}

#[test]
fn surface_probes_feed_forecasting_episode_tracker() -> Result<(), String> {
    use arus_forecasting::{
        EpisodePolicy, EpisodeTracker, LocationSupportStatus, OnsetStatus, RecessionStatus,
    };
    use arus_hydraulics::{LocationProbe, SurfaceEngine};
    use arus_jobs::{CancellationToken, StepBudget};
    use hydr::grid::CartesianGrid;
    use hydr::model::{CellState, Domain, FlowState, SpatialReference};
    use hydr::soil::{LandModel, SoilState};
    use hydr::solver::{
        BoundaryConditions, ForcingInterval, ForcingSchedule, InitialState, SolverConfig,
        SolverSetup,
    };

    let grid = CartesianGrid::new(3, 3, 10.0, 10.0).map_err(|e| e.to_string())?;
    let n = grid.cell_count();
    let reference = SpatialReference::new(
        "synthetic metric plane".to_owned(),
        "synthetic datum".to_owned(),
        [0.0, 0.0],
    )
    .map_err(|e| e.to_string())?;
    let domain = Domain::new(grid, vec![0.0; n], vec![0.03; n], vec![true; n], reference)
        .map_err(|e| e.to_string())?;
    let land = LandModel::impervious(&domain);
    let soil = SoilState::initialized(&land).map_err(|e| e.to_string())?;
    let flow = FlowState::new(&domain, vec![CellState::default(); n]).map_err(|e| e.to_string())?;
    let boundaries = BoundaryConditions::impermeable_walls();
    let forcing = ForcingSchedule::new(
        &domain,
        &boundaries,
        vec![ForcingInterval {
            start: 0.0,
            end: 60.0,
            rain: vec![1e-4; n],
            potential_evaporation: vec![0.0; n],
            withdrawal: vec![0.0; n],
            boundaries: Vec::new(),
        }],
    )
    .map_err(|e| e.to_string())?;

    let mut engine = SurfaceEngine::new(
        SolverSetup {
            domain,
            land,
            boundaries,
            forcing,
            config: SolverConfig::default(),
        },
        InitialState {
            time: 0.0,
            flow,
            soil,
        },
    )
    .map_err(|e| e.to_string())?;

    let probes = vec![
        LocationProbe {
            id: "center".to_owned(),
            x_m: 15.0,
            y_m: 15.0,
        },
        LocationProbe {
            id: "outside".to_owned(),
            x_m: 100.0,
            y_m: 15.0,
        },
    ];
    let policy = EpisodePolicy::new(0.003, 0.0, 10.0).map_err(|e| e.to_string())?;
    let mut center_tracker = EpisodeTracker::new("center", policy);
    let mut outside_tracker = EpisodeTracker::new("outside", policy);
    center_tracker
        .observe(0.0, engine.cell_at(15.0, 15.0).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    outside_tracker
        .observe(0.0, engine.cell_at(100.0, 15.0).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;

    let token = CancellationToken::new();
    let mut callback_error: Option<String> = None;
    engine
        .advance_with_probes(
            60.0,
            StepBudget::new(10_000),
            &token,
            &probes,
            |_, samples| {
                for item in samples {
                    let res = match item.id.as_str() {
                        "center" => center_tracker.observe(item.time_s, item.sample),
                        "outside" => outside_tracker.observe(item.time_s, item.sample),
                        _ => Ok(()),
                    };
                    if let Err(err) = res {
                        callback_error = Some(err.to_string());
                    }
                }
            },
        )
        .map_err(|e| e.to_string())?;
    if let Some(err) = callback_error {
        return Err(err);
    }

    let center_evo = center_tracker.finish();
    assert_eq!(center_evo.support, LocationSupportStatus::Supported);
    assert_eq!(center_evo.episodes.len(), 1);
    let ep = &center_evo.episodes[0];
    assert!(!ep.initial_inundation);
    assert!(matches!(ep.onset, OnsetStatus::Observed { time_s } if (time_s - 30.0).abs() <= 1.0));
    assert!(ep.peak_at_horizon_end);
    assert_eq!(
        ep.recession,
        RecessionStatus::RightCensored {
            horizon_end_s: 60.0
        }
    );

    let outside_evo = outside_tracker.finish();
    assert_eq!(outside_evo.support, LocationSupportStatus::Unsupported);
    assert!(outside_evo.episodes.is_empty());
    assert_eq!(outside_evo.max_depth_m, None);
    Ok(())
}
