//! Versioned conditional scenario manifests and paired baseline/intervention comparisons.
//!
//! Paired comparisons enforce matched forcing and initial-state digests, check water-budget
//! conservation on both runs, and report transferred (worsened) impacts explicitly.

pub mod compare;

pub use compare::{
    LocationDelta, PairedComparisonReport, ScenarioError, ScenarioManifest, ScenarioRunSummary,
    compare_paired_runs,
};
