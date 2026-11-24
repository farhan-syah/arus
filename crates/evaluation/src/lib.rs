//! Independent event evaluation, censored timing scoring, and numerical budget assessment.
//!
//! Keeps evaluation independent of operational forecast publication and enforces separation
//! between calibration, development, and locked test events, as well as reconstruction
//! versus as-issued forecast replay.

pub mod scoring;

pub use scoring::{
    BudgetAssessment, CensoredTime, DepthSeriesMetrics, EvaluationError, EvaluationMode,
    EvaluationPartition, Result, TimingScore, assess_water_budget, score_crossing_time,
    score_depth_series,
};
