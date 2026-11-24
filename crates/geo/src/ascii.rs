//! ESRI ASCII Grid (`.asc`) bare-earth DEM parser and serializer.
//!
//! Converts standard top-to-bottom ESRI ASCII rows into ARUS bottom-to-top row-major
//! order and maps `NODATA_value` strictly to `None`.

use arus_domain::{GridSpec, SpatialReference};

use crate::terrain::{GeoError, Result, TerrainGrid};

/// Parses an ESRI ASCII Grid (`.asc`) string into a checked `TerrainGrid`.
///
/// Supports `ncols`, `nrows`, `xllcorner`, `yllcorner`, `cellsize`, and optional
/// `NODATA_value` (default `-9999`). Flips top-to-bottom ASCII rows into bottom-to-top
/// Cartesian row-major order matching `hydr::grid::CartesianGrid`.
pub fn parse_esri_ascii(
    content: &str,
    projected_crs: impl Into<String>,
    vertical_datum: impl Into<String>,
    source_provenance: impl Into<String>,
) -> Result<TerrainGrid> {
    parse_esri_ascii_bounded(
        content,
        projected_crs,
        vertical_datum,
        source_provenance,
        1_000_000,
    )
}

/// Parses an ESRI ASCII Grid (`.asc`) enforcing `ncols * nrows <= max_cells` before collecting data.
pub fn parse_esri_ascii_bounded(
    content: &str,
    projected_crs: impl Into<String>,
    vertical_datum: impl Into<String>,
    source_provenance: impl Into<String>,
    max_cells: usize,
) -> Result<TerrainGrid> {
    let mut ncols: Option<usize> = None;
    let mut nrows: Option<usize> = None;
    let mut xllcorner: Option<f64> = None;
    let mut yllcorner: Option<f64> = None;
    let mut cellsize: Option<f64> = None;
    let mut nodata: f64 = -9999.0;

    let mut data_tokens = Vec::new();
    let mut in_data = false;

    for (line_no, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if !in_data {
            let mut parts = line.split_whitespace();
            if let (Some(key), Some(val), None) = (parts.next(), parts.next(), parts.next()) {
                match key.to_ascii_lowercase().as_str() {
                    "ncols" => {
                        ncols = Some(val.parse::<usize>().map_err(|_| {
                            GeoError::InvalidGrid(format!(
                                "line {}: invalid ncols {val}",
                                line_no + 1
                            ))
                        })?);
                        continue;
                    }
                    "nrows" => {
                        nrows = Some(val.parse::<usize>().map_err(|_| {
                            GeoError::InvalidGrid(format!(
                                "line {}: invalid nrows {val}",
                                line_no + 1
                            ))
                        })?);
                        continue;
                    }
                    "xllcorner" => {
                        xllcorner = Some(val.parse::<f64>().map_err(|_| {
                            GeoError::InvalidGrid(format!(
                                "line {}: invalid xllcorner {val}",
                                line_no + 1
                            ))
                        })?);
                        continue;
                    }
                    "yllcorner" => {
                        yllcorner = Some(val.parse::<f64>().map_err(|_| {
                            GeoError::InvalidGrid(format!(
                                "line {}: invalid yllcorner {val}",
                                line_no + 1
                            ))
                        })?);
                        continue;
                    }
                    "cellsize" => {
                        cellsize = Some(val.parse::<f64>().map_err(|_| {
                            GeoError::InvalidGrid(format!(
                                "line {}: invalid cellsize {val}",
                                line_no + 1
                            ))
                        })?);
                        continue;
                    }
                    "nodata_value" => {
                        nodata = val.parse::<f64>().map_err(|_| {
                            GeoError::InvalidGrid(format!(
                                "line {}: invalid NODATA_value {val}",
                                line_no + 1
                            ))
                        })?;
                        continue;
                    }
                    _ => {}
                }
            }
            in_data = true;
            let cols =
                ncols.ok_or_else(|| GeoError::InvalidGrid("missing ncols header".to_owned()))?;
            let rows =
                nrows.ok_or_else(|| GeoError::InvalidGrid("missing nrows header".to_owned()))?;
            let total = cols
                .checked_mul(rows)
                .ok_or_else(|| GeoError::InvalidGrid("grid dimensions overflow".to_owned()))?;
            if total > max_cells {
                return Err(GeoError::InvalidGrid(format!(
                    "grid cell count {total} exceeds maximum allowed {max_cells}"
                )));
            }
        }
        data_tokens.extend(line.split_whitespace());
        if data_tokens.len() > max_cells {
            return Err(GeoError::InvalidGrid(format!(
                "raster token count exceeds maximum allowed {max_cells}"
            )));
        }
    }

    let cols = ncols.ok_or_else(|| GeoError::InvalidGrid("missing ncols header".to_owned()))?;
    let rows = nrows.ok_or_else(|| GeoError::InvalidGrid("missing nrows header".to_owned()))?;
    let ox =
        xllcorner.ok_or_else(|| GeoError::InvalidGrid("missing xllcorner header".to_owned()))?;
    let oy =
        yllcorner.ok_or_else(|| GeoError::InvalidGrid("missing yllcorner header".to_owned()))?;
    let dx = cellsize.ok_or_else(|| GeoError::InvalidGrid("missing cellsize header".to_owned()))?;

    let total = cols
        .checked_mul(rows)
        .ok_or_else(|| GeoError::InvalidGrid("grid dimensions overflow".to_owned()))?;
    if data_tokens.len() != total {
        return Err(GeoError::InvalidGrid(format!(
            "expected {total} raster values, found {}",
            data_tokens.len()
        )));
    }

    let mut cells = vec![None; total];
    for file_row in 0..rows {
        let grid_row = rows - 1 - file_row;
        for col in 0..cols {
            let token = data_tokens[file_row * cols + col];
            let value = token.parse::<f64>().map_err(|_| {
                GeoError::InvalidGrid(format!(
                    "invalid elevation token {token:?} at row {file_row}, col {col}"
                ))
            })?;
            cells[grid_row * cols + col] = if value == nodata { None } else { Some(value) };
        }
    }

    TerrainGrid::new(
        GridSpec {
            columns: cols,
            rows,
            dx_m: dx,
            dy_m: dx,
        },
        SpatialReference {
            projected_crs: projected_crs.into(),
            vertical_datum: vertical_datum.into(),
            origin_m: [ox, oy],
        },
        source_provenance,
        cells,
    )
}

/// Serializes a square-cell `TerrainGrid` into standard top-to-bottom ESRI ASCII Grid format.
pub fn write_esri_ascii(terrain: &TerrainGrid, nodata_value: f64) -> Result<String> {
    let spec = terrain.grid();
    if (spec.dx_m - spec.dy_m).abs() > 1e-12 {
        return Err(GeoError::InvalidGrid(format!(
            "ESRI ASCII Grid requires square cells (dx={} != dy={})",
            spec.dx_m, spec.dy_m
        )));
    }
    if !nodata_value.is_finite() {
        return Err(GeoError::InvalidGrid(
            "nodata_value must be finite".to_owned(),
        ));
    }
    let origin = terrain.reference().origin_m;
    let mut out = format!(
        "ncols {}\nnrows {}\nxllcorner {}\nyllcorner {}\ncellsize {}\nNODATA_value {}\n",
        spec.columns, spec.rows, origin[0], origin[1], spec.dx_m, nodata_value
    );
    for file_row in 0..spec.rows {
        let grid_row = spec.rows - 1 - file_row;
        for col in 0..spec.columns {
            if col > 0 {
                out.push(' ');
            }
            let val = terrain.cells()[grid_row * spec.columns + col].unwrap_or(nodata_value);
            use std::fmt::Write as _;
            let _ = write!(&mut out, "{val}");
        }
        out.push('\n');
    }
    Ok(out)
}
