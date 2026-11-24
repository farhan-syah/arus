//! Forecast cycles, location flood-episode extraction, and publication gates.
//!
//! Execution and episode extraction do not authorise operational forecast publication.

pub mod ensemble;
pub mod episode;
pub mod pipeline;
pub mod publication;
pub mod stations;

pub use ensemble::{
    AgreementLevel, EnsembleForecastSummary, ModelForecastMember, summarize_ensemble,
};
pub use episode::{
    EpisodePolicy, EpisodeTracker, FloodEpisode, ForecastError, LocationEvolution,
    LocationSupportStatus, OnsetStatus, RecessionStatus, Result,
};
pub use pipeline::{
    CompoundTideLock, FourStagePipelineReport, InitialBasinState, Windowed2DOutcome,
    build_initial_basin_state, evaluate_compound_tide_lock,
};
pub use publication::{
    ApprovalStatus, PublicationCandidate, PublicationDecision, PublicationManifest,
    evaluate_publication,
};
pub use stations::{
    DailyShadowForecast, DailyShadowObservation, ShadowEvaluationReport, StationError,
    StationForecastAssessment, StationRegistry, StationStatus, StationThreshold, assess_station,
    score_shadow_forecast,
};
