use arus_evaluation::assess_water_budget;
use arus_hydraulics::CoupledSurfaceChannelEngine;
use arus_jobs::{CancellationToken, StepBudget};
use arus_model_api::{AdvanceOutcome, CellSample};
use hydr::channel::{
    ChannelBoundary, ChannelConfig, ChannelGeometry, ChannelInitialState, ChannelReachDefinition,
    RectangularChannelCell,
};
use hydr::coupling::{
    SurfaceChannelConfig, SurfaceChannelForcingInterval, SurfaceChannelInitialState,
    SurfaceChannelLink, SurfaceChannelReach, SurfaceChannelReachForcing, SurfaceChannelSetup,
};
use hydr::grid::CartesianGrid;
use hydr::model::{CellState, Domain, FlowState, SpatialReference};
use hydr::soil::{LandModel, SoilState};
use hydr::solver::{
    BoundaryConditions, ForcingInterval, ForcingSchedule, InitialState, SolverConfig, SolverSetup,
};
use hydr::storage::LevelPoolConfig;
use hydr::structures::WeirApplicability;

fn build_coupled_engine() -> Result<CoupledSurfaceChannelEngine, String> {
    let grid = CartesianGrid::new(2, 1, 10.0, 10.0).map_err(|e| e.to_string())?;
    let n = grid.cell_count();
    let reference = SpatialReference::new(
        "synthetic metric plane".to_owned(),
        "synthetic datum".to_owned(),
        [0.0, 0.0],
    )
    .map_err(|e| e.to_string())?;
    let domain = Domain::new(grid, vec![1.0; n], vec![0.03; n], vec![true; n], reference)
        .map_err(|e| e.to_string())?;
    let land = LandModel::impervious(&domain);
    let soil = SoilState::initialized(&land).map_err(|e| e.to_string())?;
    // Surface starts at elevation 1.8 m (depth 0.8 m above bed 1.0 m)
    let flow = FlowState::new(
        &domain,
        vec![
            CellState {
                h: 0.8,
                qx: 0.0,
                qy: 0.0,
            };
            n
        ],
    )
    .map_err(|e| e.to_string())?;
    let boundaries = BoundaryConditions::impermeable_walls();
    let forcing = ForcingSchedule::new(
        &domain,
        &boundaries,
        vec![ForcingInterval {
            start: 0.0,
            end: 5.0,
            rain: vec![0.0; n],
            potential_evaporation: vec![0.0; n],
            withdrawal: vec![0.0; n],
            boundaries: Vec::new(),
        }],
    )
    .map_err(|e| e.to_string())?;
    let surface_config = SolverConfig::default();

    let reach_def = ChannelReachDefinition {
        vertical_datum: "synthetic datum".to_owned(),
        provenance: "synthetic rectangular channel test".to_owned(),
        geometry: ChannelGeometry::Rectangular {
            cells: vec![
                RectangularChannelCell {
                    id: "ch-0".to_owned(),
                    length_m: 10.0,
                    width_m: 4.0,
                    bed_elevation_m: 0.0,
                    maximum_depth_m: 5.0,
                    manning_n: 0.03,
                },
                RectangularChannelCell {
                    id: "ch-1".to_owned(),
                    length_m: 10.0,
                    width_m: 4.0,
                    bed_elevation_m: 0.0,
                    maximum_depth_m: 5.0,
                    manning_n: 0.03,
                },
            ],
        },
        lateral_bindings: Vec::new(),
        config: ChannelConfig::default(),
    };
    // Channel starts at depth 0.5 m (elevation 0.5 m -> volume = 0.5 * 4 * 10 = 20 m^3 per cell)
    let reach_initial = ChannelInitialState {
        time_s: 0.0,
        volume_m3: vec![20.0, 20.0],
        longitudinal_momentum_m4_per_s: vec![0.0, 0.0],
    };

    let link = SurfaceChannelLink {
        id: "bank-0".to_owned(),
        surface_cell: 0,
        reach_id: "reach-main".to_owned(),
        channel_cell: 0,
        crest_elevation_m: 1.2,
        crest_length_m: 5.0,
        weir_coefficient: 1.7,
        applicability: WeirApplicability {
            coefficient_provenance: "synthetic test coefficient".to_owned(),
            free_nappe_and_approach_assertion: "synthetic bank overflow test".to_owned(),
            submergence_correction_assertion: "Villemonte synthetic test".to_owned(),
            symmetric_reversal: true,
        },
        reach_tangent: [1.0, 0.0],
        mapping_evidence: "synthetic adjacent bank cell".to_owned(),
    };

    let setup = SurfaceChannelSetup {
        surface: SolverSetup {
            domain,
            land,
            boundaries,
            forcing,
            config: surface_config,
        },
        reaches: vec![SurfaceChannelReach {
            id: "reach-main".to_owned(),
            definition: reach_def,
        }],
        links: vec![link],
        ports: Vec::new(),
        pools: Vec::new(),
        pool_support: Vec::new(),
        pool_sources: None,
        pool_inflows: Vec::new(),
        storage: LevelPoolConfig::default(),
        structures: Vec::new(),
        pool_ends: Vec::new(),
        operating: None,
        runoff: None,
        geometry_schedules: Vec::new(),
        pool_curve_schedules: Vec::new(),
        surface_bed_schedules: Vec::new(),
        channel_section_schedules: Vec::new(),
        storage_exclusion_evidence: "disjoint synthetic floodplain and channel cells".to_owned(),
        config: SurfaceChannelConfig {
            head_absolute_m: 1e-6,
            head_relative: 1e-6,
            transfer_absolute_m3: 1e-6,
            transfer_relative: 1e-6,
            mass_absolute_m3: 1e-6,
            mass_relative: 1e-9,
            minimum_step_s: 1e-6,
            maximum_step_s: 0.25,
            iteration_limit: 32,
            retry_limit: 16,
            contraction_target: 0.5,
        },
    };

    let initial = SurfaceChannelInitialState {
        time_s: 0.0,
        surface: InitialState {
            time: 0.0,
            flow,
            soil,
        },
        reaches: vec![("reach-main".to_owned(), reach_initial)],
        pools: Vec::new(),
        runoff: Vec::new(),
    };

    CoupledSurfaceChannelEngine::new(setup, initial).map_err(|e| e.to_string())
}

fn closed_reach_forcing(start_s: f64, end_s: f64) -> SurfaceChannelForcingInterval {
    SurfaceChannelForcingInterval {
        start_s,
        end_s,
        reaches: vec![SurfaceChannelReachForcing {
            reach_id: "reach-main".to_owned(),
            left: Some(ChannelBoundary::Wall),
            right: Some(ChannelBoundary::Wall),
            lateral_inputs: Vec::new(),
        }],
        pool_sources: None,
        pool_inflows: Vec::new(),
        runoff: Vec::new(),
    }
}

#[test]
fn coupled_bank_weir_transfers_water_conservatively_and_supports_step_budget() -> Result<(), String>
{
    let mut engine = build_coupled_engine()?;
    let token = CancellationToken::new();

    // First advance bounded to 2 steps, then resume to 1.0 s.
    let partial = engine
        .advance_to(1.0, StepBudget::new(2), &token, closed_reach_forcing)
        .map_err(|e| e.to_string())?;
    assert_eq!(partial.outcome, AdvanceOutcome::StepBudgetExhausted);
    assert_eq!(partial.steps_taken, 2);

    let done = engine
        .advance_to(1.0, StepBudget::new(100), &token, closed_reach_forcing)
        .map_err(|e| e.to_string())?;
    assert_eq!(done.outcome, AdvanceOutcome::ReachedTarget);
    assert_eq!(engine.time_s(), 1.0);

    let ledgers = engine.link_ledgers().map_err(|e| e.to_string())?;
    assert_eq!(ledgers.len(), 1);
    assert_eq!(ledgers[0].link_id, "bank-0");
    assert!(ledgers[0].surface_to_channel_m3 > 0.0);

    let budget = engine.water_budget().map_err(|e| e.to_string())?;
    let assessment = assess_water_budget(budget);
    assert!(
        assessment.passed,
        "coupled budget failed: {:?}",
        assessment.reasons
    );

    match engine
        .surface_cell_at(5.0, 5.0)
        .map_err(|e| e.to_string())?
    {
        CellSample::Wet { depth_m, .. } => assert!(depth_m < 0.8),
        other => return Err(format!("expected wet surface cell, got {other:?}")),
    }
    Ok(())
}
