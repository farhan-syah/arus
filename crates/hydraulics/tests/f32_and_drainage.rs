use std::path::PathBuf;

use arus_domain::{GridSpec, RunClassification, SpatialReference};
use arus_geo::TerrainGrid;
use arus_hydraulics::{
    DrainageControlSpec, DrainageSurchargeSpec, SurfaceF32RunSpec, SwmmDrainageEngine,
    SwmmDrainageSpec,
};

#[test]
fn surface_f32_screening_matches_closed_basin_rain_within_f32_envelope() -> Result<(), String> {
    let terrain = TerrainGrid::new(
        GridSpec {
            columns: 3,
            rows: 3,
            dx_m: 10.0,
            dy_m: 10.0,
        },
        SpatialReference {
            projected_crs: "synthetic-metric-plane".to_owned(),
            vertical_datum: "synthetic-datum".to_owned(),
            origin_m: [0.0, 0.0],
        },
        "f32-screening-test",
        vec![Some(0.0); 9],
    )
    .map_err(|e| e.to_string())?;

    let spec = SurfaceF32RunSpec {
        run_id: "f32-rain-1".to_owned(),
        classification: RunClassification::NumericalDemonstration,
        terrain,
        uniform_manning_n: 0.03,
        initial_depth_m: 0.0,
        uniform_rain_m_per_s: 1e-4,
        uniform_evap_m_per_s: 0.0,
        uniform_withdrawal_m_per_s: 0.0,
        target_time_s: 60.0,
        max_steps: 500,
    };

    let report = spec.execute().map_err(|e| e.to_string())?;
    assert_eq!(report.precision, "f32-screening");
    assert_eq!(report.accepted_time_s, 60.0);
    assert!(
        (report.max_depth_m - 0.006).abs() < 1e-5,
        "max_depth_m={}",
        report.max_depth_m
    );
    assert!(
        report.residual_m3.abs() < 1e-4,
        "residual_m3={}",
        report.residual_m3
    );
    Ok(())
}

#[test]
fn swmm_drainage_spec_validates_offline_and_reports_missing_worker_binary() -> Result<(), String> {
    let spec = SwmmDrainageSpec {
        inp_content: "[TITLE]\nSynthetic urban drainage test\n[OPTIONS]\nSURCHARGE_METHOD EXTRAN\n"
            .to_owned(),
        vertical_datum: "synthetic-datum".to_owned(),
        surcharge_method: DrainageSurchargeSpec::Extran,
        control_authority: DrainageControlSpec::NativeSwmm,
        worker_binary: PathBuf::from("/nonexistent/swmm-drainage-worker"),
        provenance: "synthetic swmm validation test".to_owned(),
        surcharge_evidence: "EXTRAN surcharge declared in .inp".to_owned(),
    };

    let report = spec.check().map_err(|e| e.to_string())?;
    assert_eq!(report.surcharge_method, "extran");
    assert_eq!(report.control_authority, "native-swmm");
    assert!(!report.worker_binary_exists);
    assert_eq!(report.inp_sha256.len(), 64);

    // Opening with a missing worker binary returns an actionable error without panicking.
    assert!(SwmmDrainageEngine::open(&spec).is_err());

    // Empty .inp is rejected during offline validation.
    let mut bad = spec;
    bad.inp_content = "   ".to_owned();
    assert!(bad.check().is_err());
    Ok(())
}
