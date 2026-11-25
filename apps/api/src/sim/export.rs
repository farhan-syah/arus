//! GIS exports (`.asc` max-depth grid and `.csv` peak envelope) for cached simulations.

use arus_domain::{GridSpec, SpatialReference};
use arus_geo::{TerrainGrid, write_esri_ascii};

use super::CachedSimulation;

/// Formats `max_depth_m` from a cached simulation as an ESRI ASCII Grid (`.asc`) string.
pub fn export_max_depth_ascii(sim: &CachedSimulation) -> Result<String, String> {
    let meta = &sim.series.metadata;
    let cells: Vec<Option<f64>> = sim
        .series
        .envelope
        .max_depth_m
        .iter()
        .zip(meta.active_mask.iter())
        .map(|(&d, &active)| if active { Some(f64::from(d)) } else { None })
        .collect();
    let grid = TerrainGrid::new(
        GridSpec {
            columns: meta.cols,
            rows: meta.rows,
            dx_m: meta.dx_m,
            dy_m: meta.dy_m,
        },
        SpatialReference {
            projected_crs: meta.projected_crs.clone(),
            vertical_datum: meta.vertical_datum.clone(),
            origin_m: meta.origin_m,
        },
        "max-depth-export",
        cells,
    )
    .map_err(|e| e.to_string())?;
    write_esri_ascii(&grid, -9999.0).map_err(|e| e.to_string())
}

/// Formats per-cell peak depth, speed, DEFRA FD2320 hazard, and onset time as CSV.
pub fn export_envelope_csv(sim: &CachedSimulation) -> String {
    let meta = &sim.series.metadata;
    let env = &sim.series.envelope;
    let mut out = String::from(
        "col,row,x_m,y_m,active,bed_m,max_depth_m,max_speed_m_s,max_hazard_fd2320,onset_time_s\n",
    );
    for r in 0..meta.rows {
        for c in 0..meta.cols {
            let idx = r * meta.cols + c;
            let x_m = meta.origin_m[0] + (c as f64 + 0.5) * meta.dx_m;
            let y_m = meta.origin_m[1] + (r as f64 + 0.5) * meta.dy_m;
            let active = meta.active_mask[idx];
            let line = format!(
                "{c},{r},{x_m:.2},{y_m:.2},{active},{:.4},{:.4},{:.4},{:.4},{:.2}\n",
                meta.bed_m[idx],
                env.max_depth_m[idx],
                env.max_speed_m_s[idx],
                env.max_hazard_m2_s[idx],
                env.onset_time_s[idx]
            );
            out.push_str(&line);
        }
    }
    out
}
