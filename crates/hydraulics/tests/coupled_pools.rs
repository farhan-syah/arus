use std::fs;
use std::path::PathBuf;

use arus_hydraulics::PoolStructureRunSpec;
use arus_model_api::AdvanceOutcome;

#[test]
fn coupled_pools_gate_and_pump_conserve_total_volume() -> Result<(), String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../benchmarks/manifests/coupled_pools_structures.json");
    let raw = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let spec: PoolStructureRunSpec =
        serde_json::from_str(&raw).map_err(|e| format!("parse {}: {e}", path.display()))?;
    let report = spec.execute().map_err(|e| e.to_string())?;

    assert_eq!(report.outcome, AdvanceOutcome::ReachedTarget);
    assert_eq!(report.accepted_time_s, 20.0);
    assert!(
        report.total_volume_residual_m3.abs() < 1e-9,
        "residual={}",
        report.total_volume_residual_m3
    );
    // Detention pond drains through gate; outfall basin gains water pumped uphill.
    assert!(report.pools[0].volume_m3 < 600.0);
    assert!(report.pools[2].volume_m3 > 500.0);
    Ok(())
}
