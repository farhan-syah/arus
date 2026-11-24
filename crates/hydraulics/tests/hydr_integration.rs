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
