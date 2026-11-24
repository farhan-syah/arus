//! Pure-Rust 1D dynamic-wave Preissmann-slot pressurized-pipe drainage adapter over `hydr::drainage::NativePipeNetwork`.

use arus_domain::RunClassification;
use arus_model_api::{ModelError, Result};
use hydr::drainage::{
    NativePipeConfig, NativePipeNetwork, NativePipeNetworkSpec, NativePipeState, PipeConduitSpec,
    PipeCrossSection, PipeNodeKind, PipeNodeSpec, PipeOutfallBoundary,
    native_pipe_drainage_capabilities,
};
use serde::{Deserialize, Serialize};

/// JSON-serializable closed-conduit cross-section.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "kebab-case")]
pub enum PipeSectionSpec {
    Circular { diameter_m: f64 },
    Box { width_m: f64, height_m: f64 },
}

impl PipeSectionSpec {
    fn to_hydr(self) -> PipeCrossSection {
        match self {
            Self::Circular { diameter_m } => PipeCrossSection::Circular { diameter_m },
            Self::Box { width_m, height_m } => PipeCrossSection::Box { width_m, height_m },
        }
    }
}

/// JSON-serializable outfall hydraulic boundary condition.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PipeOutfallSpec {
    Free,
    FixedStage { stage_m: f64 },
}

/// JSON-serializable node kind for a native pressurized-pipe network.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PipeNodeKindSpec {
    Junction {
        max_depth_m: f64,
        surcharge_depth_m: f64,
        shaft_area_m2: f64,
        ponding_area_m2: f64,
    },
    Storage {
        max_depth_m: f64,
        area_m2: f64,
    },
    Outfall {
        boundary: PipeOutfallSpec,
    },
}

/// JSON-serializable node specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipeNodeRunSpec {
    pub id: String,
    pub invert_m: f64,
    pub initial_depth_m: f64,
    pub node: PipeNodeKindSpec,
}

/// JSON-serializable conduit specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipeConduitRunSpec {
    pub id: String,
    pub from_node: String,
    pub to_node: String,
    pub length_m: f64,
    pub section: PipeSectionSpec,
    pub manning_n: f64,
    #[serde(default)]
    pub in_offset_m: f64,
    #[serde(default)]
    pub out_offset_m: f64,
    #[serde(default)]
    pub initial_flow_m3_per_s: f64,
}

/// Constant lateral inflow applied to one named node during an interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipeLateralInflowSpec {
    pub node_id: String,
    pub rate_m3_per_s: f64,
}

/// JSON-serializable manifest for a pure-Rust `NativePipeNetwork` run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NativePipeRunSpec {
    pub run_id: String,
    pub classification: RunClassification,
    pub vertical_datum: String,
    pub nodes: Vec<PipeNodeRunSpec>,
    pub conduits: Vec<PipeConduitRunSpec>,
    #[serde(default)]
    pub lateral_inflows: Vec<PipeLateralInflowSpec>,
    pub duration_s: f64,
    pub report_step_s: f64,
}

/// Final node hydraulic summary from a `NativePipeNetwork` run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipeNodeSummary {
    pub id: String,
    pub depth_m: f64,
    pub head_m: f64,
    pub surcharged: bool,
    pub flooding_m3_per_s: f64,
}

/// Final conduit hydraulic summary from a `NativePipeNetwork` run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipeConduitSummary {
    pub id: String,
    pub flow_m3_per_s: f64,
    pub depth_m: f64,
    pub pressurized: bool,
}

/// Summary report emitted by a pure-Rust `NativePipeNetwork` execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NativePipeRunReport {
    pub run_id: String,
    pub classification: RunClassification,
    pub contract: String,
    pub accepted_time_s: f64,
    pub initial_storage_m3: f64,
    pub final_storage_m3: f64,
    pub lateral_inflow_m3: f64,
    pub outfall_outflow_m3: f64,
    pub flooding_loss_m3: f64,
    pub residual_m3: f64,
    pub any_surcharged: bool,
    pub any_pressurized: bool,
    pub nodes: Vec<PipeNodeSummary>,
    pub conduits: Vec<PipeConduitSummary>,
}

/// Pure-Rust pressurized-pipe drainage network engine wrapping `hydr::drainage::NativePipeNetwork`.
pub struct NativePipeEngine {
    net: NativePipeNetwork,
}

impl NativePipeEngine {
    /// Builds and validates a `NativePipeEngine` from a JSON-serializable `NativePipeRunSpec`.
    pub fn from_spec(spec: &NativePipeRunSpec) -> Result<Self> {
        let nodes = spec
            .nodes
            .iter()
            .map(|n| PipeNodeSpec {
                id: n.id.clone(),
                invert_m: n.invert_m,
                initial_depth_m: n.initial_depth_m,
                kind: match n.node {
                    PipeNodeKindSpec::Junction {
                        max_depth_m,
                        surcharge_depth_m,
                        shaft_area_m2,
                        ponding_area_m2,
                    } => PipeNodeKind::Junction {
                        max_depth_m,
                        surcharge_depth_m,
                        shaft_area_m2,
                        ponding_area_m2,
                    },
                    PipeNodeKindSpec::Storage {
                        max_depth_m,
                        area_m2,
                    } => PipeNodeKind::Storage {
                        max_depth_m,
                        area_m2,
                    },
                    PipeNodeKindSpec::Outfall { boundary } => PipeNodeKind::Outfall {
                        boundary: match boundary {
                            PipeOutfallSpec::Free => PipeOutfallBoundary::Free,
                            PipeOutfallSpec::FixedStage { stage_m } => {
                                PipeOutfallBoundary::FixedStage(stage_m)
                            }
                        },
                    },
                },
            })
            .collect();
        let conduits = spec
            .conduits
            .iter()
            .map(|c| PipeConduitSpec {
                id: c.id.clone(),
                from_node: c.from_node.clone(),
                to_node: c.to_node.clone(),
                length_m: c.length_m,
                section: c.section.to_hydr(),
                manning_n: c.manning_n,
                in_offset_m: c.in_offset_m,
                out_offset_m: c.out_offset_m,
                initial_flow_m3_per_s: c.initial_flow_m3_per_s,
            })
            .collect();
        let net = NativePipeNetwork::new(NativePipeNetworkSpec {
            vertical_datum: spec.vertical_datum.clone(),
            nodes,
            conduits,
            config: NativePipeConfig::default(),
        })
        .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
        Ok(Self { net })
    }

    /// Returns the live SI state snapshot.
    pub fn state(&self) -> NativePipeState {
        self.net.state()
    }
}

impl NativePipeRunSpec {
    /// Checks inputs and runs the pure-Rust pressurized-pipe simulation to `duration_s`.
    pub fn execute(&self) -> Result<NativePipeRunReport> {
        if !self.duration_s.is_finite() || self.duration_s <= 0.0 {
            return Err(ModelError::InvalidInput(format!(
                "duration_s={} must be finite and > 0",
                self.duration_s
            )));
        }
        if !self.report_step_s.is_finite() || self.report_step_s <= 0.0 {
            return Err(ModelError::InvalidInput(format!(
                "report_step_s={} must be finite and > 0",
                self.report_step_s
            )));
        }
        let mut engine = NativePipeEngine::from_spec(self)?;
        let mut inflows = Vec::with_capacity(self.lateral_inflows.len());
        for item in &self.lateral_inflows {
            let idx = engine.net.node_index(&item.node_id).ok_or_else(|| {
                ModelError::InvalidInput(format!("unknown lateral inflow node `{}`", item.node_id))
            })?;
            inflows.push((idx, item.rate_m3_per_s));
        }

        let mut any_surcharged = false;
        let mut any_pressurized = false;
        let mut t = 0.0_f64;
        while t + 1e-9 < self.duration_s {
            let next_t = (t + self.report_step_s).min(self.duration_s);
            let trial = engine
                .net
                .trial_to(next_t, &inflows, &[])
                .map_err(|error| ModelError::Engine(error.to_string()))?;
            trial.accept();
            let snap = engine.net.state();
            if snap.nodes.iter().any(|n| n.surcharged) {
                any_surcharged = true;
            }
            if snap.conduits.iter().any(|c| c.pressurized) {
                any_pressurized = true;
            }
            t = next_t;
        }

        let final_state = engine.net.state();
        let caps = native_pipe_drainage_capabilities();
        Ok(NativePipeRunReport {
            run_id: self.run_id.clone(),
            classification: self.classification,
            contract: caps
                .contract
                .unwrap_or("hydr.native-pipe-drainage/1")
                .to_owned(),
            accepted_time_s: final_state.time_s,
            initial_storage_m3: final_state.ledger.initial_storage_m3,
            final_storage_m3: final_state.ledger.final_storage_m3,
            lateral_inflow_m3: final_state.ledger.lateral_inflow_m3,
            outfall_outflow_m3: final_state.ledger.outfall_outflow_m3,
            flooding_loss_m3: final_state.ledger.flooding_loss_m3,
            residual_m3: final_state.ledger.residual_m3,
            any_surcharged,
            any_pressurized,
            nodes: final_state
                .nodes
                .into_iter()
                .map(|n| PipeNodeSummary {
                    id: n.id,
                    depth_m: n.depth_m,
                    head_m: n.head_m,
                    surcharged: n.surcharged,
                    flooding_m3_per_s: n.flooding_m3_per_s,
                })
                .collect(),
            conduits: final_state
                .conduits
                .into_iter()
                .map(|c| PipeConduitSummary {
                    id: c.id,
                    flow_m3_per_s: c.flow_m3_per_s,
                    depth_m: c.depth_m,
                    pressurized: c.pressurized,
                })
                .collect(),
        })
    }
}
