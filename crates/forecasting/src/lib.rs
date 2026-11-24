//! Forecast cycles, location flood-episode extraction, and threshold-crossing contracts.
//!
//! Execution and episode extraction do not authorise operational forecast publication.

pub mod episode;

pub use episode::{
    EpisodePolicy, EpisodeTracker, FloodEpisode, ForecastError, LocationEvolution,
    LocationSupportStatus, OnsetStatus, RecessionStatus, Result,
};
