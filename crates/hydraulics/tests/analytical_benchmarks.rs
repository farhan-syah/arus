use std::fs;
use std::path::PathBuf;

use arus_evaluation::assess_water_budget;
use arus_hydraulics::SurfaceRunSpec;
use arus_jobs::StepBudget;
use arus_model_api::{AdvanceOutcome, CellSample};

fn load_manifest(name: &str) -> Result<SurfaceRunSpec, String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../benchmarks/manifests")
        .join(name);
    let raw = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse {}: {e}", path.display()))
}

#[test]
fn closed_basin_uniform_rain_matches_analytical_depth() -> Result<(), String> {
    let spec = load_manifest("closed_basin_uniform_rain.json")?;
    let mut engine = spec.build_engine().map_err(|e| e.to_string())?;
    let summary = engine
        .advance_to(spec.target_time_s, StepBudget::new(spec.max_steps))
        .map_err(|e| e.to_string())?;
    assert_eq!(summary.outcome, AdvanceOutcome::ReachedTarget);

    let budget = assess_water_budget(engine.water_budget());
    assert!(budget.passed, "budget failed: {:?}", budget.reasons);

    match engine.cell_at(15.0, 15.0).map_err(|e| e.to_string())? {
        CellSample::Wet {
            depth_m,
            velocity_x_m_s,
            velocity_y_m_s,
            ..
        } => {
            assert!((depth_m - 0.006).abs() < 1e-9, "depth_m={depth_m}");
            assert!(velocity_x_m_s.abs() < 1e-12);
            assert!(velocity_y_m_s.abs() < 1e-12);
        }
        other => return Err(format!("expected wet cell, got {other:?}")),
    }
    Ok(())
}

#[test]
fn lake_at_rest_preserves_still_surface_and_zero_velocity() -> Result<(), String> {
    let spec = load_manifest("lake_at_rest_flat_bed.json")?;
    let mut engine = spec.build_engine().map_err(|e| e.to_string())?;
    let summary = engine
        .advance_to(spec.target_time_s, StepBudget::new(spec.max_steps))
        .map_err(|e| e.to_string())?;
    assert_eq!(summary.outcome, AdvanceOutcome::ReachedTarget);

    let budget = assess_water_budget(engine.water_budget());
    assert!(budget.passed, "budget failed: {:?}", budget.reasons);

    match engine.cell_at(15.0, 15.0).map_err(|e| e.to_string())? {
        CellSample::Wet {
            depth_m,
            water_elevation_m,
            velocity_x_m_s,
            velocity_y_m_s,
        } => {
            assert!((depth_m - 0.5).abs() < 1e-12, "depth_m={depth_m}");
            assert!((water_elevation_m - 1.5).abs() < 1e-12);
            assert!(velocity_x_m_s.abs() < 1e-12);
            assert!(velocity_y_m_s.abs() < 1e-12);
        }
        other => return Err(format!("expected wet cell, got {other:?}")),
    }
    Ok(())
}

#[test]
fn pervious_green_ampt_reduces_surface_depth_and_conserves_water() -> Result<(), String> {
    let spec = load_manifest("pervious_green_ampt_infiltration.json")?;
    let mut engine = spec.build_engine().map_err(|e| e.to_string())?;
    let summary = engine
        .advance_to(spec.target_time_s, StepBudget::new(spec.max_steps))
        .map_err(|e| e.to_string())?;
    assert_eq!(summary.outcome, AdvanceOutcome::ReachedTarget);

    let budget = assess_water_budget(engine.water_budget());
    assert!(budget.passed, "budget failed: {:?}", budget.reasons);

    match engine.cell_at(15.0, 15.0).map_err(|e| e.to_string())? {
        CellSample::Dry => {}
        CellSample::Wet { depth_m, .. } => {
            assert!(
                depth_m < 0.006,
                "expected infiltration below 0.006 m, got {depth_m}"
            );
        }
        other => return Err(format!("unexpected cell sample: {other:?}")),
    }
    Ok(())
}

#[test]
fn open_stage_drainage_drains_water_conservatively() -> Result<(), String> {
    let spec = load_manifest("open_stage_drainage.json")?;
    let mut engine = spec.build_engine().map_err(|e| e.to_string())?;
    let summary = engine
        .advance_to(spec.target_time_s, StepBudget::new(spec.max_steps))
        .map_err(|e| e.to_string())?;
    assert_eq!(summary.outcome, AdvanceOutcome::ReachedTarget);

    let budget = engine.water_budget();
    assert!(budget.outputs_m3 > 0.0, "expected positive outflow");
    let assessment = assess_water_budget(budget);
    assert!(assessment.passed, "budget failed: {:?}", assessment.reasons);

    match engine.cell_at(35.0, 5.0).map_err(|e| e.to_string())? {
        CellSample::Wet { depth_m, .. } => {
            assert!(
                depth_m < 0.4,
                "expected drainage below 0.4 m, got {depth_m}"
            );
        }
        other => return Err(format!("expected wet outlet cell, got {other:?}")),
    }
    Ok(())
}
