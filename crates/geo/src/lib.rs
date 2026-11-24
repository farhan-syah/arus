//! Checked Cartesian terrain preparation and audited conditioning edits.
//!
//! Missing terrain stays `None` (nodata) and maps to inactive hydraulic cells.
//! Missing terrain never becomes dry land or zero elevation.

pub mod terrain;

pub use terrain::{ConditioningRecord, GeoError, Result, TerrainEdit, TerrainGrid};
