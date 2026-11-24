//! Constrained mitigation-candidate screening and Pareto trade-off extraction.
//!
//! Outputs are advisory screening candidates requiring full-model verification of finalists.

pub mod screening;

pub use screening::{
    MitigationCandidate, MitigationConstraints, MitigationError, MitigationScore,
    RejectedCandidate, Result, ScreeningReport, screen_candidates,
};
