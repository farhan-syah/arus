//! JSON-serializable subcatchment runoff manifest (`RunoffRunSpec`) driving `SubcatchmentEngine`.

use arus_domain::RunClassification;
use arus_model_api::{ModelError, Result};
use hydr::runoff::{
    SubcatchmentConfig, SubcatchmentDefinition, SubcatchmentGeometry, SubcatchmentInput,
    SubcatchmentParameters, SubcatchmentRouting, SubcatchmentState,
};
use hydr::soil::ModifiedGreenAmptParameters;
use serde::{Deserialize, Serialize};

use crate::runoff::{SubcatchmentEngine, SubcatchmentStepSummary};

/// Pervious infiltration parameters for a subcatchment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunoffInfiltrationSpec {
    pub suction_m: f64,
    pub conductivity_m_per_s: f64,
    pub maximum_deficit: f64,
}

/// Internal subarea routing mode inside a subcatchment.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RunoffRoutingSpec {
    DirectToOutlet,
    ImperviousToPervious { fraction: f64 },
    PerviousToImpervious { fraction: f64 },
}

impl RunoffRoutingSpec {
    fn to_hydr(self) -> SubcatchmentRouting {
        match self {
            Self::DirectToOutlet => SubcatchmentRouting::Direct,
            Self::ImperviousToPervious { fraction } => {
                SubcatchmentRouting::ImperviousToPervious { fraction }
            }
            Self::PerviousToImpervious { fraction } => {
                SubcatchmentRouting::PerviousToImpervious { fraction }
            }
        }
    }
}

/// Definition of one three-subarea urban/catchment runoff unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubcatchmentSpec {
    pub id: String,
    pub total_area_m2: f64,
    pub width_m: f64,
    pub slope: f64,
    pub impervious_fraction: f64,
    pub zero_storage_impervious_fraction: f64,
    pub impervious_manning: f64,
    pub pervious_manning: f64,
    pub impervious_depression_m: f64,
    pub pervious_depression_m: f64,
    pub infiltration: Option<RunoffInfiltrationSpec>,
    pub routing: RunoffRoutingSpec,
}

/// Uniform rainfall and evaporation forcing interval applied to all subcatchments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunoffIntervalSpec {
    pub end_time_s: f64,
    pub rain_rate_m_per_s: f64,
    pub potential_evaporation_m_per_s: f64,
}

/// Complete JSON-serializable subcatchment runoff simulation manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunoffRunSpec {
    pub run_id: String,
    pub classification: RunClassification,
    pub subcatchments: Vec<SubcatchmentSpec>,
    pub intervals: Vec<RunoffIntervalSpec>,
}

/// Aggregated water-balance report produced by executing a `RunoffRunSpec`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunoffRunReport {
    pub run_id: String,
    pub classification: RunClassification,
    pub final_time_s: f64,
    pub total_rain_m3: f64,
    pub total_infiltration_m3: f64,
    pub total_evaporation_m3: f64,
    pub total_outlet_m3: f64,
    pub max_abs_residual_m3: f64,
    pub steps: Vec<SubcatchmentStepSummary>,
}

impl RunoffRunSpec {
    /// Checks inputs, constructs a `SubcatchmentEngine`, and executes all forcing intervals.
    pub fn execute(&self) -> Result<RunoffRunReport> {
        if self.subcatchments.is_empty() {
            return Err(ModelError::InvalidInput(
                "runoff spec must declare at least one subcatchment".to_owned(),
            ));
        }
        let mut definitions = Vec::with_capacity(self.subcatchments.len());
        let mut states = Vec::with_capacity(self.subcatchments.len());
        for spec in &self.subcatchments {
            let infiltration = match &spec.infiltration {
                Some(inf) => Some(
                    ModifiedGreenAmptParameters::from_si(
                        inf.suction_m,
                        inf.conductivity_m_per_s,
                        inf.maximum_deficit,
                    )
                    .map_err(|error| ModelError::InvalidInput(error.to_string()))?,
                ),
                None => None,
            };
            let params = SubcatchmentParameters::new(
                SubcatchmentGeometry {
                    total_area_m2: spec.total_area_m2,
                    width_m: spec.width_m,
                    slope: spec.slope,
                    impervious_fraction: spec.impervious_fraction,
                    zero_storage_impervious_fraction: spec.zero_storage_impervious_fraction,
                    impervious_manning: spec.impervious_manning,
                    pervious_manning: spec.pervious_manning,
                    impervious_depression_m: spec.impervious_depression_m,
                    pervious_depression_m: spec.pervious_depression_m,
                },
                infiltration,
                spec.routing.to_hydr(),
            )
            .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
            let state = SubcatchmentState::initialized(&params)
                .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
            definitions.push(SubcatchmentDefinition {
                id: spec.id.clone(),
                parameters: params,
            });
            states.push(state);
        }

        let mut engine =
            SubcatchmentEngine::new(definitions, states, 0.0, SubcatchmentConfig::default())?;
        let count = self.subcatchments.len();
        let mut steps = Vec::with_capacity(self.intervals.len());
        let mut total_rain_m3 = 0.0;
        let mut total_infiltration_m3 = 0.0;
        let mut total_evaporation_m3 = 0.0;
        let mut total_outlet_m3 = 0.0;
        let mut max_abs_residual_m3 = 0.0_f64;

        for interval in &self.intervals {
            let inputs = vec![
                SubcatchmentInput {
                    rain_rate_m_per_s: interval.rain_rate_m_per_s,
                    potential_evaporation_m_per_s: interval.potential_evaporation_m_per_s,
                    external_runon_volumes_m3: [0.0; 3],
                };
                count
            ];
            let step = engine.step_to(interval.end_time_s, &inputs)?;
            total_rain_m3 += step.rain_m3;
            total_infiltration_m3 += step.infiltration_m3;
            total_evaporation_m3 += step.evaporation_m3;
            total_outlet_m3 += step.outlet_m3;
            max_abs_residual_m3 = max_abs_residual_m3.max(step.residual_m3.abs());
            steps.push(step);
        }

        Ok(RunoffRunReport {
            run_id: self.run_id.clone(),
            classification: self.classification,
            final_time_s: engine.time_s(),
            total_rain_m3,
            total_infiltration_m3,
            total_evaporation_m3,
            total_outlet_m3,
            max_abs_residual_m3,
            steps,
        })
    }
}
