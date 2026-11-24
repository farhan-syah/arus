//! Coupled level-pool storage and hydraulic structures (weir, orifice, gate, pump) over `hydr`.

use arus_domain::RunClassification;
use arus_jobs::{CancellationToken, StepBudget};
use arus_model_api::{AdvanceOutcome, ModelError, Result};
use hydr::coupling::{
    CoupledLevelPoolBatch, CoupledLevelPoolConfig, CoupledLevelPoolInitialState,
    CoupledLevelPoolSetup, CoupledPoolAttachment, CoupledPoolConnection, CoupledPoolSupport,
};
use hydr::storage::{
    LevelPoolConfig, LevelPoolCurve, LevelPoolElement, LevelPoolKnot, LevelPoolState,
};
use hydr::structures::{
    CulvertAlignment, CulvertApplicability, CulvertBarrel, CulvertEnd, CulvertEnds, CulvertInlet,
    CulvertParameters, DirectionPolicy, GateApplicability, GateParameters, OrificeApplicability,
    PumpApplicability, PumpCurve, PumpCurveKnot, PumpParameters, PumpSpeedPolicy,
    StructureCondition, StructureElement, StructureLaw, StructureOperatingPoint,
    StructureOperation, SubmergedOrificeParameters, SuppressedRectangularWeirParameters,
    WeirApplicability,
};
use serde::{Deserialize, Serialize};

use crate::runtime::{ReferenceRuntime, reference_cpu_client};

const GRAVITY_M_PER_S2: f64 = 9.80665;

/// One stage-volume knot on a level-pool storage curve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PoolKnotSpec {
    pub volume_m3: f64,
    pub stage_m: f64,
}

/// One detention pond, reservoir, or storage cell in a coupled-pools manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LevelPoolSpec {
    pub id: String,
    pub provenance: String,
    pub knots: Vec<PoolKnotSpec>,
    pub initial_volume_m3: f64,
}

/// One head-discharge knot on a reference-speed pump curve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PumpKnotSpec {
    pub head_m: f64,
    pub discharge_m3_per_s: f64,
}

/// Cross-section shape of an HDS-5 culvert barrel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "kebab-case")]
pub enum CulvertBarrelSpec {
    Circular { diameter_m: f64 },
    RectangularBox { span_m: f64, rise_m: f64 },
}

/// Supported hydraulic structure laws connecting two level pools.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum StructureLawSpec {
    Weir {
        crest_length_m: f64,
        crest_m: f64,
        discharge_coefficient: f64,
    },
    SubmergedOrifice {
        area_m2: f64,
        sill_m: f64,
        crown_m: f64,
        discharge_coefficient: f64,
    },
    Gate {
        width_m: f64,
        sill_m: f64,
        maximum_opening_m: f64,
        opening_m: f64,
        discharge_coefficient: f64,
    },
    Pump {
        minimum_suction_stage_m: f64,
        knots: Vec<PumpKnotSpec>,
        /// `Some(1.0)` runs at reference speed; `None` isolates the pump stopped.
        running_speed_ratio: Option<f64>,
    },
    Culvert {
        barrel: CulvertBarrelSpec,
        length_m: f64,
        invert_a_m: f64,
        invert_b_m: f64,
        manning_n: f64,
        barrel_count: u32,
        /// HDS-5 Table A.1/A.2 inlet code (1..=8 circular, 9..=26 concrete box).
        hds5_inlet_code: u8,
        entrance_loss_ke: f64,
    },
}

/// One structure link connecting `from_pool` (endpoint A) to `to_pool` (endpoint B).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoolStructureSpec {
    pub id: String,
    pub from_pool: String,
    pub from_bed_elevation_m: f64,
    pub to_pool: String,
    pub to_bed_elevation_m: f64,
    pub law: StructureLawSpec,
}

/// Complete JSON-serializable coupled level-pool + hydraulic structures manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoolStructureRunSpec {
    pub run_id: String,
    pub classification: RunClassification,
    pub vertical_datum: String,
    pub pools: Vec<LevelPoolSpec>,
    pub structures: Vec<PoolStructureSpec>,
    pub target_time_s: f64,
    pub max_steps: usize,
    pub maximum_step_s: f64,
}

/// Final storage state of one pool after executing a `PoolStructureRunSpec`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoolFinalState {
    pub pool_id: String,
    pub volume_m3: f64,
    pub stage_m: Option<f64>,
}

/// Final-step transfer summary for one hydraulic structure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructureTransferSummary {
    pub structure_id: String,
    pub last_signed_volume_m3: f64,
}

/// Report emitted after advancing a coupled level-pool + structures network.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoolStructureRunReport {
    pub run_id: String,
    pub classification: RunClassification,
    pub outcome: AdvanceOutcome,
    pub accepted_time_s: f64,
    pub steps_taken: usize,
    pub initial_total_volume_m3: f64,
    pub final_total_volume_m3: f64,
    pub total_volume_residual_m3: f64,
    pub pools: Vec<PoolFinalState>,
    pub last_transfers: Vec<StructureTransferSummary>,
}

/// Deterministic CPU `F64` coupled level-pool + hydraulic structure engine.
pub struct CoupledPoolsEngine {
    batch: CoupledLevelPoolBatch<ReferenceRuntime>,
}

impl CoupledPoolsEngine {
    /// Checks setup and initial storage and constructs the engine on CPU `F64`.
    pub fn new(
        setup: CoupledLevelPoolSetup,
        initial: CoupledLevelPoolInitialState,
    ) -> Result<Self> {
        let batch = CoupledLevelPoolBatch::new(reference_cpu_client(), setup, initial)
            .map_err(|error| ModelError::Engine(error.to_string()))?;
        Ok(Self { batch })
    }

    /// Advances toward `target_s`, committing each accepted interval atomically.
    pub fn advance_to(
        &mut self,
        target_s: f64,
        budget: StepBudget,
        token: &CancellationToken,
    ) -> Result<(AdvanceOutcome, usize)> {
        if !target_s.is_finite() || target_s < self.batch.time() {
            return Err(ModelError::InvalidInput(format!(
                "target_s={target_s} is invalid for current time {} s",
                self.batch.time()
            )));
        }
        let mut steps_taken = 0_usize;
        while self.batch.time() < target_s {
            if token.is_cancelled() {
                return Ok((AdvanceOutcome::Cancelled, steps_taken));
            }
            if steps_taken >= budget.max_steps {
                return Ok((AdvanceOutcome::StepBudgetExhausted, steps_taken));
            }
            let trial = self
                .batch
                .trial_to(target_s)
                .map_err(|error| ModelError::Engine(error.to_string()))?;
            self.batch = trial.into_next();
            steps_taken += 1;
        }
        Ok((AdvanceOutcome::ReachedTarget, steps_taken))
    }

    /// Returns accepted elapsed time in seconds.
    pub fn time_s(&self) -> f64 {
        self.batch.time()
    }

    /// Projects authoritative final volumes and checked physical stages for every pool.
    pub fn pool_states(&self) -> Result<Vec<PoolFinalState>> {
        let elements = self.batch.pools().elements();
        let states = self
            .batch
            .pools()
            .states()
            .map_err(|error| ModelError::Engine(error.to_string()))?;
        let outputs = self
            .batch
            .pools()
            .outputs()
            .map_err(|error| ModelError::Engine(error.to_string()))?;
        Ok(elements
            .iter()
            .zip(states.iter().zip(outputs.iter()))
            .map(|(elem, (state, out))| PoolFinalState {
                pool_id: elem.id.clone(),
                volume_m3: state.volume_m3,
                stage_m: out.stage_m,
            })
            .collect())
    }

    /// Projects last-step signed transfers for every structure.
    pub fn last_transfers(&self) -> Result<Vec<StructureTransferSummary>> {
        let Some(diag) = self
            .batch
            .last_step()
            .map_err(|error| ModelError::Engine(error.to_string()))?
        else {
            return Ok(Vec::new());
        };
        Ok(diag
            .transfers
            .into_iter()
            .map(|t| StructureTransferSummary {
                structure_id: t.structure_id,
                last_signed_volume_m3: t.signed_volume_m3,
            })
            .collect())
    }
}

impl PoolStructureRunSpec {
    /// Checks manifest inputs, builds `CoupledPoolsEngine`, and advances to `target_time_s`.
    pub fn execute(&self) -> Result<PoolStructureRunReport> {
        let mut pools = Vec::with_capacity(self.pools.len());
        let mut pool_support = Vec::with_capacity(self.pools.len());
        let mut initial_states = Vec::with_capacity(self.pools.len());
        let mut initial_total_volume_m3 = 0.0;

        for pool in &self.pools {
            let knots: Vec<LevelPoolKnot> = pool
                .knots
                .iter()
                .map(|k| LevelPoolKnot {
                    volume_m3: k.volume_m3,
                    stage_m: k.stage_m,
                })
                .collect();
            let zero_stage = knots.first().map(|k| k.stage_m).ok_or_else(|| {
                ModelError::InvalidInput(format!("pool {} has no curve knots", pool.id))
            })?;
            let curve =
                LevelPoolCurve::new(knots, self.vertical_datum.clone(), pool.provenance.clone())
                    .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
            pools.push(LevelPoolElement {
                id: pool.id.clone(),
                curve,
                connected_pool_assertion: format!("manifest level pool {}", pool.id),
                storage_support_identity: format!("support-{}", pool.id),
            });
            pool_support.push(CoupledPoolSupport {
                pool_id: pool.id.clone(),
                zero_volume_stage_m: zero_stage,
                zero_volume_limit_assertion: format!("zero-volume datum for {}", pool.id),
            });
            initial_states.push(LevelPoolState {
                volume_m3: pool.initial_volume_m3,
            });
            initial_total_volume_m3 += pool.initial_volume_m3;
        }

        let mut structures = Vec::with_capacity(self.structures.len());
        let mut operations = Vec::with_capacity(self.structures.len());
        let mut connections = Vec::with_capacity(self.structures.len());

        for st in &self.structures {
            let (law, direction, operating_point) = match &st.law {
                StructureLawSpec::Weir {
                    crest_length_m,
                    crest_m,
                    discharge_coefficient,
                } => {
                    let params = SuppressedRectangularWeirParameters::new(
                        *crest_length_m,
                        *crest_m,
                        *discharge_coefficient,
                        WeirApplicability {
                            coefficient_provenance: "manifest weir coefficient".to_owned(),
                            free_nappe_and_approach_assertion: "quiescent pool overflow".to_owned(),
                            submergence_correction_assertion: "Villemonte submergence".to_owned(),
                            symmetric_reversal: true,
                        },
                    )
                    .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
                    (
                        StructureLaw::SuppressedRectangularWeir(params),
                        DirectionPolicy::Bidirectional,
                        StructureOperatingPoint::Passive,
                    )
                }
                StructureLawSpec::SubmergedOrifice {
                    area_m2,
                    sill_m,
                    crown_m,
                    discharge_coefficient,
                } => {
                    let params = SubmergedOrificeParameters::new(
                        *area_m2,
                        *sill_m,
                        *crown_m,
                        *discharge_coefficient,
                        GRAVITY_M_PER_S2,
                        OrificeApplicability {
                            coefficient_provenance: "manifest orifice coefficient".to_owned(),
                            fully_submerged_applicability: "quiescent submerged opening".to_owned(),
                            symmetric_reversal: true,
                        },
                    )
                    .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
                    (
                        StructureLaw::SubmergedOrifice(params),
                        DirectionPolicy::Bidirectional,
                        StructureOperatingPoint::Passive,
                    )
                }
                StructureLawSpec::Gate {
                    width_m,
                    sill_m,
                    maximum_opening_m,
                    opening_m,
                    discharge_coefficient,
                } => {
                    let params = GateParameters::new(
                        *width_m,
                        *sill_m,
                        *maximum_opening_m,
                        *discharge_coefficient,
                        GRAVITY_M_PER_S2,
                        GateApplicability {
                            coefficient_provenance: "manifest gate coefficient".to_owned(),
                            hydrostatic_quiescent_pool_assertion: "quiescent pools".to_owned(),
                            free_submerged_aperture_applicability: "manifest gate envelope"
                                .to_owned(),
                            unobstructed_aperture_assertion: "unobstructed aperture".to_owned(),
                            symmetric_reversal: true,
                        },
                    )
                    .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
                    (
                        StructureLaw::HydrostaticRectangularGate(params),
                        DirectionPolicy::Bidirectional,
                        StructureOperatingPoint::GateOpening {
                            opening_m: *opening_m,
                        },
                    )
                }
                StructureLawSpec::Pump {
                    minimum_suction_stage_m,
                    knots,
                    running_speed_ratio,
                } => {
                    let curve = PumpCurve::new(
                        knots
                            .iter()
                            .map(|k| PumpCurveKnot {
                                head_m: k.head_m,
                                discharge_m3_per_s: k.discharge_m3_per_s,
                            })
                            .collect(),
                        "manifest reference pump curve".to_owned(),
                    )
                    .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
                    let params = PumpParameters::new(
                        curve,
                        *minimum_suction_stage_m,
                        PumpSpeedPolicy::ReferenceOnly,
                        PumpApplicability {
                            endpoint_stage_head_assertion: "quiescent pool stages".to_owned(),
                            connection_losses_and_velocity_head_account: "included in curve"
                                .to_owned(),
                            stopped_isolation_assertion: "check valve isolates reverse flow"
                                .to_owned(),
                            operating_envelope: "manifest pump operating envelope".to_owned(),
                        },
                    )
                    .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
                    let op = match running_speed_ratio {
                        Some(speed_ratio) => StructureOperatingPoint::PumpRunning {
                            speed_ratio: *speed_ratio,
                        },
                        None => StructureOperatingPoint::PumpStoppedIsolated,
                    };
                    (StructureLaw::Pump(params), DirectionPolicy::AtoBOnly, op)
                }
                StructureLawSpec::Culvert {
                    barrel,
                    length_m,
                    invert_a_m,
                    invert_b_m,
                    manning_n,
                    barrel_count,
                    hds5_inlet_code,
                    entrance_loss_ke,
                } => {
                    let hydr_barrel = match barrel {
                        CulvertBarrelSpec::Circular { diameter_m } => {
                            CulvertBarrel::circular(*diameter_m)
                        }
                        CulvertBarrelSpec::RectangularBox { span_m, rise_m } => {
                            CulvertBarrel::rectangular_box(*span_m, *rise_m)
                        }
                    }
                    .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
                    let inlet = CulvertInlet::hds5(*hds5_inlet_code)
                        .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
                    let end = CulvertEnd::new(inlet, *entrance_loss_ke)
                        .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
                    let params = CulvertParameters::new(
                        hydr_barrel,
                        CulvertAlignment {
                            length_m: *length_m,
                            invert_a_m: *invert_a_m,
                            invert_b_m: *invert_b_m,
                        },
                        *manning_n,
                        *barrel_count,
                        CulvertEnds {
                            a: Some(end),
                            b: Some(end),
                        },
                        GRAVITY_M_PER_S2,
                        CulvertApplicability {
                            coefficient_provenance: format!("HDS-5 inlet code {hds5_inlet_code}"),
                            barrel_condition_assertion: "unobstructed manifest culvert barrel"
                                .to_owned(),
                        },
                    )
                    .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
                    (
                        StructureLaw::Culvert(params),
                        DirectionPolicy::Bidirectional,
                        StructureOperatingPoint::Passive,
                    )
                }
            };

            structures.push(StructureElement {
                id: st.id.clone(),
                endpoint_a: st.from_pool.clone(),
                endpoint_b: st.to_pool.clone(),
                datum: self.vertical_datum.clone(),
                direction,
                law,
                condition: StructureCondition::AsBuilt,
            });
            operations.push(StructureOperation {
                structure_id: st.id.clone(),
                operating_point,
            });
            connections.push(CoupledPoolConnection {
                structure_id: st.id.clone(),
                endpoint_a: CoupledPoolAttachment {
                    pool_id: st.from_pool.clone(),
                    bed_elevation_m: st.from_bed_elevation_m,
                    physical_connection_assertion: format!("{} -> {}", st.from_pool, st.id),
                },
                endpoint_b: CoupledPoolAttachment {
                    pool_id: st.to_pool.clone(),
                    bed_elevation_m: st.to_bed_elevation_m,
                    physical_connection_assertion: format!("{} -> {}", st.id, st.to_pool),
                },
            });
        }

        let setup = CoupledLevelPoolSetup {
            source_policies: None,
            external_inflows: Vec::new(),
            pools,
            pool_support,
            structures,
            operations,
            connections,
            config: CoupledLevelPoolConfig {
                storage: LevelPoolConfig::default(),
                head_absolute_m: 1e-6,
                transfer_absolute_m3: 1e-6,
                mass_absolute_m3: 1e-6,
                mass_relative: 1e-9,
                minimum_step_s: 1e-6,
                maximum_step_s: self.maximum_step_s,
                iteration_limit: 32,
                retry_limit: 16,
            },
        };
        let initial = CoupledLevelPoolInitialState {
            time_s: 0.0,
            states: initial_states,
        };

        let mut engine = CoupledPoolsEngine::new(setup, initial)?;
        let token = CancellationToken::new();
        let (outcome, steps_taken) =
            engine.advance_to(self.target_time_s, StepBudget::new(self.max_steps), &token)?;
        let final_pools = engine.pool_states()?;
        let final_total_volume_m3: f64 = final_pools.iter().map(|p| p.volume_m3).sum();
        let last_transfers = engine.last_transfers()?;

        Ok(PoolStructureRunReport {
            run_id: self.run_id.clone(),
            classification: self.classification,
            outcome,
            accepted_time_s: engine.time_s(),
            steps_taken,
            initial_total_volume_m3,
            final_total_volume_m3,
            total_volume_residual_m3: final_total_volume_m3 - initial_total_volume_m3,
            pools: final_pools,
            last_transfers,
        })
    }
}
