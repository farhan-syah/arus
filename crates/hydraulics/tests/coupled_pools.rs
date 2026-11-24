use std::fs;
use std::path::PathBuf;

use arus_domain::RunClassification;
use arus_hydraulics::{
    CulvertBarrelSpec, LevelPoolSpec, PoolKnotSpec, PoolStructureRunSpec, PoolStructureSpec,
    StructureLawSpec,
};
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

#[test]
fn coupled_pools_hds5_culvert_transfers_water_conservatively() -> Result<(), String> {
    // Debug builds of HDS-5 culvert quadrature/root-finding exceed Rust's 2 MiB test-thread stack;
    // match Linux's standard 8 MiB main-thread stack.
    let handle = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(run_hds5_culvert_check)
        .map_err(|e| e.to_string())?;
    handle
        .join()
        .map_err(|_| "culvert worker thread panicked".to_owned())?
}

fn run_hds5_culvert_check() -> Result<(), String> {
    let spec = PoolStructureRunSpec {
        run_id: "hds5-culvert-test".to_owned(),
        classification: RunClassification::NumericalDemonstration,
        vertical_datum: "synthetic-datum".to_owned(),
        pools: vec![
            LevelPoolSpec {
                id: "upstream".to_owned(),
                provenance: "synthetic upstream pond".to_owned(),
                knots: vec![
                    PoolKnotSpec {
                        volume_m3: 0.0,
                        stage_m: 0.0,
                    },
                    PoolKnotSpec {
                        volume_m3: 500.0,
                        stage_m: 5.0,
                    },
                ],
                initial_volume_m3: 300.0,
            },
            LevelPoolSpec {
                id: "downstream".to_owned(),
                provenance: "synthetic downstream pond".to_owned(),
                knots: vec![
                    PoolKnotSpec {
                        volume_m3: 0.0,
                        stage_m: 0.0,
                    },
                    PoolKnotSpec {
                        volume_m3: 500.0,
                        stage_m: 5.0,
                    },
                ],
                initial_volume_m3: 100.0,
            },
        ],
        structures: vec![PoolStructureSpec {
            id: "box-culvert-1".to_owned(),
            from_pool: "upstream".to_owned(),
            from_bed_elevation_m: 0.0,
            to_pool: "downstream".to_owned(),
            to_bed_elevation_m: 0.0,
            law: StructureLawSpec::Culvert {
                barrel: CulvertBarrelSpec::RectangularBox {
                    span_m: 1.5,
                    rise_m: 1.0,
                },
                length_m: 20.0,
                invert_a_m: 0.5,
                invert_b_m: 0.4,
                manning_n: 0.013,
                barrel_count: 1,
                hds5_inlet_code: 9,
                entrance_loss_ke: 0.5,
            },
        }],
        target_time_s: 15.0,
        max_steps: 200,
        maximum_step_s: 2.0,
    };

    let report = spec.execute().map_err(|e| e.to_string())?;
    assert_eq!(report.outcome, AdvanceOutcome::ReachedTarget);
    assert!(report.total_volume_residual_m3.abs() < 1e-9);
    assert!(report.pools[0].volume_m3 < 300.0);
    assert!(report.pools[1].volume_m3 > 100.0);
    Ok(())
}
