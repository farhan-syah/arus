//! JSON-serializable coupled 2D surface + 1D rectangular channel manifest (`CoupledRunSpec`).

use arus_domain::RunClassification;
use arus_geo::TerrainGrid;
use arus_jobs::{CancellationToken, StepBudget};
use arus_model_api::{AdvanceOutcome, ModelError, Result, WaterBudget};
use hydr::channel::{
    ChannelBoundary, ChannelConfig, ChannelGeometry, ChannelInitialState, ChannelReachDefinition,
    RectangularChannelCell,
};
use hydr::coupling::{
    SurfaceChannelConfig, SurfaceChannelForcingInterval, SurfaceChannelInitialState,
    SurfaceChannelLink, SurfaceChannelReach, SurfaceChannelReachForcing, SurfaceChannelSetup,
};
use hydr::model::{CellState, FlowState};
use hydr::soil::{LandModel, SoilState};
use hydr::solver::{
    BoundaryConditions, ForcingInterval, ForcingSchedule, InitialState, SolverConfig, SolverSetup,
};
use hydr::storage::LevelPoolConfig;
use hydr::structures::WeirApplicability;
use serde::{Deserialize, Serialize};

use crate::coupled::{BankExchangeSummary, CoupledSurfaceChannelEngine};
use crate::surface::domain_from_terrain;

/// One rectangular 1D channel cell in a coupled manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RectangularChannelCellSpec {
    pub id: String,
    pub length_m: f64,
    pub width_m: f64,
    pub bed_elevation_m: f64,
    pub maximum_depth_m: f64,
    pub manning_n: f64,
    pub initial_depth_m: f64,
}

/// One 1D rectangular channel reach in a coupled manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelReachSpec {
    pub id: String,
    pub provenance: String,
    pub cells: Vec<RectangularChannelCellSpec>,
}

/// One 2D-surface to 1D-channel bank-weir link in a coupled manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BankLinkSpec {
    pub id: String,
    pub surface_cell: usize,
    pub reach_id: String,
    pub channel_cell: usize,
    pub crest_elevation_m: f64,
    pub crest_length_m: f64,
    pub weir_coefficient: f64,
    pub reach_tangent: [f64; 2],
    pub mapping_evidence: String,
}

/// Complete JSON-serializable coupled 2D surface + 1D channel simulation manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoupledRunSpec {
    pub run_id: String,
    pub classification: RunClassification,
    pub terrain: TerrainGrid,
    pub uniform_manning_n: f64,
    pub initial_surface_depth_m: f64,
    pub uniform_rain_m_per_s: f64,
    pub reaches: Vec<ChannelReachSpec>,
    pub links: Vec<BankLinkSpec>,
    pub storage_exclusion_evidence: String,
    pub target_time_s: f64,
    pub max_steps: usize,
    pub maximum_step_s: f64,
}

/// Composite water-budget and bank-exchange report produced by executing a `CoupledRunSpec`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoupledRunReport {
    pub run_id: String,
    pub classification: RunClassification,
    pub outcome: AdvanceOutcome,
    pub accepted_time_s: f64,
    pub steps_taken: usize,
    pub water_budget: WaterBudget,
    pub bank_exchanges: Vec<BankExchangeSummary>,
}

impl CoupledRunSpec {
    /// Checks inputs, constructs a `CoupledSurfaceChannelEngine`, and advances to `target_time_s`.
    pub fn execute(&self) -> Result<CoupledRunReport> {
        let n =
            self.terrain.grid().cell_count().ok_or_else(|| {
                ModelError::InvalidInput("terrain cell count overflow".to_owned())
            })?;
        let domain = domain_from_terrain(&self.terrain, vec![self.uniform_manning_n; n])?;
        let active = domain.active();
        let land = LandModel::impervious(&domain);
        let soil = SoilState::initialized(&land)
            .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
        let flow_cells: Vec<CellState> = active
            .iter()
            .map(|&is_active| CellState {
                h: if is_active {
                    self.initial_surface_depth_m
                } else {
                    0.0
                },
                qx: 0.0,
                qy: 0.0,
            })
            .collect();
        let flow = FlowState::new(&domain, flow_cells)
            .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
        let boundaries = BoundaryConditions::impermeable_walls();
        let forcing = ForcingSchedule::new(
            &domain,
            &boundaries,
            vec![ForcingInterval {
                start: 0.0,
                end: self.target_time_s,
                rain: active
                    .iter()
                    .map(|&is_active| {
                        if is_active {
                            self.uniform_rain_m_per_s
                        } else {
                            0.0
                        }
                    })
                    .collect(),
                potential_evaporation: vec![0.0; n],
                withdrawal: vec![0.0; n],
                boundaries: Vec::new(),
            }],
        )
        .map_err(|error| ModelError::InvalidInput(error.to_string()))?;

        let vertical_datum = self.terrain.reference().vertical_datum.clone();
        let mut reaches = Vec::with_capacity(self.reaches.len());
        let mut reach_initials = Vec::with_capacity(self.reaches.len());
        let mut reach_ids = Vec::with_capacity(self.reaches.len());

        for reach in &self.reaches {
            let mut cells = Vec::with_capacity(reach.cells.len());
            let mut volumes = Vec::with_capacity(reach.cells.len());
            for cell in &reach.cells {
                volumes.push(cell.initial_depth_m * cell.width_m * cell.length_m);
                cells.push(RectangularChannelCell {
                    id: cell.id.clone(),
                    length_m: cell.length_m,
                    width_m: cell.width_m,
                    bed_elevation_m: cell.bed_elevation_m,
                    maximum_depth_m: cell.maximum_depth_m,
                    manning_n: cell.manning_n,
                });
            }
            reaches.push(SurfaceChannelReach {
                id: reach.id.clone(),
                definition: ChannelReachDefinition {
                    vertical_datum: vertical_datum.clone(),
                    provenance: reach.provenance.clone(),
                    geometry: ChannelGeometry::Rectangular { cells },
                    lateral_bindings: Vec::new(),
                    config: ChannelConfig::default(),
                },
            });
            let cell_count = reach.cells.len();
            reach_initials.push((
                reach.id.clone(),
                ChannelInitialState {
                    time_s: 0.0,
                    volume_m3: volumes,
                    longitudinal_momentum_m4_per_s: vec![0.0; cell_count],
                },
            ));
            reach_ids.push(reach.id.clone());
        }

        let links: Vec<SurfaceChannelLink> = self
            .links
            .iter()
            .map(|link| SurfaceChannelLink {
                id: link.id.clone(),
                surface_cell: link.surface_cell,
                reach_id: link.reach_id.clone(),
                channel_cell: link.channel_cell,
                crest_elevation_m: link.crest_elevation_m,
                crest_length_m: link.crest_length_m,
                weir_coefficient: link.weir_coefficient,
                applicability: WeirApplicability {
                    coefficient_provenance: "manifest-declared bank weir coefficient".to_owned(),
                    free_nappe_and_approach_assertion: "manifest-declared bank overflow".to_owned(),
                    submergence_correction_assertion: "Villemonte submergence correction"
                        .to_owned(),
                    symmetric_reversal: true,
                },
                reach_tangent: link.reach_tangent,
                mapping_evidence: link.mapping_evidence.clone(),
            })
            .collect();

        let setup = SurfaceChannelSetup {
            surface: SolverSetup {
                domain,
                land,
                boundaries,
                forcing,
                config: SolverConfig::default(),
            },
            reaches,
            links,
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
            storage_exclusion_evidence: self.storage_exclusion_evidence.clone(),
            config: SurfaceChannelConfig {
                head_absolute_m: 1e-6,
                head_relative: 1e-6,
                transfer_absolute_m3: 1e-6,
                transfer_relative: 1e-6,
                mass_absolute_m3: 1e-6,
                mass_relative: 1e-9,
                minimum_step_s: 1e-6,
                maximum_step_s: self.maximum_step_s,
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
            reaches: reach_initials,
            pools: Vec::new(),
            runoff: Vec::new(),
        };

        let mut engine = CoupledSurfaceChannelEngine::new(setup, initial)?;
        let token = CancellationToken::new();
        let summary = engine.advance_to(
            self.target_time_s,
            StepBudget::new(self.max_steps),
            &token,
            |start_s, end_s| SurfaceChannelForcingInterval {
                start_s,
                end_s,
                reaches: reach_ids
                    .iter()
                    .map(|id| SurfaceChannelReachForcing {
                        reach_id: id.clone(),
                        left: Some(ChannelBoundary::Wall),
                        right: Some(ChannelBoundary::Wall),
                        lateral_inputs: Vec::new(),
                    })
                    .collect(),
                pool_sources: None,
                pool_inflows: Vec::new(),
                runoff: Vec::new(),
            },
        )?;

        Ok(CoupledRunReport {
            run_id: self.run_id.clone(),
            classification: self.classification,
            outcome: summary.outcome,
            accepted_time_s: summary.accepted_time_s,
            steps_taken: summary.steps_taken,
            water_budget: engine.water_budget()?,
            bank_exchanges: engine.link_ledgers()?,
        })
    }
}
