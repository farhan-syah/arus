//! JSON-serializable surface simulation manifest (`SurfaceRunSpec`) driving `SurfaceEngine`.

use arus_domain::RunClassification;
use arus_geo::TerrainGrid;
use arus_model_api::{ModelError, Result};
use hydr::capability::{BoundaryKind, SourceKind};
use hydr::model::{CellState, FlowState};
use hydr::soil::{LandCell, LandModel, ModifiedGreenAmptParameters, SoilState};
use hydr::solver::{
    BoundaryConditions, ForcingInterval, ForcingSchedule, InitialState, SolverConfig, SolverSetup,
};
use serde::{Deserialize, Serialize};

use crate::capabilities::check_surface_admission;
use crate::surface::{LocationProbe, SurfaceEngine, domain_from_terrain};

/// Land infiltration mechanism applied to active cells. Inactive cells stay impervious.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum LandSpec {
    UniformImpervious,
    UniformModifiedGreenAmpt {
        suction_m: f64,
        conductivity_m_per_s: f64,
        maximum_deficit: f64,
    },
}

/// Piecewise-constant uniform rainfall interval over active cells.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RainIntervalSpec {
    pub start_s: f64,
    pub end_s: f64,
    pub uniform_rain_m_per_s: f64,
}

/// Named coordinate probe declared in a run manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeSpec {
    pub id: String,
    pub x_m: f64,
    pub y_m: f64,
}

/// Complete manifest-driven closed-basin surface simulation specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceRunSpec {
    pub run_id: String,
    pub classification: RunClassification,
    pub terrain: TerrainGrid,
    pub uniform_manning_n: f64,
    pub land: LandSpec,
    pub initial_depth_m: f64,
    pub forcing: Vec<RainIntervalSpec>,
    pub target_time_s: f64,
    pub max_steps: usize,
    pub probes: Vec<ProbeSpec>,
    pub threshold_m: f64,
    pub hysteresis_m: f64,
    pub recession_persistence_s: f64,
}

impl SurfaceRunSpec {
    /// Checks capabilities and constructs a `SurfaceEngine` from this manifest.
    pub fn build_engine(&self) -> Result<SurfaceEngine> {
        let required_sources = match self.land {
            LandSpec::UniformImpervious => [SourceKind::Rain, SourceKind::ImperviousLand],
            LandSpec::UniformModifiedGreenAmpt { .. } => {
                [SourceKind::Rain, SourceKind::ModifiedGreenAmpt]
            }
        };
        check_surface_admission(&[BoundaryKind::Wall], &required_sources)?;

        let n =
            self.terrain.grid().cell_count().ok_or_else(|| {
                ModelError::InvalidInput("terrain cell count overflow".to_owned())
            })?;
        let domain = domain_from_terrain(&self.terrain, vec![self.uniform_manning_n; n])?;
        let active = domain.active();

        let land_cells = match self.land {
            LandSpec::UniformImpervious => vec![LandCell::Impervious; n],
            LandSpec::UniformModifiedGreenAmpt {
                suction_m,
                conductivity_m_per_s,
                maximum_deficit,
            } => {
                let params = ModifiedGreenAmptParameters::from_si(
                    suction_m,
                    conductivity_m_per_s,
                    maximum_deficit,
                )
                .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
                active
                    .iter()
                    .map(|&is_active| {
                        if is_active {
                            LandCell::ModifiedGreenAmpt(params)
                        } else {
                            LandCell::Impervious
                        }
                    })
                    .collect()
            }
        };
        let land = LandModel::new(&domain, land_cells)
            .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
        let soil = SoilState::initialized(&land)
            .map_err(|error| ModelError::InvalidInput(error.to_string()))?;

        if !self.initial_depth_m.is_finite() || self.initial_depth_m < 0.0 {
            return Err(ModelError::InvalidInput(format!(
                "initial_depth_m={} must be finite nonnegative",
                self.initial_depth_m
            )));
        }
        let flow_cells: Vec<CellState> = active
            .iter()
            .map(|&is_active| CellState {
                h: if is_active { self.initial_depth_m } else { 0.0 },
                qx: 0.0,
                qy: 0.0,
            })
            .collect();
        let flow = FlowState::new(&domain, flow_cells)
            .map_err(|error| ModelError::InvalidInput(error.to_string()))?;

        let boundaries = BoundaryConditions::impermeable_walls();
        let intervals: Vec<ForcingInterval> = self
            .forcing
            .iter()
            .map(|item| ForcingInterval {
                start: item.start_s,
                end: item.end_s,
                rain: active
                    .iter()
                    .map(|&is_active| {
                        if is_active {
                            item.uniform_rain_m_per_s
                        } else {
                            0.0
                        }
                    })
                    .collect(),
                potential_evaporation: vec![0.0; n],
                withdrawal: vec![0.0; n],
                boundaries: Vec::new(),
            })
            .collect();
        let forcing = ForcingSchedule::new(&domain, &boundaries, intervals)
            .map_err(|error| ModelError::InvalidInput(error.to_string()))?;

        SurfaceEngine::new(
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
    }

    /// Converts declared probe specs into `LocationProbe` values.
    pub fn location_probes(&self) -> Vec<LocationProbe> {
        self.probes
            .iter()
            .map(|probe| LocationProbe {
                id: probe.id.clone(),
                x_m: probe.x_m,
                y_m: probe.y_m,
            })
            .collect()
    }
}
