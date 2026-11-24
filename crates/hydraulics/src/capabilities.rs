//! Mapping and admission checks against `hydr::solver::surface_solver_capabilities()`.

use arus_model_api::{
    BackendCapabilities, CapabilityLimitSummary, ModelError, Result, RuntimeSupportSummary,
};
use hydr::capability::{
    BoundaryKind, EndpointId, Precision, RuntimeKind, RuntimeStatus, SourceKind,
};
use hydr::solver::{CONTRACT_VERSION, SNAPSHOT_SCHEMA, surface_solver_capabilities};

/// Returns the ARUS capability summary for `hydr`'s 2D shallow-water `F64` surface solver.
pub fn surface_capabilities() -> BackendCapabilities {
    let caps = surface_solver_capabilities();
    let boundaries = caps
        .endpoints
        .iter()
        .filter(|endpoint| endpoint.id == EndpointId::ExteriorFace)
        .flat_map(|endpoint| endpoint.admitted.iter().map(|kind| kind.id().to_owned()))
        .collect();
    BackendCapabilities {
        backend: "hydr".to_owned(),
        component: caps.component.id().to_owned(),
        contract: caps.contract.unwrap_or(CONTRACT_VERSION).to_owned(),
        snapshot_schema: caps.snapshot_schema.unwrap_or(SNAPSHOT_SCHEMA).to_owned(),
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
