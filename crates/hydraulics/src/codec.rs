//! Exact binary codec for `hydr::solver::SnapshotData`.
//!
//! Preserves `schema` and `contract` verbatim first, dispatches on `schema`, and stores
//! every `f64` as little-endian IEEE-754 bits so `-0.0` and `+inf` round-trip bit-for-bit.

use arus_model_api::{CheckpointHeader, CheckpointPackage, ModelError, Result};
use hydr::grid::CartesianGrid;
use hydr::model::{CellState, Domain, FlowState, SpatialReference};
use hydr::soil::{
    LandCell, LandModel, ModifiedGreenAmptParameters, NativeSoilState, SoilCellState,
    SoilSourceReceipt, SoilState,
};
use hydr::solver::{
    Axis, BoundaryConditions, BoundaryFace, BoundaryForcing, BoundaryTolerances,
    BoundaryTraceDiagnostics, DischargeBoundary, ExteriorBoundary, ExteriorState, FaceTransfer,
    FluxPort, ForcingInterval, ForcingPrefix, ForcingSchedule, InitialState, SNAPSHOT_SCHEMA,
    SnapshotData, SolverConfig, SolverSetup, SourceReceipt, StageBoundary, StepDiagnostics,
    TransmissiveDiagnostics, WaterLedger,
};

const MAGIC: &[u8; 8] = b"ARUSHYDR";
const CODEC_VERSION: u32 = 1;

/// Encodes `data` into an exact binary `CheckpointPackage`.
pub fn encode_snapshot(data: &SnapshotData) -> Result<CheckpointPackage> {
    let mut writer = Writer::default();
    writer.bytes(MAGIC);
    writer.u32(CODEC_VERSION);
    writer.string(&data.schema)?;
    writer.string(&data.contract)?;
    encode_setup(&mut writer, &data.setup)?;
    encode_forcing_prefix(&mut writer, &data.forcing_prefix)?;
    encode_initial_state(&mut writer, &data.accepted)?;
    encode_ledger(&mut writer, &data.ledger);
    writer.usize(data.accepted_steps);
    match &data.last_step {
        None => writer.u8(0),
        Some(step) => {
            writer.u8(1);
            encode_step_diagnostics(&mut writer, step)?;
        }
    }

    let header = CheckpointHeader {
        schema: data.schema.clone(),
        contract: data.contract.clone(),
        accepted_time_s: data.accepted.time,
        accepted_steps: data.accepted_steps,
        forcing_prefix_intervals: data.forcing_prefix.intervals,
        forcing_prefix_digest_hex: hex_digest(&data.forcing_prefix.digest),
    };
    Ok(CheckpointPackage {
        header,
        payload: writer.buf,
    })
}

/// Decodes an exact binary `CheckpointPackage` into `SnapshotData`.
pub fn decode_snapshot(package: &CheckpointPackage) -> Result<SnapshotData> {
    let data = decode_snapshot_bytes(&package.payload)?;
    if data.schema != package.header.schema || data.contract != package.header.contract {
        return Err(ModelError::Checkpoint(format!(
            "checkpoint header mismatch: header=({}, {}), payload=({}, {})",
            package.header.schema, package.header.contract, data.schema, data.contract
        )));
    }
    Ok(data)
}

/// Reconstructs a `CheckpointPackage` (header + payload) directly from raw archived bytes.
pub fn package_from_bytes(payload: Vec<u8>) -> Result<CheckpointPackage> {
    let data = decode_snapshot_bytes(&payload)?;
    let header = CheckpointHeader {
        schema: data.schema,
        contract: data.contract,
        accepted_time_s: data.accepted.time,
        accepted_steps: data.accepted_steps,
        forcing_prefix_intervals: data.forcing_prefix.intervals,
        forcing_prefix_digest_hex: hex_digest(&data.forcing_prefix.digest),
    };
    Ok(CheckpointPackage { header, payload })
}

fn decode_snapshot_bytes(payload: &[u8]) -> Result<SnapshotData> {
    let mut reader = Reader::new(payload);
    let magic = reader.fixed::<8>()?;
    if &magic != MAGIC {
        return Err(ModelError::Checkpoint(
            "invalid checkpoint magic header".to_owned(),
        ));
    }
    let version = reader.u32()?;
    if version != CODEC_VERSION {
        return Err(ModelError::Checkpoint(format!(
            "unsupported checkpoint codec version {version}: expected {CODEC_VERSION}"
        )));
    }
    let schema = reader.string()?;
    let contract = reader.string()?;
    if schema != SNAPSHOT_SCHEMA {
        return Err(ModelError::Checkpoint(format!(
            "unsupported checkpoint schema {schema}: expected {SNAPSHOT_SCHEMA}"
        )));
    }

    let setup = decode_setup(&mut reader)?;
    let forcing_prefix = decode_forcing_prefix(&mut reader)?;
    let accepted = decode_initial_state(&mut reader, &setup.domain, &setup.land)?;
    let ledger = decode_ledger(&mut reader)?;
    let accepted_steps = reader.usize()?;
    let last_step = match reader.u8()? {
        0 => None,
        1 => Some(decode_step_diagnostics(&mut reader)?),
        tag => {
            return Err(ModelError::Checkpoint(format!(
                "invalid last_step tag {tag}"
            )));
        }
    };
    reader.finish()?;

    Ok(SnapshotData {
        schema,
        contract,
        setup,
        forcing_prefix,
        accepted,
        ledger,
        accepted_steps,
        last_step,
    })
}

fn hex_digest(digest: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn encode_setup(w: &mut Writer, setup: &SolverSetup) -> Result<()> {
    let grid = setup.domain.grid();
    let (nx, ny) = grid.dimensions();
    let (dx, dy) = grid.spacing();
    w.usize(nx);
    w.usize(ny);
    w.f64(dx);
    w.f64(dy);
    w.f64_slice(setup.domain.bed());
    w.f64_slice(setup.domain.manning());
    w.bool_slice(setup.domain.active());
    let reference = setup.domain.reference();
    w.string(reference.projected_crs())?;
    w.string(reference.vertical_datum())?;
    w.f64(reference.origin()[0]);
    w.f64(reference.origin()[1]);

    w.usize(setup.land.cells().len());
    for cell in setup.land.cells() {
        match cell {
            LandCell::Impervious => w.u8(0),
            LandCell::ModifiedGreenAmpt(params) => {
                w.u8(1);
                w.f64(params.suction_ft());
                w.f64(params.conductivity_ft_per_s());
                w.f64(params.maximum_deficit());
            }
        }
    }

    w.usize(setup.boundaries.open_faces().len());
    for face in setup.boundaries.open_faces() {
        encode_face(w, *face);
    }

    w.usize(setup.forcing.intervals().len());
    for interval in setup.forcing.intervals() {
        encode_interval(w, interval)?;
    }

    encode_config(w, &setup.config);
    Ok(())
}

fn decode_setup(r: &mut Reader<'_>) -> Result<SolverSetup> {
    let nx = r.usize()?;
    let ny = r.usize()?;
    let dx = r.f64()?;
    let dy = r.f64()?;
    let grid = CartesianGrid::new(nx, ny, dx, dy)
        .map_err(|error| ModelError::Checkpoint(format!("grid decode failed: {error}")))?;
    let bed = r.f64_vec()?;
    let manning = r.f64_vec()?;
    let active = r.bool_vec()?;
    let crs = r.string()?;
    let datum = r.string()?;
    let origin = [r.f64()?, r.f64()?];
    let reference = SpatialReference::new(crs, datum, origin)
        .map_err(|error| ModelError::Checkpoint(format!("reference decode failed: {error}")))?;
    let domain = Domain::new(grid, bed, manning, active, reference)
        .map_err(|error| ModelError::Checkpoint(format!("domain decode failed: {error}")))?;

    let land_len = r.bounded_count(1)?;
    let mut land_cells = Vec::with_capacity(land_len);
    for _ in 0..land_len {
        match r.u8()? {
            0 => land_cells.push(LandCell::Impervious),
            1 => {
                let suction = r.f64()?;
                let conductivity = r.f64()?;
                let deficit = r.f64()?;
                let params =
                    ModifiedGreenAmptParameters::from_native(suction, conductivity, deficit)
                        .map_err(|error| {
                            ModelError::Checkpoint(format!("soil parameter decode failed: {error}"))
                        })?;
                land_cells.push(LandCell::ModifiedGreenAmpt(params));
            }
            tag => {
                return Err(ModelError::Checkpoint(format!(
                    "invalid land cell tag {tag}"
                )));
            }
        }
    }
    let land = LandModel::new(&domain, land_cells)
        .map_err(|error| ModelError::Checkpoint(format!("land model decode failed: {error}")))?;

    let open_len = r.bounded_count(17)?;
    let mut open_faces = Vec::with_capacity(open_len);
    for _ in 0..open_len {
        open_faces.push(decode_face(r)?);
    }
    let boundaries = if open_faces.is_empty() {
        BoundaryConditions::impermeable_walls()
    } else {
        BoundaryConditions::with_open_faces(&domain, open_faces)
            .map_err(|error| ModelError::Checkpoint(format!("boundaries decode failed: {error}")))?
    };

    let interval_len = r.bounded_count(48)?;
    let mut intervals = Vec::with_capacity(interval_len);
    for _ in 0..interval_len {
        intervals.push(decode_interval(r)?);
    }
    let forcing = ForcingSchedule::new(&domain, &boundaries, intervals)
        .map_err(|error| ModelError::Checkpoint(format!("forcing decode failed: {error}")))?;
    let config = decode_config(r)?;

    Ok(SolverSetup {
        domain,
        land,
        boundaries,
        forcing,
        config,
    })
}

fn encode_interval(w: &mut Writer, interval: &ForcingInterval) -> Result<()> {
    w.f64(interval.start);
    w.f64(interval.end);
    w.f64_slice(&interval.rain);
    w.f64_slice(&interval.potential_evaporation);
    w.f64_slice(&interval.withdrawal);
    w.usize(interval.boundaries.len());
    for boundary in &interval.boundaries {
        match boundary {
            BoundaryForcing::Flux(port) => {
                w.u8(0);
                encode_face(w, port.face);
                w.f64(port.water_rate);
                w.f64(port.momentum_rate[0]);
                w.f64(port.momentum_rate[1]);
                w.f64(port.signal_speed);
                w.string(&port.provenance)?;
            }
            BoundaryForcing::Stage(stage) => {
                w.u8(1);
                encode_face(w, stage.face);
                w.f64(stage.elevation);
                w.opt_f64(stage.inflow_tangential_velocity);
            }
            BoundaryForcing::Discharge(discharge) => {
                w.u8(2);
                encode_face(w, discharge.face);
                w.f64(discharge.outward_rate);
                w.opt_f64(discharge.inflow_tangential_velocity);
            }
            BoundaryForcing::Exterior(exterior) => {
                w.u8(3);
                encode_face(w, exterior.face);
                match exterior.state {
                    ExteriorState::Dry => w.u8(0),
                    ExteriorState::Wet {
                        elevation,
                        outward_velocity,
                        tangential_velocity,
                    } => {
                        w.u8(1);
                        w.f64(elevation);
                        w.f64(outward_velocity);
                        w.f64(tangential_velocity);
                    }
                }
            }
            BoundaryForcing::Transmissive(face) => {
                w.u8(4);
                encode_face(w, *face);
            }
        }
    }
    Ok(())
}

fn decode_interval(r: &mut Reader<'_>) -> Result<ForcingInterval> {
    let start = r.f64()?;
    let end = r.f64()?;
    let rain = r.f64_vec()?;
    let potential_evaporation = r.f64_vec()?;
    let withdrawal = r.f64_vec()?;
    let boundary_len = r.bounded_count(18)?;
    let mut boundaries = Vec::with_capacity(boundary_len);
    for _ in 0..boundary_len {
        let boundary = match r.u8()? {
            0 => BoundaryForcing::Flux(FluxPort {
                face: decode_face(r)?,
                water_rate: r.f64()?,
                momentum_rate: [r.f64()?, r.f64()?],
                signal_speed: r.f64()?,
                provenance: r.string()?,
            }),
            1 => BoundaryForcing::Stage(StageBoundary {
                face: decode_face(r)?,
                elevation: r.f64()?,
                inflow_tangential_velocity: r.opt_f64()?,
            }),
            2 => BoundaryForcing::Discharge(DischargeBoundary {
                face: decode_face(r)?,
                outward_rate: r.f64()?,
                inflow_tangential_velocity: r.opt_f64()?,
            }),
            3 => {
                let face = decode_face(r)?;
                let state = match r.u8()? {
                    0 => ExteriorState::Dry,
                    1 => ExteriorState::Wet {
                        elevation: r.f64()?,
                        outward_velocity: r.f64()?,
                        tangential_velocity: r.f64()?,
                    },
                    tag => {
                        return Err(ModelError::Checkpoint(format!(
                            "invalid exterior state tag {tag}"
                        )));
                    }
                };
                BoundaryForcing::Exterior(ExteriorBoundary { face, state })
            }
            4 => BoundaryForcing::Transmissive(decode_face(r)?),
            tag => {
                return Err(ModelError::Checkpoint(format!(
                    "invalid boundary forcing tag {tag}"
                )));
            }
        };
        boundaries.push(boundary);
    }
    Ok(ForcingInterval {
        start,
        end,
        rain,
        potential_evaporation,
        withdrawal,
        boundaries,
    })
}

fn encode_config(w: &mut Writer, config: &SolverConfig) {
    w.f64(config.gravity);
    w.f64(config.cfl);
    w.f64(config.max_step);
    w.f64(config.min_step);
    w.usize(config.max_retries);
    w.f64(config.root_absolute_depth);
    w.f64(config.root_relative_depth);
    w.usize(config.root_iterations);
    w.f64(config.volume_absolute);
    w.f64(config.volume_relative);
    w.f64(config.momentum_film_depth);
    w.f64(config.max_cumulative_water_adjustment_m3);
    w.f64(config.boundary.discharge_absolute);
    w.f64(config.boundary.discharge_relative);
    w.f64(config.boundary.characteristic_relative);
    w.f64(config.boundary.critical_relative);
}

fn decode_config(r: &mut Reader<'_>) -> Result<SolverConfig> {
    Ok(SolverConfig {
        gravity: r.f64()?,
        cfl: r.f64()?,
        max_step: r.f64()?,
        min_step: r.f64()?,
        max_retries: r.usize()?,
        root_absolute_depth: r.f64()?,
        root_relative_depth: r.f64()?,
        root_iterations: r.usize()?,
        volume_absolute: r.f64()?,
        volume_relative: r.f64()?,
        momentum_film_depth: r.f64()?,
        max_cumulative_water_adjustment_m3: r.f64()?,
        boundary: BoundaryTolerances {
            discharge_absolute: r.f64()?,
            discharge_relative: r.f64()?,
            characteristic_relative: r.f64()?,
            critical_relative: r.f64()?,
        },
    })
}

fn encode_forcing_prefix(w: &mut Writer, prefix: &ForcingPrefix) -> Result<()> {
    w.usize(prefix.intervals);
    w.f64(prefix.start);
    w.f64(prefix.end);
    w.bytes(&prefix.digest);
    Ok(())
}

fn decode_forcing_prefix(r: &mut Reader<'_>) -> Result<ForcingPrefix> {
    Ok(ForcingPrefix {
        intervals: r.usize()?,
        start: r.f64()?,
        end: r.f64()?,
        digest: r.fixed::<32>()?,
    })
}

fn encode_initial_state(w: &mut Writer, state: &InitialState) -> Result<()> {
    w.f64(state.time);
    w.usize(state.flow.cells().len());
    for cell in state.flow.cells() {
        w.f64(cell.h);
        w.f64(cell.qx);
        w.f64(cell.qy);
    }
    w.usize(state.soil.cells().len());
    for cell in state.soil.cells() {
        match cell {
            SoilCellState::Impervious => w.u8(0),
            SoilCellState::ModifiedGreenAmpt(native) => {
                w.u8(1);
                w.f64(native.imd);
                w.f64(native.f_ft);
                w.f64(native.fu_ft);
                w.bool(native.saturated);
                w.f64(native.t_s);
            }
        }
    }
    Ok(())
}

fn decode_initial_state(
    r: &mut Reader<'_>,
    domain: &Domain,
    land: &LandModel,
) -> Result<InitialState> {
    let time = r.f64()?;
    let flow_len = r.bounded_count(24)?;
    let mut flow_cells = Vec::with_capacity(flow_len);
    for _ in 0..flow_len {
        flow_cells.push(CellState {
            h: r.f64()?,
            qx: r.f64()?,
            qy: r.f64()?,
        });
    }
    let flow = FlowState::new(domain, flow_cells)
        .map_err(|error| ModelError::Checkpoint(format!("flow state decode failed: {error}")))?;

    let soil_len = r.bounded_count(1)?;
    let mut soil_cells = Vec::with_capacity(soil_len);
    for _ in 0..soil_len {
        match r.u8()? {
            0 => soil_cells.push(SoilCellState::Impervious),
            1 => soil_cells.push(SoilCellState::ModifiedGreenAmpt(NativeSoilState {
                imd: r.f64()?,
                f_ft: r.f64()?,
                fu_ft: r.f64()?,
                saturated: r.bool()?,
                t_s: r.f64()?,
            })),
            tag => {
                return Err(ModelError::Checkpoint(format!(
                    "invalid soil state tag {tag}"
                )));
            }
        }
    }
    let soil = SoilState::new(land, soil_cells)
        .map_err(|error| ModelError::Checkpoint(format!("soil state decode failed: {error}")))?;
    Ok(InitialState { time, flow, soil })
}

fn encode_ledger(w: &mut Writer, ledger: &WaterLedger) {
    w.f64(ledger.initial_storage);
    w.f64(ledger.storage);
    w.f64(ledger.rain);
    w.f64(ledger.withdrawal);
    w.f64(ledger.infiltration);
    w.f64(ledger.evaporation);
    w.f64(ledger.boundary_in);
    w.f64(ledger.boundary_out);
    w.f64(ledger.exchange_in_m3);
    w.f64(ledger.exchange_out_m3);
    w.f64(ledger.water_adjustment);
    w.f64(ledger.water_adjustment_abs_bound);
    w.f64(ledger.momentum_removed[0]);
    w.f64(ledger.momentum_removed[1]);
    w.f64(ledger.residual);
}

fn decode_ledger(r: &mut Reader<'_>) -> Result<WaterLedger> {
    Ok(WaterLedger {
        initial_storage: r.f64()?,
        storage: r.f64()?,
        rain: r.f64()?,
        withdrawal: r.f64()?,
        infiltration: r.f64()?,
        evaporation: r.f64()?,
        boundary_in: r.f64()?,
        boundary_out: r.f64()?,
        exchange_in_m3: r.f64()?,
        exchange_out_m3: r.f64()?,
        water_adjustment: r.f64()?,
        water_adjustment_abs_bound: r.f64()?,
        momentum_removed: [r.f64()?, r.f64()?],
        residual: r.f64()?,
    })
}

fn encode_step_diagnostics(w: &mut Writer, step: &StepDiagnostics) -> Result<()> {
    w.f64(step.start);
    w.f64(step.end);
    w.f64(step.dt);
    w.usize(step.retries);
    w.f64(step.cfl_limit);
    w.f64(step.depletion_limit);
    w.f64(step.storage_before);
    w.f64(step.storage_after);
    w.f64(step.rain_volume);
    w.f64(step.withdrawal_volume);
    w.f64(step.infiltration_volume);
    w.f64(step.evaporation_volume);
    w.f64(step.soil_water_adjustment_volume);
    w.f64(step.water_adjustment_volume);
    w.f64(step.water_adjustment_abs_charge);
    w.f64(step.boundary_in);
    w.f64(step.boundary_out);
    w.f64(step.exchange_in_volume);
    w.f64(step.exchange_out_volume);
    w.f64(step.exchange_representation_error);
    w.f64(step.residual);
    w.f64(step.normalized_residual);
    w.f64(step.max_local_residual);
    w.f64(step.momentum_removed[0]);
    w.f64(step.momentum_removed[1]);

    w.usize(step.faces.len());
    for face in &step.faces {
        encode_face(w, face.face);
        w.opt_usize(face.left);
        w.opt_usize(face.right);
        w.f64(face.flux[0]);
        w.f64(face.flux[1]);
        w.f64(face.flux[2]);
        w.f64(face.pressure_corrections[0]);
        w.f64(face.pressure_corrections[1]);
        w.f64(face.signal_speed);
        w.f64(face.volume);
    }

    w.usize(step.boundary_traces.len());
    for trace in &step.boundary_traces {
        encode_face(w, trace.face);
        w.f64(trace.depth);
        w.f64(trace.normal_velocity);
        w.f64(trace.tangential_velocity);
        w.f64(trace.outward_water_rate);
        w.f64(trace.discharge_residual_per_width);
        w.f64(trace.characteristic_velocity_residual);
        w.f64(trace.signal_speed);
    }

    w.usize(step.transmissive_states.len());
    for state in &step.transmissive_states {
        encode_face(w, state.face);
        w.f64(state.depth);
        w.f64(state.outward_discharge_per_width);
        w.f64(state.tangential_discharge_per_width);
    }

    w.usize(step.sources.len());
    for source in &step.sources {
        w.usize(source.cell);
        w.f64(source.ponded_depth_m);
        w.f64(source.rain_depth_m);
        w.f64(source.infiltration_depth_m);
        w.f64(source.evaporation_depth_m);
        w.f64(source.withdrawal_depth_m);
        w.f64(source.water_adjustment_depth_m);
        w.f64(source.exchange_inflow_depth_m);
        w.f64(source.exchange_withdrawal_depth_m);
        match &source.soil {
            None => w.u8(0),
            Some(soil) => {
                w.u8(1);
                w.f64(soil.native_rate_ft_per_s);
                w.f64(soil.native_available_depth_ft);
                w.f64(soil.native_infiltration_depth_ft);
                w.f64(soil.native_excess_depth_ft);
                w.f64(soil.native_roundoff_bound_ft);
                w.f64(soil.bridge_roundoff_bound_m);
                w.f64(soil.total_roundoff_bound_m);
            }
        }
    }
    Ok(())
}

fn decode_step_diagnostics(r: &mut Reader<'_>) -> Result<StepDiagnostics> {
    let start = r.f64()?;
    let end = r.f64()?;
    let dt = r.f64()?;
    let retries = r.usize()?;
    let cfl_limit = r.f64()?;
    let depletion_limit = r.f64()?;
    let storage_before = r.f64()?;
    let storage_after = r.f64()?;
    let rain_volume = r.f64()?;
    let withdrawal_volume = r.f64()?;
    let infiltration_volume = r.f64()?;
    let evaporation_volume = r.f64()?;
    let soil_water_adjustment_volume = r.f64()?;
    let water_adjustment_volume = r.f64()?;
    let water_adjustment_abs_charge = r.f64()?;
    let boundary_in = r.f64()?;
    let boundary_out = r.f64()?;
    let exchange_in_volume = r.f64()?;
    let exchange_out_volume = r.f64()?;
    let exchange_representation_error = r.f64()?;
    let residual = r.f64()?;
    let normalized_residual = r.f64()?;
    let max_local_residual = r.f64()?;
    let momentum_removed = [r.f64()?, r.f64()?];

    let face_len = r.bounded_count(67)?;
    let mut faces = Vec::with_capacity(face_len);
    for _ in 0..face_len {
        faces.push(FaceTransfer {
            face: decode_face(r)?,
            left: r.opt_usize()?,
            right: r.opt_usize()?,
            flux: [r.f64()?, r.f64()?, r.f64()?],
            pressure_corrections: [r.f64()?, r.f64()?],
            signal_speed: r.f64()?,
            volume: r.f64()?,
        });
    }

    let trace_len = r.bounded_count(73)?;
    let mut boundary_traces = Vec::with_capacity(trace_len);
    for _ in 0..trace_len {
        boundary_traces.push(BoundaryTraceDiagnostics {
            face: decode_face(r)?,
            depth: r.f64()?,
            normal_velocity: r.f64()?,
            tangential_velocity: r.f64()?,
            outward_water_rate: r.f64()?,
            discharge_residual_per_width: r.f64()?,
            characteristic_velocity_residual: r.f64()?,
            signal_speed: r.f64()?,
        });
    }

    let transmissive_len = r.bounded_count(41)?;
    let mut transmissive_states = Vec::with_capacity(transmissive_len);
    for _ in 0..transmissive_len {
        transmissive_states.push(TransmissiveDiagnostics {
            face: decode_face(r)?,
            depth: r.f64()?,
            outward_discharge_per_width: r.f64()?,
            tangential_discharge_per_width: r.f64()?,
        });
    }

    let source_len = r.bounded_count(73)?;
    let mut sources = Vec::with_capacity(source_len);
    for _ in 0..source_len {
        let cell = r.usize()?;
        let ponded_depth_m = r.f64()?;
        let rain_depth_m = r.f64()?;
        let infiltration_depth_m = r.f64()?;
        let evaporation_depth_m = r.f64()?;
        let withdrawal_depth_m = r.f64()?;
        let water_adjustment_depth_m = r.f64()?;
        let exchange_inflow_depth_m = r.f64()?;
        let exchange_withdrawal_depth_m = r.f64()?;
        let soil = match r.u8()? {
            0 => None,
            1 => Some(SoilSourceReceipt {
                native_rate_ft_per_s: r.f64()?,
                native_available_depth_ft: r.f64()?,
                native_infiltration_depth_ft: r.f64()?,
                native_excess_depth_ft: r.f64()?,
                native_roundoff_bound_ft: r.f64()?,
                bridge_roundoff_bound_m: r.f64()?,
                total_roundoff_bound_m: r.f64()?,
            }),
            tag => {
                return Err(ModelError::Checkpoint(format!(
                    "invalid soil source receipt tag {tag}"
                )));
            }
        };
        sources.push(SourceReceipt {
            cell,
            ponded_depth_m,
            rain_depth_m,
            infiltration_depth_m,
            evaporation_depth_m,
            withdrawal_depth_m,
            water_adjustment_depth_m,
            exchange_inflow_depth_m,
            exchange_withdrawal_depth_m,
            soil,
        });
    }

    Ok(StepDiagnostics {
        start,
        end,
        dt,
        retries,
        cfl_limit,
        depletion_limit,
        storage_before,
        storage_after,
        rain_volume,
        withdrawal_volume,
        infiltration_volume,
        evaporation_volume,
        soil_water_adjustment_volume,
        water_adjustment_volume,
        water_adjustment_abs_charge,
        boundary_in,
        boundary_out,
        exchange_in_volume,
        exchange_out_volume,
        exchange_representation_error,
        residual,
        normalized_residual,
        max_local_residual,
        momentum_removed,
        faces,
        boundary_traces,
        transmissive_states,
        sources,
    })
}

fn encode_face(w: &mut Writer, face: BoundaryFace) {
    w.u8(match face.axis {
        Axis::X => 0,
        Axis::Y => 1,
    });
    w.usize(face.column);
    w.usize(face.row);
}

fn decode_face(r: &mut Reader<'_>) -> Result<BoundaryFace> {
    let axis = match r.u8()? {
        0 => Axis::X,
        1 => Axis::Y,
        tag => return Err(ModelError::Checkpoint(format!("invalid axis tag {tag}"))),
    };
    Ok(BoundaryFace {
        axis,
        column: r.usize()?,
        row: r.usize()?,
    })
}

#[derive(Default)]
struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn bytes(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    fn u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn usize(&mut self, value: usize) {
        self.bytes(&(value as u64).to_le_bytes());
    }

    fn f64(&mut self, value: f64) {
        self.bytes(&value.to_bits().to_le_bytes());
    }

    fn opt_f64(&mut self, value: Option<f64>) {
        match value {
            None => self.u8(0),
            Some(v) => {
                self.u8(1);
                self.f64(v);
            }
        }
    }

    fn opt_usize(&mut self, value: Option<usize>) {
        match value {
            None => self.u8(0),
            Some(v) => {
                self.u8(1);
                self.usize(v);
            }
        }
    }

    fn string(&mut self, value: &str) -> Result<()> {
        let len = u32::try_from(value.len()).map_err(|_| {
            ModelError::Checkpoint(format!("string exceeds u32 length: {} bytes", value.len()))
        })?;
        self.u32(len);
        self.bytes(value.as_bytes());
        Ok(())
    }

    fn f64_slice(&mut self, values: &[f64]) {
        self.usize(values.len());
        for &value in values {
            self.f64(value);
        }
    }

    fn bool_slice(&mut self, values: &[bool]) {
        self.usize(values.len());
        for &value in values {
            self.bool(value);
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self.offset.checked_add(count).ok_or_else(|| {
            ModelError::Checkpoint("checkpoint length overflow while decoding".to_owned())
        })?;
        let slice = self.bytes.get(self.offset..end).ok_or_else(|| {
            ModelError::Checkpoint(format!(
                "unexpected end of checkpoint at byte {} (needed {count} bytes)",
                self.offset
            ))
        })?;
        self.offset = end;
        Ok(slice)
    }

    fn fixed<const N: usize>(&mut self) -> Result<[u8; N]> {
        let slice = self.take(N)?;
        let mut out = [0_u8; N];
        out.copy_from_slice(slice);
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.fixed::<1>()?[0])
    }

    fn bool(&mut self) -> Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            tag => Err(ModelError::Checkpoint(format!("invalid bool byte {tag}"))),
        }
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.fixed::<4>()?))
    }

    fn usize(&mut self) -> Result<usize> {
        let raw = u64::from_le_bytes(self.fixed::<8>()?);
        usize::try_from(raw)
            .map_err(|_| ModelError::Checkpoint(format!("length {raw} exceeds platform usize")))
    }

    fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_bits(u64::from_le_bytes(self.fixed::<8>()?)))
    }

    fn opt_f64(&mut self) -> Result<Option<f64>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.f64()?)),
            tag => Err(ModelError::Checkpoint(format!("invalid option tag {tag}"))),
        }
    }

    fn opt_usize(&mut self) -> Result<Option<usize>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.usize()?)),
            tag => Err(ModelError::Checkpoint(format!("invalid option tag {tag}"))),
        }
    }

    fn string(&mut self) -> Result<String> {
        let len = self.u32()? as usize;
        let bytes = self.take(len)?;
        std::str::from_utf8(bytes)
            .map(ToOwned::to_owned)
            .map_err(|error| ModelError::Checkpoint(format!("invalid UTF-8 string: {error}")))
    }

    fn bounded_count(&mut self, min_item_bytes: usize) -> Result<usize> {
        let count = self.usize()?;
        let remaining = self.bytes.len().saturating_sub(self.offset);
        let max_possible = remaining / min_item_bytes.max(1);
        if count > max_possible {
            return Err(ModelError::Checkpoint(format!(
                "declared count {count} exceeds remaining payload capacity ({remaining} bytes)"
            )));
        }
        Ok(count)
    }

    fn f64_vec(&mut self) -> Result<Vec<f64>> {
        let len = self.bounded_count(8)?;
        let mut out = Vec::with_capacity(len);
        for _ in 0..len {
            out.push(self.f64()?);
        }
        Ok(out)
    }

    fn bool_vec(&mut self) -> Result<Vec<bool>> {
        let len = self.bounded_count(1)?;
        let mut out = Vec::with_capacity(len);
        for _ in 0..len {
            out.push(self.bool()?);
        }
        Ok(out)
    }

    fn finish(self) -> Result<()> {
        if self.offset != self.bytes.len() {
            return Err(ModelError::Checkpoint(format!(
                "trailing checkpoint bytes: {} unread",
                self.bytes.len() - self.offset
            )));
        }
        Ok(())
    }
}
