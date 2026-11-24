//! Forecast cycles, location flood-episode extraction, and publication gates.
//!
//! Execution and episode extraction do not authorise operational forecast publication.

pub mod episode;
pub mod publication;

pub use episode::{
    EpisodePolicy, EpisodeTracker, FloodEpisode, ForecastError, LocationEvolution,
    LocationSupportStatus, OnsetStatus, RecessionStatus, Result,
};
pub use publication::{
    ApprovalStatus, PublicationCandidate, PublicationDecision, PublicationManifest,
    evaluate_publication,
};
