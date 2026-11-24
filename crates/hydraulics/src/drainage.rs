//! Piped urban drainage adapter over `hydr::drainage` (pinned EPA SWMM v5.2.4 worker).
//!
//! Validates SWMM `.inp` manifests, surcharge methods (`extran` / `slot`), and control-authority
//! declarations offline, and opens the isolated SWMM worker process when the host binary exists.

use std::path::PathBuf;

use arus_model_api::{ModelError, Result};
use hydr::drainage::{
    SwmmControlAuthority, SwmmDrainage, SwmmDrainageDefinition, SwmmNetworkSummary,
    SwmmSurchargeMethod, swmm_drainage_capabilities,
};
use serde::{Deserialize, Serialize};

/// Pressurized conduit surcharge formulation for piped drainage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DrainageSurchargeSpec {
    Extran,
    Slot,
}

impl DrainageSurchargeSpec {
    fn to_hydr(self) -> SwmmSurchargeMethod {
        match self {
            Self::Extran => SwmmSurchargeMethod::Extran,
            Self::Slot => SwmmSurchargeMethod::Slot,
        }
    }
}

/// Control-authority mode for structures inside a SWMM drainage network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DrainageControlSpec {
    NativeSwmm,
    HydrCommandsNativeActuation,
    HydrActuatorImposedPosition,
    PrescribedHydrTrajectory,
}

impl DrainageControlSpec {
    fn to_hydr(self) -> SwmmControlAuthority {
        match self {
            Self::NativeSwmm => SwmmControlAuthority::NativeSwmm,
            Self::HydrCommandsNativeActuation => SwmmControlAuthority::HydrCommandsNativeActuation,
            Self::HydrActuatorImposedPosition => SwmmControlAuthority::HydrActuatorImposedPosition,
            Self::PrescribedHydrTrajectory => SwmmControlAuthority::PrescribedHydrTrajectory,
        }
    }
}

/// JSON-serializable manifest describing a pinned EPA SWMM v5.2.4 drainage network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwmmDrainageSpec {
    pub inp_content: String,
    pub vertical_datum: String,
    pub surcharge_method: DrainageSurchargeSpec,
    pub control_authority: DrainageControlSpec,
    pub worker_binary: PathBuf,
    pub provenance: String,
    pub surcharge_evidence: String,
}

/// Offline validation report for a `SwmmDrainageSpec` and host SWMM worker readiness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwmmDrainageCheckReport {
    pub contract: String,
    pub inp_sha256: String,
    pub vertical_datum: String,
    pub surcharge_method: String,
    pub control_authority: String,
    pub worker_binary: String,
    pub worker_binary_exists: bool,
}

impl SwmmDrainageSpec {
    /// Checks manifest fields via `SwmmDrainageDefinition::new` without launching a worker.
    pub fn into_definition(&self) -> Result<SwmmDrainageDefinition> {
        SwmmDrainageDefinition::new(
            self.inp_content.clone(),
            self.vertical_datum.clone(),
            self.surcharge_method.to_hydr(),
            self.control_authority.to_hydr(),
            self.worker_binary.clone(),
            self.provenance.clone(),
            self.surcharge_evidence.clone(),
        )
        .map_err(|error| ModelError::InvalidInput(error.to_string()))
    }

    /// Validates the definition and reports whether the configured SWMM worker binary is present.
    pub fn check(&self) -> Result<SwmmDrainageCheckReport> {
        let def = self.into_definition()?;
        let caps = swmm_drainage_capabilities();
        Ok(SwmmDrainageCheckReport {
            contract: caps
                .contract
                .unwrap_or("hydr-swmm-drainage-v5.2.4-1")
                .to_owned(),
            inp_sha256: def.inp_sha256().to_owned(),
            vertical_datum: def.vertical_datum().to_owned(),
            surcharge_method: def.surcharge_method().id().to_owned(),
            control_authority: def.control_authority().id().to_owned(),
            worker_binary: def.worker_binary().display().to_string(),
            worker_binary_exists: def.worker_binary().is_file(),
        })
    }
}

/// Live piped urban drainage engine wrapping `hydr::drainage::SwmmDrainage`.
pub struct SwmmDrainageEngine {
    inner: SwmmDrainage,
}

impl SwmmDrainageEngine {
    /// Validates `spec` and spawns the isolated pinned EPA SWMM v5.2.4 worker process.
    pub fn open(spec: &SwmmDrainageSpec) -> Result<Self> {
        let def = spec.into_definition()?;
        if !def.worker_binary().is_file() {
            return Err(ModelError::InvalidInput(format!(
                "SWMM worker binary {} does not exist; build the pinned EPA SWMM v5.2.4 worker first",
                def.worker_binary().display()
            )));
        }
        let inner =
            SwmmDrainage::open(def).map_err(|error| ModelError::Engine(error.to_string()))?;
        Ok(Self { inner })
    }

    /// Returns the SWMM network object counts and options reported at open.
    pub fn summary(&self) -> SwmmNetworkSummary {
        self.inner.summary()
    }

    /// Returns accepted simulation time in seconds.
    pub fn time_s(&self) -> f64 {
        self.inner.time_s()
    }
}
