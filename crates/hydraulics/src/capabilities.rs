//! Mapping and admission checks against `hydr` component capability declarations.

use arus_model_api::{
    BackendCapabilities, CapabilityLimitSummary, ModelError, Result, RuntimeSupportSummary,
};
use hydr::capability::{
    BoundaryKind, ComponentCapabilities, EndpointId, Precision, RuntimeKind, RuntimeStatus,
    SourceKind,
};
use hydr::channel::{
    rectangular_reach_capabilities, surveyed_reach_capabilities, triangular_reach_capabilities,
};
use hydr::coupling::{
    channel_pool_capabilities, coupled_level_pool_capabilities, runoff_pool_capabilities,
    surface_channel_capabilities, surface_channel_drainage_capabilities,
    surface_drainage_capabilities,
};
use hydr::drainage::{native_pipe_drainage_capabilities, swmm_drainage_capabilities};
use hydr::runoff::{reservoir_capabilities, subcatchment_capabilities};
use hydr::soil::soil_capabilities;
use hydr::solver::{
    CONTRACT_VERSION, SNAPSHOT_SCHEMA, surface_solver_capabilities, surface_solver_f32_capabilities,
};
use hydr::storage::level_pool_capabilities;
use hydr::structures::{
    controlled_gate_capabilities, controlled_pump_capabilities, culvert_barrel_capabilities,
    culvert_capabilities, culvert_inlet_capabilities, culvert_outlet_capabilities,
    gate_actuator_capabilities, operating_control_capabilities,
};

/// Maps any `hydr` component capability declaration into an ARUS `BackendCapabilities` summary.
pub fn map_component_capabilities(caps: &ComponentCapabilities) -> BackendCapabilities {
    let mut boundaries = Vec::new();
    for endpoint in caps.endpoints {
        for kind in endpoint.admitted {
            let id = kind.id().to_owned();
            if !boundaries.contains(&id) {
                boundaries.push(id);
            }
        }
    }
    BackendCapabilities {
        backend: "hydr".to_owned(),
        component: caps.component.id().to_owned(),
        contract: caps.contract.unwrap_or("unversioned").to_owned(),
        snapshot_schema: caps.snapshot_schema.unwrap_or("none").to_owned(),
        precision: caps
            .precision
            .iter()
            .map(|precision| precision.id().to_owned())
            .collect(),
        runtimes: caps
            .runtimes
            .iter()
            .map(|support| RuntimeSupportSummary {
                runtime: support.runtime.id().to_owned(),
                status: support.status.id().to_owned(),
            })
            .collect(),
        boundaries,
        sources: caps
            .sources
            .iter()
            .map(|source| source.id().to_owned())
            .collect(),
        limits: caps
            .limits
            .iter()
            .map(|limit| CapabilityLimitSummary {
                id: limit.id.to_owned(),
                summary: limit.summary.to_owned(),
            })
            .collect(),
    }
}

/// Returns the ARUS capability summary for `hydr`'s 2D shallow-water `F64` surface solver.
pub fn surface_capabilities() -> BackendCapabilities {
    map_component_capabilities(surface_solver_capabilities())
}

/// Returns the complete capability catalog across all 25 `hydr 0.2.0` components.
pub fn all_component_capabilities() -> Vec<BackendCapabilities> {
    [
        surface_solver_capabilities(),
        surface_solver_f32_capabilities(),
        soil_capabilities(),
        rectangular_reach_capabilities(),
        triangular_reach_capabilities(),
        surveyed_reach_capabilities(),
        level_pool_capabilities(),
        subcatchment_capabilities(),
        reservoir_capabilities(),
        operating_control_capabilities(),
        gate_actuator_capabilities(),
        controlled_gate_capabilities(),
        controlled_pump_capabilities(),
        culvert_barrel_capabilities(),
        culvert_inlet_capabilities(),
        culvert_outlet_capabilities(),
        culvert_capabilities(),
        coupled_level_pool_capabilities(),
        channel_pool_capabilities(),
        runoff_pool_capabilities(),
        surface_channel_capabilities(),
        swmm_drainage_capabilities(),
        native_pipe_drainage_capabilities(),
        surface_drainage_capabilities(),
        surface_channel_drainage_capabilities(),
    ]
    .into_iter()
    .map(map_component_capabilities)
    .collect()
}

/// Refuses execution unless `hydr`'s surface solver admits every required boundary, source,
/// `F64` precision, CPU reference status, and matching contract/schema identities.
pub fn check_surface_admission(
    required_boundaries: &[BoundaryKind],
    required_sources: &[SourceKind],
) -> Result<BackendCapabilities> {
    let caps = surface_solver_capabilities();
    for &kind in required_boundaries {
        if !caps.admits(EndpointId::ExteriorFace, kind) {
            return Err(ModelError::CapabilityRefused(format!(
                "boundary {} refused at {}: remove it from the run definition",
                kind.id(),
                EndpointId::ExteriorFace.id()
            )));
        }
    }
    for &source in required_sources {
        if !caps.sources.contains(&source) {
            return Err(ModelError::CapabilityRefused(format!(
                "source {} refused: remove it from the forcing definition",
                source.id()
            )));
        }
    }
    if !caps.precision.contains(&Precision::F64) {
        return Err(ModelError::CapabilityRefused(
            "precision f64 refused: select an F64 build".to_owned(),
        ));
    }
    let cpu_reference = caps
        .runtime(RuntimeKind::Cpu)
        .is_some_and(|support| support.status == RuntimeStatus::Reference);
    if !cpu_reference {
        return Err(ModelError::CapabilityRefused(
            "runtime cpu is not the reference: select the CPU reference runtime".to_owned(),
        ));
    }
    if caps.contract != Some(CONTRACT_VERSION) {
        return Err(ModelError::CapabilityRefused(
            "surface contract mismatch: rebuild arus-hydraulics against this hydr build".to_owned(),
        ));
    }
    if caps.snapshot_schema != Some(SNAPSHOT_SCHEMA) {
        return Err(ModelError::CapabilityRefused(
            "surface snapshot schema mismatch: rebuild arus-hydraulics against this hydr build"
                .to_owned(),
        ));
    }
    Ok(surface_capabilities())
}
