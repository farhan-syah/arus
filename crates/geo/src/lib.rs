//! Checked Cartesian terrain preparation, ESRI ASCII Grid I/O, and audited conditioning edits.
//!
//! Missing terrain stays `None` (nodata) and maps to inactive hydraulic cells.
//! Missing terrain never becomes dry land or zero elevation.

pub mod ascii;
pub mod terrain;

pub use ascii::{parse_esri_ascii, write_esri_ascii};
pub use terrain::{ConditioningRecord, GeoError, Result, TerrainEdit, TerrainGrid};
