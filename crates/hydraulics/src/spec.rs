//! JSON-serializable surface simulation manifest (`SurfaceRunSpec`) driving `SurfaceEngine`.

use arus_domain::RunClassification;
use arus_geo::TerrainGrid;
use arus_model_api::{ModelError, Result};
use hydr::capability::{BoundaryKind, SourceKind};
use hydr::model::{CellState, FlowState};
use hydr::soil::{LandCell, LandModel, ModifiedGreenAmptParameters, SoilState};
use hydr::solver::{
    Axis, BoundaryConditions, BoundaryFace, BoundaryForcing, CoastalForcing, DischargeBoundary,
    ExteriorBoundary, ExteriorState, FluxPort, ForcingInterval, ForcingSchedule, InitialState,
    SolverConfig, SolverSetup, StageBoundary, SubgridTerrain,
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
    SpatiallyVaryingModifiedGreenAmpt {
        suction_m: f64,
        conductivity_m_per_s: Vec<f64>,
        maximum_deficit: f64,
    },
}

/// Grid face normal direction for an open exterior boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BoundaryAxisSpec {
    X,
    Y,
}

/// Exterior face coordinate in the Cartesian grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundaryFaceSpec {
    pub axis: BoundaryAxisSpec,
    pub column: usize,
    pub row: usize,
}

impl BoundaryFaceSpec {
    fn to_hydr(self) -> BoundaryFace {
        BoundaryFace {
            axis: match self.axis {
                BoundaryAxisSpec::X => Axis::X,
                BoundaryAxisSpec::Y => Axis::Y,
            },
            column: self.column,
            row: self.row,
        }
    }
}

/// Physical boundary condition supplied on one declared open face during a forcing interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum BoundaryForcingSpec {
    Stage {
        face: BoundaryFaceSpec,
        elevation_m: f64,
        inflow_tangential_velocity_m_s: Option<f64>,
    },
    Discharge {
        face: BoundaryFaceSpec,
        outward_rate_m3_per_s: f64,
        inflow_tangential_velocity_m_s: Option<f64>,
    },
    ExteriorDry {
        face: BoundaryFaceSpec,
    },
    ExteriorWet {
        face: BoundaryFaceSpec,
        elevation_m: f64,
        outward_velocity_m_s: f64,
        tangential_velocity_m_s: f64,
    },
    Transmissive {
        face: BoundaryFaceSpec,
    },
    FluxPort {
        face: BoundaryFaceSpec,
        water_rate_m3_per_s: f64,
        momentum_rate: [f64; 2],
        signal_speed_m_s: f64,
        provenance: String,
    },
}

impl BoundaryForcingSpec {
    fn boundary_kind(&self) -> BoundaryKind {
        match self {
            Self::Stage { .. } => BoundaryKind::Stage,
            Self::Discharge { .. } => BoundaryKind::Discharge,
            Self::ExteriorDry { .. } | Self::ExteriorWet { .. } => BoundaryKind::Exterior,
            Self::Transmissive { .. } => BoundaryKind::Transmissive,
            Self::FluxPort { .. } => BoundaryKind::FluxPort,
        }
    }

    fn to_hydr(&self) -> BoundaryForcing {
        match self {
            Self::Stage {
                face,
                elevation_m,
                inflow_tangential_velocity_m_s,
            } => BoundaryForcing::Stage(StageBoundary {
                face: face.to_hydr(),
                elevation: *elevation_m,
                inflow_tangential_velocity: *inflow_tangential_velocity_m_s,
            }),
            Self::Discharge {
                face,
                outward_rate_m3_per_s,
                inflow_tangential_velocity_m_s,
            } => BoundaryForcing::Discharge(DischargeBoundary {
                face: face.to_hydr(),
                outward_rate: *outward_rate_m3_per_s,
                inflow_tangential_velocity: *inflow_tangential_velocity_m_s,
            }),
            Self::ExteriorDry { face } => BoundaryForcing::Exterior(ExteriorBoundary {
                face: face.to_hydr(),
                state: ExteriorState::Dry,
            }),
            Self::ExteriorWet {
                face,
                elevation_m,
                outward_velocity_m_s,
                tangential_velocity_m_s,
            } => BoundaryForcing::Exterior(ExteriorBoundary {
                face: face.to_hydr(),
                state: ExteriorState::Wet {
                    elevation: *elevation_m,
                    outward_velocity: *outward_velocity_m_s,
                    tangential_velocity: *tangential_velocity_m_s,
                },
            }),
            Self::Transmissive { face } => BoundaryForcing::Transmissive(face.to_hydr()),
            Self::FluxPort {
                face,
                water_rate_m3_per_s,
                momentum_rate,
                signal_speed_m_s,
                provenance,
            } => BoundaryForcing::Flux(FluxPort {
                face: face.to_hydr(),
                water_rate: *water_rate_m3_per_s,
                momentum_rate: *momentum_rate,
                signal_speed: *signal_speed_m_s,
                provenance: provenance.clone(),
            }),
        }
    }
}

/// Piecewise-constant uniform rainfall and open-boundary forcing interval over active cells.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RainIntervalSpec {
    pub start_s: f64,
    pub end_s: f64,
    pub uniform_rain_m_per_s: f64,
    #[serde(default)]
    pub boundaries: Vec<BoundaryForcingSpec>,
}

/// Named coordinate probe declared in a run manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeSpec {
    pub id: String,
    pub x_m: f64,
    pub y_m: f64,
}

/// Opt-in coastal & meteorological momentum forcing (`hydr 0.2.0`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct CoastalForcingSpec {
    #[serde(default)]
    pub wind_x_m_per_s: f64,
    #[serde(default)]
    pub wind_y_m_per_s: f64,
    #[serde(default)]
    pub coriolis_f_per_s: f64,
    #[serde(default)]
    pub dp_dx_pa_per_m: f64,
    #[serde(default)]
    pub dp_dy_pa_per_m: f64,
}

impl CoastalForcingSpec {
    fn to_hydr(self) -> CoastalForcing {
        CoastalForcing {
            wind_x_m_per_s: self.wind_x_m_per_s,
            wind_y_m_per_s: self.wind_y_m_per_s,
            coriolis_f_per_s: self.coriolis_f_per_s,
            dp_dx_pa_per_m: self.dp_dx_pa_per_m,
            dp_dy_pa_per_m: self.dp_dy_pa_per_m,
            ..CoastalForcing::default()
        }
    }
}

/// Complete manifest-driven surface simulation specification supporting walls and open boundaries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceRunSpec {
    pub run_id: String,
    pub classification: RunClassification,
    pub terrain: TerrainGrid,
    pub uniform_manning_n: f64,
    pub land: LandSpec,
    pub initial_depth_m: f64,
    #[serde(default)]
    pub open_faces: Vec<BoundaryFaceSpec>,
    pub forcing: Vec<RainIntervalSpec>,
    pub target_time_s: f64,
    pub max_steps: usize,
    pub probes: Vec<ProbeSpec>,
    pub threshold_m: f64,
    pub hysteresis_m: f64,
    pub recession_persistence_s: f64,
    #[serde(default)]
    pub coastal: Option<CoastalForcingSpec>,
}

/// Optional per-cell and per-interval physical overrides passed to `SurfaceRunSpec::build_engine_with_overrides`.
#[derive(Debug, Default, Clone, Copy)]
pub struct SurfacePhysicsOverrides<'a> {
    /// Per-cell Manning roughness `n` (`len == cols * rows`) overriding `uniform_manning_n`.
    pub manning_n_per_cell: Option<&'a [f64]>,
    /// Per-cell initial water depths (m) (`len == cols * rows`) overriding `initial_depth_m`.
    pub initial_depths_m: Option<&'a [f64]>,
    /// Per-interval spatially varying rainfall grids (`rain[interval][cell]` in m/s).
    pub interval_rain_grids: Option<&'a [Vec<f64>]>,
    /// Per-interval spatially varying depth-limited evaporation+drainage grids (`loss[interval][cell]` in m/s).
    pub interval_loss_grids: Option<&'a [Vec<f64>]>,
    /// Scalar fallback `(evap_m_s, drain_m_s)` per interval when `interval_loss_grids` is `None`.
    pub interval_losses: Option<&'a [(f64, f64)]>,
    /// Optional exact binary checkpoint bytes (`ARUSHYDR`) preserving `h`, `qx`, `qy`, and soil state.
    pub initial_checkpoint: Option<&'a [u8]>,
    /// Optional fine-resolution DEM `(fine_cols, fine_rows, fine_elevations_m)` for subgrid 2D hydraulics.
    pub subgrid_fine_dem: Option<(usize, usize, &'a [f64])>,
    /// Optional per-coarse-cell sub-pixel river channels `(width_m, depth_m)` incised into `subgrid_fine_dem`.
    pub subgrid_channels: Option<&'a [Option<(f64, f64)>]>,
}

impl SurfaceRunSpec {
    /// Checks capabilities and constructs a `SurfaceEngine` from this manifest.
    pub fn build_engine(&self) -> Result<SurfaceEngine> {
        self.build_engine_with_overrides(SurfacePhysicsOverrides::default())
    }

    /// Constructs a `SurfaceEngine` with optional per-cell initial water depths (for valley/gauge
    /// pre-state assimilation) and per-interval `(potential_evaporation_m_per_s, drainage_m_per_s)`.
    pub fn build_engine_with_physics(
        &self,
        initial_depths_m: Option<&[f64]>,
        interval_losses: Option<&[(f64, f64)]>,
    ) -> Result<SurfaceEngine> {
        self.build_engine_with_overrides(SurfacePhysicsOverrides {
            initial_depths_m,
            interval_losses,
            ..SurfacePhysicsOverrides::default()
        })
    }

    /// Constructs a `SurfaceEngine` with full per-cell roughness, initial depth, spatial rainfall,
    /// and spatial Penman-Monteith evaporation + dynamic infiltration loss grids.
    pub fn build_engine_with_overrides(
        &self,
        overrides: SurfacePhysicsOverrides<'_>,
    ) -> Result<SurfaceEngine> {
        let initial_depths_m = overrides.initial_depths_m;
        let interval_rain_grids = overrides.interval_rain_grids;
        let interval_loss_grids = overrides.interval_loss_grids;
        let interval_losses = overrides.interval_losses;
        let required_sources = match self.land {
            LandSpec::UniformImpervious => [SourceKind::Rain, SourceKind::ImperviousLand],
            LandSpec::UniformModifiedGreenAmpt { .. }
            | LandSpec::SpatiallyVaryingModifiedGreenAmpt { .. } => {
                [SourceKind::Rain, SourceKind::ModifiedGreenAmpt]
            }
        };
        let mut required_boundaries = vec![BoundaryKind::Wall];
        for interval in &self.forcing {
            for boundary in &interval.boundaries {
                let kind = boundary.boundary_kind();
                if !required_boundaries.contains(&kind) {
                    required_boundaries.push(kind);
                }
            }
        }
        check_surface_admission(&required_boundaries, &required_sources)?;

        let n =
            self.terrain.grid().cell_count().ok_or_else(|| {
                ModelError::InvalidInput("terrain cell count overflow".to_owned())
            })?;
        let manning_cells = if let Some(per_cell) = overrides.manning_n_per_cell {
            if per_cell.len() != n {
                return Err(ModelError::InvalidInput(format!(
                    "manning_n_per_cell len {} != grid cell count {n}",
                    per_cell.len()
                )));
            }
            per_cell.to_vec()
        } else {
            vec![self.uniform_manning_n; n]
        };
        let domain = domain_from_terrain(&self.terrain, manning_cells)?;
        let active = domain.active();

        let land_cells = match &self.land {
            LandSpec::UniformImpervious => vec![LandCell::Impervious; n],
            LandSpec::UniformModifiedGreenAmpt {
                suction_m,
                conductivity_m_per_s,
                maximum_deficit,
            } => {
                let params = ModifiedGreenAmptParameters::from_si(
                    *suction_m,
                    *conductivity_m_per_s,
                    *maximum_deficit,
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
            LandSpec::SpatiallyVaryingModifiedGreenAmpt {
                suction_m,
                conductivity_m_per_s,
                maximum_deficit,
            } => {
                if conductivity_m_per_s.len() != n {
                    return Err(ModelError::InvalidInput(format!(
                        "spatially varying conductivity length ({}) != grid cell count ({n})",
                        conductivity_m_per_s.len()
                    )));
                }
                let mut cells = Vec::with_capacity(n);
                for (&is_active, &k) in active.iter().zip(conductivity_m_per_s.iter()) {
                    if !is_active || k <= 0.0 {
                        cells.push(LandCell::Impervious);
                    } else {
                        let params = ModifiedGreenAmptParameters::from_si(
                            *suction_m,
                            k,
                            *maximum_deficit,
                        )
                        .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
                        cells.push(LandCell::ModifiedGreenAmpt(params));
                    }
                }
                cells
            }
        };
        let land = LandModel::new(&domain, land_cells)
            .map_err(|error| ModelError::InvalidInput(error.to_string()))?;

        let (flow, soil) = if let Some(ckpt_bytes) = overrides.initial_checkpoint {
            let pkg = crate::codec::package_from_bytes(ckpt_bytes.to_vec())?;
            let data = crate::codec::decode_snapshot(&pkg)?;
            let (ckpt_nx, ckpt_ny) = data.setup.domain.grid().dimensions();
            let (dom_nx, dom_ny) = domain.grid().dimensions();
            if ckpt_nx != dom_nx || ckpt_ny != dom_ny {
                return Err(ModelError::InvalidInput(format!(
                    "checkpoint grid {ckpt_nx}x{ckpt_ny} != requested {dom_nx}x{dom_ny}",
                )));
            }
            (data.accepted.flow, data.accepted.soil)
        } else {
            let soil = SoilState::initialized(&land)
                .map_err(|error| ModelError::InvalidInput(error.to_string()))?;

            if !self.initial_depth_m.is_finite() || self.initial_depth_m < 0.0 {
                return Err(ModelError::InvalidInput(format!(
                    "initial_depth_m={} must be finite nonnegative",
                    self.initial_depth_m
                )));
            }
            let flow_cells: Vec<CellState> = if let Some(per_cell) = initial_depths_m {
                if per_cell.len() != n {
                    return Err(ModelError::InvalidInput(format!(
                        "initial_depths_m len {} != grid cell count {n}",
                        per_cell.len()
                    )));
                }
                active
                    .iter()
                    .zip(per_cell.iter())
                    .map(|(&is_active, &h)| CellState {
                        h: if is_active && h.is_finite() && h > 0.0 {
                            h
                        } else {
                            0.0
                        },
                        qx: 0.0,
                        qy: 0.0,
                    })
                    .collect()
            } else {
                active
                    .iter()
                    .map(|&is_active| CellState {
                        h: if is_active { self.initial_depth_m } else { 0.0 },
                        qx: 0.0,
                        qy: 0.0,
                    })
                    .collect()
            };
            let flow = FlowState::new(&domain, flow_cells)
                .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
            (flow, soil)
        };

        let boundaries = if self.open_faces.is_empty() {
            BoundaryConditions::impermeable_walls()
        } else {
            let faces = self
                .open_faces
                .iter()
                .copied()
                .map(BoundaryFaceSpec::to_hydr)
                .collect();
            BoundaryConditions::with_open_faces(&domain, faces)
                .map_err(|error| ModelError::InvalidInput(error.to_string()))?
        };
        let intervals: Vec<ForcingInterval> = self
            .forcing
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let (evap, drain) = interval_losses
                    .and_then(|losses| losses.get(idx).copied())
                    .unwrap_or((0.0, 0.0));
                let rain: Vec<f64> = if let Some(grid) =
                    interval_rain_grids.and_then(|grids| grids.get(idx))
                    && grid.len() == n
                {
                    active
                        .iter()
                        .zip(grid.iter())
                        .map(|(&is_active, &r)| {
                            if is_active && r.is_finite() && r > 0.0 {
                                r
                            } else {
                                0.0
                            }
                        })
                        .collect()
                } else {
                    active
                        .iter()
                        .map(|&is_active| {
                            if is_active {
                                item.uniform_rain_m_per_s
                            } else {
                                0.0
                            }
                        })
                        .collect()
                };
                let potential_evaporation: Vec<f64> = if let Some(grid) =
                    interval_loss_grids.and_then(|grids| grids.get(idx))
                    && grid.len() == n
                {
                    active
                        .iter()
                        .zip(grid.iter())
                        .map(|(&is_active, &l)| {
                            if is_active && l.is_finite() && l > 0.0 {
                                l
                            } else {
                                0.0
                            }
                        })
                        .collect()
                } else {
                    active
                        .iter()
                        .map(|&is_active| {
                            if is_active {
                                evap.max(0.0) + drain.max(0.0)
                            } else {
                                0.0
                            }
                        })
                        .collect()
                };
                ForcingInterval {
                    start: item.start_s,
                    end: item.end_s,
                    rain,
                    potential_evaporation,
                    withdrawal: vec![0.0; n],
                    boundaries: item
                        .boundaries
                        .iter()
                        .map(BoundaryForcingSpec::to_hydr)
                        .collect(),
                }
            })
            .collect();
        let forcing = ForcingSchedule::new(&domain, &boundaries, intervals)
            .map_err(|error| ModelError::InvalidInput(error.to_string()))?;

        let grid = self.terrain.grid();
        let min_dx = grid.dx_m.min(grid.dy_m);
        let domain_area_m2 = (grid.columns * grid.rows) as f64 * grid.dx_m * grid.dy_m;
        let config = SolverConfig {
            max_step: (min_dx / 15.0).clamp(1.0, 10.0),
            momentum_film_depth: 1e-4,
            max_cumulative_water_adjustment_m3: (domain_area_m2 * 1e-12).max(1e-9),
            ..SolverConfig::default()
        };
        let subgrid = if let Some((fine_nx, fine_ny, fine_bed)) = overrides.subgrid_fine_dem {
            let mut sg = SubgridTerrain::from_fine_dem(domain.grid(), fine_nx, fine_ny, fine_bed)
                .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
            if let Some(channels) = overrides.subgrid_channels {
                sg = sg
                    .with_channels(channels)
                    .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
            }
            Some(sg)
        } else {
            None
        };
        let mut engine = SurfaceEngine::new(
            SolverSetup {
                domain,
                land,
                boundaries,
                forcing,
                config,
            },
            InitialState {
                time: 0.0,
                flow,
                soil,
            },
        )?;
        if let Some(sg) = subgrid {
            engine = engine.with_subgrid_terrain(sg)?;
        }
        match self.coastal {
            Some(coastal) => engine.with_coastal_forcing(coastal.to_hydr()),
            None => Ok(engine),
        }
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
