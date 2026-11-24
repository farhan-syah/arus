//! Deterministic flood-episode extraction from accepted solver time series.
//!
//! Preserves onset, peak, exceedance duration, recession crossing and confirmation times,
//! rebound counts, left-censoring (initial inundation), right-censoring (unresolved recession
//! at horizon end), and explicit unsupported location status.

use arus_model_api::CellSample;
use serde::{Deserialize, Serialize};

/// Typed errors returned by episode extraction and policy checks.
#[derive(Debug, thiserror::Error)]
pub enum ForecastError {
    #[error("invalid episode policy: {0}")]
    InvalidPolicy(String),
    #[error("invalid sample sequence: {0}")]
    InvalidSample(String),
}

/// Result alias for forecasting operations.
pub type Result<T> = std::result::Result<T, ForecastError>;

/// Frozen threshold, hysteresis, and recession-persistence rules for episode extraction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EpisodePolicy {
    /// Depth threshold in metres that qualifies an exceedance.
    pub threshold_m: f64,
    /// Optional hysteresis margin in metres below `threshold_m` required to start recession.
    pub hysteresis_m: f64,
    /// Sustained duration in seconds below `threshold_m - hysteresis_m` required to confirm recession.
    pub recession_persistence_s: f64,
}

impl EpisodePolicy {
    /// Creates a checked episode policy.
    pub fn new(threshold_m: f64, hysteresis_m: f64, recession_persistence_s: f64) -> Result<Self> {
        if !threshold_m.is_finite() || threshold_m <= 0.0 {
            return Err(ForecastError::InvalidPolicy(format!(
                "threshold_m={threshold_m}: supply a finite positive depth in metres"
            )));
        }
        if !hysteresis_m.is_finite() || hysteresis_m < 0.0 || hysteresis_m >= threshold_m {
            return Err(ForecastError::InvalidPolicy(format!(
                "hysteresis_m={hysteresis_m}: require finite value in [0, {threshold_m})"
            )));
        }
        if !recession_persistence_s.is_finite() || recession_persistence_s < 0.0 {
            return Err(ForecastError::InvalidPolicy(format!(
                "recession_persistence_s={recession_persistence_s}: supply a finite nonnegative duration"
            )));
        }
        Ok(Self {
            threshold_m,
            hysteresis_m,
            recession_persistence_s,
        })
    }

    /// Returns the depth below which recession begins.
    pub fn recession_level_m(self) -> f64 {
        self.threshold_m - self.hysteresis_m
    }
}

/// Support status for a tracked location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocationSupportStatus {
    /// All samples came from supported active model cells.
    Supported,
    /// One or more samples fell outside active model support. Never treated as dry zero.
    Unsupported,
}

/// Onset classification for a flood episode.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum OnsetStatus {
    /// First qualifying exceedance occurred within the observed horizon.
    Observed { time_s: f64 },
    /// Location was already at or above threshold at the start of the horizon.
    LeftCensored { first_observed_s: f64 },
}

/// Recession classification for a flood episode.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum RecessionStatus {
    /// Downward crossing stayed below the recession level for the full persistence window.
    Confirmed {
        crossing_time_s: f64,
        confirmation_time_s: f64,
    },
    /// Downward crossing occurred, but the horizon ended before persistence was confirmed.
    PendingAtHorizon {
        crossing_time_s: f64,
        horizon_end_s: f64,
    },
    /// Location remained at or above the recession level at horizon end.
    RightCensored { horizon_end_s: f64 },
}

/// Extracted metrics for one flood episode at a single location.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FloodEpisode {
    pub onset: OnsetStatus,
    pub initial_inundation: bool,
    pub peak_depth_m: f64,
    pub peak_time_s: f64,
    pub peak_at_horizon_end: bool,
    pub exceedance_duration_s: f64,
    pub recession: RecessionStatus,
    pub rebound_count: usize,
}

/// Complete extracted flood evolution for one location over a computed horizon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocationEvolution {
    pub location_id: String,
    pub policy: EpisodePolicy,
    pub support: LocationSupportStatus,
    pub horizon_start_s: Option<f64>,
    pub horizon_end_s: Option<f64>,
    pub max_depth_m: Option<f64>,
    pub total_exceedance_duration_s: f64,
    pub episodes: Vec<FloodEpisode>,
}

#[derive(Debug, Clone)]
struct ActiveEpisode {
    onset: OnsetStatus,
    initial_inundation: bool,
    peak_depth_m: f64,
    peak_time_s: f64,
    exceedance_duration_s: f64,
    pending_recession_start_s: Option<f64>,
    rebound_count: usize,
}

/// Streaming deterministic tracker converting accepted `(time_s, CellSample)` samples
/// into a `LocationEvolution`.
#[derive(Debug, Clone)]
pub struct EpisodeTracker {
    location_id: String,
    policy: EpisodePolicy,
    support: LocationSupportStatus,
    horizon_start_s: Option<f64>,
    last_time_s: Option<f64>,
    last_depth_m: Option<f64>,
    max_depth_m: Option<f64>,
    total_exceedance_duration_s: f64,
    active: Option<ActiveEpisode>,
    completed: Vec<FloodEpisode>,
}

impl EpisodeTracker {
    /// Creates a new tracker for `location_id` under `policy`.
    pub fn new(location_id: impl Into<String>, policy: EpisodePolicy) -> Self {
        Self {
            location_id: location_id.into(),
            policy,
            support: LocationSupportStatus::Supported,
            horizon_start_s: None,
            last_time_s: None,
            last_depth_m: None,
            max_depth_m: None,
            total_exceedance_duration_s: 0.0,
            active: None,
            completed: Vec::new(),
        }
    }

    /// Records one accepted sample at `time_s`.
    ///
    /// Rejects non-finite or non-monotonic timestamps.
    pub fn observe(&mut self, time_s: f64, sample: CellSample) -> Result<()> {
        if !time_s.is_finite() {
            return Err(ForecastError::InvalidSample(format!(
                "non-finite timestamp {time_s}"
            )));
        }
        if let Some(prev) = self.last_time_s
            && time_s < prev
        {
            return Err(ForecastError::InvalidSample(format!(
                "non-monotonic timestamp {time_s} s < previous {prev} s"
            )));
        }

        let is_first = self.horizon_start_s.is_none();
        if is_first {
            self.horizon_start_s = Some(time_s);
        }
        let dt = self.last_time_s.map_or(0.0, |prev| time_s - prev);

        let depth_m = match sample {
            CellSample::Unsupported => {
                self.support = LocationSupportStatus::Unsupported;
                self.last_time_s = Some(time_s);
                self.last_depth_m = None;
                self.max_depth_m = None;
                self.active = None;
                self.completed.clear();
                self.total_exceedance_duration_s = 0.0;
                return Ok(());
            }
            CellSample::Dry => 0.0,
            CellSample::Wet { depth_m, .. } => {
                if !depth_m.is_finite() || depth_m < 0.0 {
                    return Err(ForecastError::InvalidSample(format!(
                        "invalid wet depth {depth_m} m at t={time_s} s"
                    )));
                }
                depth_m
            }
        };

        if self.support == LocationSupportStatus::Unsupported {
            self.last_time_s = Some(time_s);
            return Ok(());
        }

        self.max_depth_m = Some(
            self.max_depth_m
                .map_or(depth_m, |current| current.max(depth_m)),
        );

        let prev_exceeding = self
            .last_depth_m
            .is_some_and(|prev| prev >= self.policy.threshold_m);
        let now_exceeding = depth_m >= self.policy.threshold_m;
        let below_recession = depth_m < self.policy.recession_level_m();

        // Conservative step-wise exceedance integration over the accepted interval `[t_prev, t]`.
        let interval_exceedance = if dt > 0.0 && (prev_exceeding || now_exceeding) {
            if prev_exceeding && now_exceeding {
                dt
            } else {
                // Exact linear crossing fraction within the accepted step when one endpoint exceeds.
                let prev_depth = self.last_depth_m.unwrap_or(0.0);
                let delta = depth_m - prev_depth;
                if delta == 0.0 {
                    0.0
                } else {
                    let frac = ((self.policy.threshold_m - prev_depth) / delta).clamp(0.0, 1.0);
                    if now_exceeding {
                        dt * (1.0 - frac)
                    } else {
                        dt * frac
                    }
                }
            }
        } else {
            0.0
        };
        self.total_exceedance_duration_s += interval_exceedance;

        match self.active.as_mut() {
            None => {
                if now_exceeding {
                    let onset = if is_first {
                        OnsetStatus::LeftCensored {
                            first_observed_s: time_s,
                        }
                    } else {
                        OnsetStatus::Observed { time_s }
                    };
                    self.active = Some(ActiveEpisode {
                        onset,
                        initial_inundation: is_first,
                        peak_depth_m: depth_m,
                        peak_time_s: time_s,
                        exceedance_duration_s: interval_exceedance,
                        pending_recession_start_s: None,
                        rebound_count: 0,
                    });
                }
            }
            Some(active) => {
                active.exceedance_duration_s += interval_exceedance;
                if depth_m > active.peak_depth_m {
                    active.peak_depth_m = depth_m;
                    active.peak_time_s = time_s;
                }

                if now_exceeding {
                    if active.pending_recession_start_s.take().is_some() {
                        active.rebound_count += 1;
                    }
                } else if below_recession {
                    let crossing = *active.pending_recession_start_s.get_or_insert(time_s);
                    let confirmation_time_s = crossing + self.policy.recession_persistence_s;
                    if time_s >= confirmation_time_s {
                        let finished = self.active.take();
                        if let Some(episode) = finished {
                            self.completed.push(FloodEpisode {
                                onset: episode.onset,
                                initial_inundation: episode.initial_inundation,
                                peak_depth_m: episode.peak_depth_m,
                                peak_time_s: episode.peak_time_s,
                                peak_at_horizon_end: false,
                                exceedance_duration_s: episode.exceedance_duration_s,
                                recession: RecessionStatus::Confirmed {
                                    crossing_time_s: crossing,
                                    confirmation_time_s,
                                },
                                rebound_count: episode.rebound_count,
                            });
                        }
                    }
                } else {
                    // Depth rose back into the hysteresis band `[recession_level, threshold)`:
                    // sustained recession below `recession_level` is broken.
                    active.pending_recession_start_s = None;
                }
            }
        }

        self.last_time_s = Some(time_s);
        self.last_depth_m = Some(depth_m);
        Ok(())
    }

    /// Finalises the evolution at the last observed horizon time without inventing recession.
    pub fn finish(mut self) -> LocationEvolution {
        if self.support == LocationSupportStatus::Unsupported {
            return LocationEvolution {
                location_id: self.location_id,
                policy: self.policy,
                support: LocationSupportStatus::Unsupported,
                horizon_start_s: self.horizon_start_s,
                horizon_end_s: self.last_time_s,
                max_depth_m: None,
                total_exceedance_duration_s: 0.0,
                episodes: Vec::new(),
            };
        }

        if let Some(active) = self.active.take() {
            let horizon_end_s = self.last_time_s.unwrap_or(active.peak_time_s);
            let recession = match active.pending_recession_start_s {
                Some(crossing_time_s) => RecessionStatus::PendingAtHorizon {
                    crossing_time_s,
                    horizon_end_s,
                },
                None => RecessionStatus::RightCensored { horizon_end_s },
            };
            let peak_at_horizon_end = active.peak_time_s == horizon_end_s;
            self.completed.push(FloodEpisode {
                onset: active.onset,
                initial_inundation: active.initial_inundation,
                peak_depth_m: active.peak_depth_m,
                peak_time_s: active.peak_time_s,
                peak_at_horizon_end,
                exceedance_duration_s: active.exceedance_duration_s,
                recession,
                rebound_count: active.rebound_count,
            });
        }

        LocationEvolution {
            location_id: self.location_id,
            policy: self.policy,
            support: self.support,
            horizon_start_s: self.horizon_start_s,
            horizon_end_s: self.last_time_s,
            max_depth_m: self.max_depth_m,
            total_exceedance_duration_s: self.total_exceedance_duration_s,
            episodes: self.completed,
        }
    }
}
