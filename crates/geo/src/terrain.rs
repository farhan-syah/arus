//! Checked Cartesian terrain grids with explicit nodata handling and audited edits.

use arus_domain::{GridSpec, SpatialReference};
use serde::{Deserialize, Serialize};

/// Typed errors returned by terrain preparation and conditioning operations.
#[derive(Debug, thiserror::Error)]
pub enum GeoError {
    #[error("invalid terrain grid: {0}")]
    InvalidGrid(String),
    #[error("invalid terrain edit: {0}")]
    InvalidEdit(String),
}

/// Result alias for geographic preparation operations.
pub type Result<T> = std::result::Result<T, GeoError>;

/// Immutable checked Cartesian bare-earth elevation grid with explicit nodata (`None`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerrainGrid {
    grid: GridSpec,
    reference: SpatialReference,
    source_provenance: String,
    cells: Vec<Option<f64>>,
    conditioning_history: Vec<ConditioningRecord>,
}

/// Reversible, audited terrain conditioning edit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TerrainEdit {
    /// Replaces one cell's bare-earth elevation in metres with recorded justification.
    CellElevationOverride {
        cell: usize,
        new_elevation_m: f64,
        reason: String,
    },
    /// Excludes one cell as nodata with recorded justification.
    MaskExclusion { cell: usize, reason: String },
}

/// Immutable audit record of one applied terrain conditioning edit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConditioningRecord {
    pub edit: TerrainEdit,
    pub previous_elevation_m: Option<f64>,
}

/// Cut-and-fill earthwork volumes produced by a batch of terrain conditioning edits.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EarthworkSummary {
    pub cut_volume_m3: f64,
    pub fill_volume_m3: f64,
    pub net_volume_m3: f64,
}

impl TerrainGrid {
    /// Constructs a checked terrain grid.
    ///
    /// Rejects empty CRS/datum/provenance, non-positive spacing, mismatched lengths,
    /// non-finite elevations, and grids with zero active cells.
    pub fn new(
        grid: GridSpec,
        reference: SpatialReference,
        source_provenance: impl Into<String>,
        cells: Vec<Option<f64>>,
    ) -> Result<Self> {
        let source_provenance = source_provenance.into();
        if reference.projected_crs.trim().is_empty()
            || reference.vertical_datum.trim().is_empty()
            || source_provenance.trim().is_empty()
        {
            return Err(GeoError::InvalidGrid(
                "projected_crs, vertical_datum, and source_provenance must be non-empty".to_owned(),
            ));
        }
        if !reference.origin_m[0].is_finite() || !reference.origin_m[1].is_finite() {
            return Err(GeoError::InvalidGrid(
                "origin_m coordinates must be finite".to_owned(),
            ));
        }
        if grid.columns == 0
            || grid.rows == 0
            || !grid.dx_m.is_finite()
            || grid.dx_m <= 0.0
            || !grid.dy_m.is_finite()
            || grid.dy_m <= 0.0
        {
            return Err(GeoError::InvalidGrid(format!(
                "invalid grid dimensions or spacing: {grid:?}"
            )));
        }
        let expected = grid
            .cell_count()
            .ok_or_else(|| GeoError::InvalidGrid("grid cell count overflow".to_owned()))?;
        if cells.len() != expected {
            return Err(GeoError::InvalidGrid(format!(
                "expected {expected} cells, received {}",
                cells.len()
            )));
        }
        let mut active_count = 0_usize;
        for (index, cell) in cells.iter().enumerate() {
            if let Some(z) = cell {
                if !z.is_finite() {
                    return Err(GeoError::InvalidGrid(format!(
                        "cell[{index}] elevation {z} is non-finite"
                    )));
                }
                active_count += 1;
            }
        }
        if active_count == 0 {
            return Err(GeoError::InvalidGrid(
                "terrain grid has zero active cells".to_owned(),
            ));
        }
        Ok(Self {
            grid,
            reference,
            source_provenance,
            cells,
            conditioning_history: Vec::new(),
        })
    }

    /// Returns grid dimensions and spacing.
    pub fn grid(&self) -> GridSpec {
        self.grid
    }

    /// Returns caller-attested CRS, vertical datum, and origin.
    pub fn reference(&self) -> &SpatialReference {
        &self.reference
    }

    /// Returns terrain source provenance.
    pub fn source_provenance(&self) -> &str {
        &self.source_provenance
    }

    /// Returns row-major cells (`None` = nodata).
    pub fn cells(&self) -> &[Option<f64>] {
        &self.cells
    }

    /// Returns row-major active mask (`true` where terrain is present).
    pub fn active_mask(&self) -> Vec<bool> {
        self.cells.iter().map(Option::is_some).collect()
    }

    /// Returns the number of active cells.
    pub fn active_count(&self) -> usize {
        self.cells.iter().filter(|cell| cell.is_some()).count()
    }

    /// Returns the number of nodata cells.
    pub fn nodata_count(&self) -> usize {
        self.cells.iter().filter(|cell| cell.is_none()).count()
    }

    /// Returns the audit history of conditioning edits applied to this revision.
    pub fn conditioning_history(&self) -> &[ConditioningRecord] {
        &self.conditioning_history
    }

    /// Applies a single audited conditioning edit, returning a new immutable `TerrainGrid`.
    pub fn apply_edit(&self, edit: TerrainEdit) -> Result<Self> {
        let (next, _) = self.apply_edits(vec![edit])?;
        Ok(next)
    }

    /// Applies a batch of audited conditioning edits in one clone and computes cut/fill earthwork volumes.
    pub fn apply_edits(&self, edits: Vec<TerrainEdit>) -> Result<(Self, EarthworkSummary)> {
        let mut next = self.clone();
        let cell_area = self.grid.dx_m * self.grid.dy_m;
        let mut cut_volume_m3 = 0.0_f64;
        let mut fill_volume_m3 = 0.0_f64;

        for edit in edits {
            match &edit {
                TerrainEdit::CellElevationOverride {
                    cell,
                    new_elevation_m,
                    reason,
                } => {
                    if reason.trim().is_empty() {
                        return Err(GeoError::InvalidEdit(
                            "conditioning edit requires a non-empty reason".to_owned(),
                        ));
                    }
                    if !new_elevation_m.is_finite() {
                        return Err(GeoError::InvalidEdit(format!(
                            "new_elevation_m={new_elevation_m} must be finite"
                        )));
                    }
                    let slot = next.cells.get_mut(*cell).ok_or_else(|| {
                        GeoError::InvalidEdit(format!("cell index {cell} out of bounds"))
                    })?;
                    let previous_elevation_m = *slot;
                    if let Some(prev) = previous_elevation_m {
                        let delta = *new_elevation_m - prev;
                        if delta > 0.0 {
                            fill_volume_m3 += delta * cell_area;
                        } else if delta < 0.0 {
                            cut_volume_m3 += (-delta) * cell_area;
                        }
                    }
                    *slot = Some(*new_elevation_m);
                    next.conditioning_history.push(ConditioningRecord {
                        edit,
                        previous_elevation_m,
                    });
                }
                TerrainEdit::MaskExclusion { cell, reason } => {
                    if reason.trim().is_empty() {
                        return Err(GeoError::InvalidEdit(
                            "mask exclusion requires a non-empty reason".to_owned(),
                        ));
                    }
                    let slot = next.cells.get_mut(*cell).ok_or_else(|| {
                        GeoError::InvalidEdit(format!("cell index {cell} out of bounds"))
                    })?;
                    let previous_elevation_m = *slot;
                    *slot = None;
                    if next.active_count() == 0 {
                        return Err(GeoError::InvalidEdit(
                            "cannot exclude the final active terrain cell".to_owned(),
                        ));
                    }
                    next.conditioning_history.push(ConditioningRecord {
                        edit,
                        previous_elevation_m,
                    });
                }
            }
        }
        let summary = EarthworkSummary {
            cut_volume_m3,
            fill_volume_m3,
            net_volume_m3: fill_volume_m3 - cut_volume_m3,
        };
        Ok((next, summary))
    }
}
