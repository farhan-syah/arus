use arus_hydrology::SubcatchmentEngine;
use hydr::runoff::{
    SubcatchmentConfig, SubcatchmentDefinition, SubcatchmentGeometry, SubcatchmentInput,
    SubcatchmentParameters, SubcatchmentRouting, SubcatchmentState,
};
use hydr::soil::ModifiedGreenAmptParameters;

fn build_engine() -> Result<SubcatchmentEngine, String> {
    let parameters = SubcatchmentParameters::new(
        SubcatchmentGeometry {
            total_area_m2: 100.0,
            width_m: 10.0,
            slope: 0.01,
            impervious_fraction: 0.5,
            zero_storage_impervious_fraction: 0.5,
            impervious_manning: 0.03,
            pervious_manning: 0.04,
            impervious_depression_m: 0.002,
            pervious_depression_m: 0.004,
        },
        Some(ModifiedGreenAmptParameters::from_si(0.04, 1e-5, 0.3).map_err(|e| e.to_string())?),
        SubcatchmentRouting::ImperviousToPervious { fraction: 0.5 },
    )
    .map_err(|e| e.to_string())?;
    let state = SubcatchmentState::initialized(&parameters).map_err(|e| e.to_string())?;
    SubcatchmentEngine::new(
        vec![SubcatchmentDefinition {
            id: "sub-0".to_owned(),
            parameters,
        }],
        vec![state],
        0.0,
        SubcatchmentConfig::default(),
    )
    .map_err(|e| e.to_string())
}

#[test]
fn subcatchment_runoff_conserves_water_and_restores_snapshot() -> Result<(), String> {
    let mut engine = build_engine()?;
    let input = [SubcatchmentInput {
        rain_rate_m_per_s: 1e-4,
        potential_evaporation_m_per_s: 1e-7,
        external_runon_volumes_m3: [0.0; 3],
    }];

    let step1 = engine.step_to(60.0, &input).map_err(|e| e.to_string())?;
    assert_eq!(step1.end_s, 60.0);
    assert!(step1.rain_m3 > 0.0);
    assert!(step1.infiltration_m3 > 0.0);
    assert!(step1.outlet_m3 > 0.0);
    assert!(step1.residual_m3.abs() < 1e-9);

    // Non-increasing target rejects cleanly without altering state.
    assert!(engine.step_to(60.0, &input).is_err());
    assert_eq!(engine.time_s(), 60.0);

    let snap = engine.snapshot_data().map_err(|e| e.to_string())?;
    let step2 = engine.step_to(120.0, &input).map_err(|e| e.to_string())?;

    let mut restored = build_engine()?;
    restored.restore_data(&snap).map_err(|e| e.to_string())?;
    let replay2 = restored.step_to(120.0, &input).map_err(|e| e.to_string())?;
    assert_eq!(step2, replay2);
    Ok(())
}
