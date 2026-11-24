use arus_domain::{GridSpec, RunClassification, SpatialReference};
use arus_geo::TerrainGrid;
use arus_hydraulics::{
    CoastalForcingSpec, LandSpec, NativePipeRunSpec, PipeConduitRunSpec, PipeLateralInflowSpec,
    PipeNodeKindSpec, PipeNodeRunSpec, PipeOutfallSpec, PipeSectionSpec, RainIntervalSpec,
    SpatialOrderSpec, SubgridTerrainAdapter, SurfaceF32RunSpec, SurfaceRunSpec,
    all_component_capabilities,
};
use arus_jobs::StepBudget;
use arus_model_api::CellSample;

fn flat_basin(cols: usize, rows: usize, dx_m: f64) -> TerrainGrid {
    TerrainGrid::new(
        GridSpec {
            columns: cols,
            rows,
            dx_m,
            dy_m: dx_m,
        },
        SpatialReference {
            projected_crs: "EPSG:3375".to_owned(),
            vertical_datum: "local-test-datum".to_owned(),
            origin_m: [0.0, 0.0],
        },
        "synthetic-hydr-020-test",
        vec![Some(0.0); cols * rows],
    )
    .unwrap_or_else(|e| unreachable!("valid flat basin: {e}"))
}

#[test]
fn all_25_hydr_020_components_include_native_pipes_and_3way_coupling() {
    let caps = all_component_capabilities();
    assert_eq!(caps.len(), 25);
    assert!(caps.iter().any(|c| c.contract.contains("native-pipe")));
    assert!(
        caps.iter()
            .any(|c| c.component.contains("channel") && c.component.contains("drainage"))
    );
}

#[test]
fn coastal_wind_forcing_creates_downwind_setup_with_exact_mass_conservation() {
    let spec = SurfaceRunSpec {
        run_id: "coastal-wind-test".to_owned(),
        classification: RunClassification::NumericalDemonstration,
        terrain: flat_basin(8, 3, 25.0),
        uniform_manning_n: 0.025,
        land: LandSpec::UniformImpervious,
        initial_depth_m: 1.0,
        open_faces: Vec::new(),
        forcing: vec![RainIntervalSpec {
            start_s: 0.0,
            end_s: 30.0,
            uniform_rain_m_per_s: 0.0,
            boundaries: Vec::new(),
        }],
        target_time_s: 30.0,
        max_steps: 2_000,
        probes: Vec::new(),
        threshold_m: 0.5,
        hysteresis_m: 0.05,
        recession_persistence_s: 5.0,
        coastal: Some(CoastalForcingSpec {
            wind_x_m_per_s: 25.0,
            wind_y_m_per_s: 0.0,
            ..CoastalForcingSpec::default()
        }),
    };

    let mut engine = spec
        .build_engine()
        .unwrap_or_else(|e| unreachable!("build coastal engine: {e}"));
    let advance = engine
        .advance_to(30.0, StepBudget::new(2_000))
        .unwrap_or_else(|e| unreachable!("advance coastal engine: {e}"));
    assert!((advance.accepted_time_s - 30.0).abs() < 1e-9);
    assert!(engine.water_budget().residual_m3.abs() < 1e-9);

    // Row 1 (y = 37.5 m): upwind col 0 (x = 12.5 m) vs downwind col 7 (x = 187.5 m) under +x wind
    let h_upwind = match engine
        .cell_at(12.5, 37.5)
        .unwrap_or_else(|e| unreachable!("sample upwind: {e}"))
    {
        CellSample::Wet { depth_m, .. } => depth_m,
        other => unreachable!("expected wet upwind cell, got {other:?}"),
    };
    let h_downwind = match engine
        .cell_at(187.5, 37.5)
        .unwrap_or_else(|e| unreachable!("sample downwind: {e}"))
    {
        CellSample::Wet { depth_m, .. } => depth_m,
        other => unreachable!("expected wet downwind cell, got {other:?}"),
    };
    assert!(
        h_downwind > h_upwind + 1e-4,
        "expected downwind surge setup: downwind={h_downwind} vs upwind={h_upwind}"
    );
}

#[test]
fn subgrid_terrain_inverts_stage_volume_and_wet_fraction_exactly() {
    let coarse = GridSpec {
        columns: 2,
        rows: 1,
        dx_m: 20.0,
        dy_m: 20.0,
    };
    // 4x2 fine DEM: left coarse cell has 2x2 subcells [0, 1, 2, 3], right has [1, 1, 1, 1]
    let fine_bed = vec![
        0.0, 1.0, 1.0, 1.0, // fine row 0
        2.0, 3.0, 1.0, 1.0, // fine row 1
    ];
    let adapter = SubgridTerrainAdapter::from_fine_dem(coarse, 4, 2, &fine_bed)
        .unwrap_or_else(|e| unreachable!("build subgrid adapter: {e}"));
    assert_eq!(adapter.cell_count(), 2);

    let proj = adapter
        .project_depths(&[0.5, 0.5])
        .unwrap_or_else(|e| unreachable!("project depths: {e}"));
    assert!(proj.stages_m[0] > 1.0 && proj.stages_m[0] < 2.0);
    assert!((proj.stages_m[1] - 1.5).abs() < 1e-9);
    assert!(proj.wet_fractions[0] > 0.25 && proj.wet_fractions[0] <= 0.75);
    assert!((proj.wet_fractions[1] - 1.0).abs() < 1e-9);

    let roundtrip = adapter
        .effective_depths(&proj.stages_m)
        .unwrap_or_else(|e| unreachable!("roundtrip effective depths: {e}"));
    assert!((roundtrip[0] - 0.5).abs() < 1e-9);
    assert!((roundtrip[1] - 0.5).abs() < 1e-9);
}

#[test]
fn native_pipe_network_solves_pressurized_surcharge_with_mass_conservation() {
    let spec = NativePipeRunSpec {
        run_id: "native-pipe-surcharge".to_owned(),
        classification: RunClassification::NumericalDemonstration,
        vertical_datum: "local-test-datum".to_owned(),
        nodes: vec![
            PipeNodeRunSpec {
                id: "MH1".to_owned(),
                invert_m: 2.0,
                initial_depth_m: 0.2,
                node: PipeNodeKindSpec::Junction {
                    max_depth_m: 3.0,
                    surcharge_depth_m: 2.0,
                    shaft_area_m2: 2.0,
                    ponding_area_m2: 10.0,
                },
            },
            PipeNodeRunSpec {
                id: "OUT".to_owned(),
                invert_m: 1.0,
                initial_depth_m: 0.0,
                node: PipeNodeKindSpec::Outfall {
                    boundary: PipeOutfallSpec::Free,
                },
            },
        ],
        conduits: vec![PipeConduitRunSpec {
            id: "P1".to_owned(),
            from_node: "MH1".to_owned(),
            to_node: "OUT".to_owned(),
            length_m: 100.0,
            section: PipeSectionSpec::Circular { diameter_m: 0.6 },
            manning_n: 0.013,
            in_offset_m: 0.0,
            out_offset_m: 0.0,
            initial_flow_m3_per_s: 0.0,
        }],
        lateral_inflows: vec![PipeLateralInflowSpec {
            node_id: "MH1".to_owned(),
            rate_m3_per_s: 1.2,
        }],
        duration_s: 20.0,
        report_step_s: 5.0,
    };

    let report = spec
        .execute()
        .unwrap_or_else(|e| unreachable!("execute native pipe spec: {e}"));
    assert!((report.accepted_time_s - 20.0).abs() < 1e-9);
    assert!(report.any_surcharged || report.any_pressurized);
    assert!(report.outfall_outflow_m3 > 0.0);
    let rel_err = report.residual_m3.abs() / report.lateral_inflow_m3.max(1.0);
    assert!(
        rel_err < 0.01,
        "expected <1% Preissmann-slot pipe continuity error, got {:.4}% (residual={} m³)",
        rel_err * 100.0,
        report.residual_m3
    );
}

#[test]
fn surface_f32_second_order_muscl_conserves_closed_basin_mass() {
    let spec = SurfaceF32RunSpec {
        run_id: "f32-muscl-test".to_owned(),
        classification: RunClassification::NumericalDemonstration,
        terrain: flat_basin(6, 6, 10.0),
        uniform_manning_n: 0.03,
        initial_depth_m: 0.05,
        uniform_rain_m_per_s: 1e-4,
        uniform_evap_m_per_s: 0.0,
        uniform_withdrawal_m_per_s: 0.0,
        target_time_s: 10.0,
        max_steps: 500,
        spatial_order: SpatialOrderSpec::SecondOrderMuscl,
    };
    let report = spec
        .execute()
        .unwrap_or_else(|e| unreachable!("execute f32 muscl: {e}"));
    assert!((report.accepted_time_s - 10.0).abs() < 1e-5);
    assert!(report.residual_m3.abs() < 1e-2);
}
