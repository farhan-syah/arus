//! Subgrid terrain storage-elevation and wet-fraction adapter over `hydr::solver::SubgridTerrain`.

use arus_domain::GridSpec;
use arus_model_api::{ModelError, Result};
use hydr::grid::CartesianGrid;
use hydr::solver::SubgridTerrain;
use serde::{Deserialize, Serialize};

/// Subgrid projection of coarse-cell effective depths into exact water-surface elevations and wet fractions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubgridFieldProjection {
    pub stages_m: Vec<f64>,
    pub wet_fractions: Vec<f64>,
}

/// ARUS wrapper around `hydr::solver::SubgridTerrain` for fine-DEM storage curves on coarse grids.
#[derive(Debug, Clone)]
pub struct SubgridTerrainAdapter {
    inner: SubgridTerrain,
}

impl SubgridTerrainAdapter {
    /// Builds exact per-cell stage-volume curves over `coarse_grid` from a fine-resolution DEM.
    pub fn from_fine_dem(
        coarse_grid: GridSpec,
        fine_columns: usize,
        fine_rows: usize,
        fine_bed_m: &[f64],
    ) -> Result<Self> {
        let grid = CartesianGrid::new(
            coarse_grid.columns,
            coarse_grid.rows,
            coarse_grid.dx_m,
            coarse_grid.dy_m,
        )
        .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
        let inner = SubgridTerrain::from_fine_dem(grid, fine_columns, fine_rows, fine_bed_m)
            .map_err(|error| ModelError::InvalidInput(error.to_string()))?;
        Ok(Self { inner })
    }

    /// Number of coarse cells covered by the subgrid table.
    pub fn cell_count(&self) -> usize {
        self.inner.curves().len()
    }

    /// Converts coarse effective depths (`m`) into exact subgrid stages (`m`) and wet fractions (`[0, 1]`).
    pub fn project_depths(&self, effective_depths_m: &[f64]) -> Result<SubgridFieldProjection> {
        if effective_depths_m.len() != self.inner.curves().len() {
            return Err(ModelError::InvalidInput(format!(
                "effective_depths_m length {} does not match subgrid cell count {}",
                effective_depths_m.len(),
                self.inner.curves().len()
            )));
        }
        Ok(SubgridFieldProjection {
            stages_m: self.inner.stage_field(effective_depths_m),
            wet_fractions: self.inner.wet_fraction_field(effective_depths_m),
        })
    }

    /// Converts per-cell water-surface elevations (`m`) back into coarse effective depths (`m`).
    pub fn effective_depths(&self, stages_m: &[f64]) -> Result<Vec<f64>> {
        if stages_m.len() != self.inner.curves().len() {
            return Err(ModelError::InvalidInput(format!(
                "stages_m length {} does not match subgrid cell count {}",
                stages_m.len(),
                self.inner.curves().len()
            )));
        }
        Ok(self.inner.effective_depth_field(stages_m))
    }
}
