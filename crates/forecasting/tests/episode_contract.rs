use arus_forecasting::{
    EpisodePolicy, EpisodeTracker, LocationSupportStatus, OnsetStatus, RecessionStatus,
};
use arus_model_api::CellSample;

fn wet(depth_m: f64) -> CellSample {
    CellSample::Wet {
        depth_m,
        water_elevation_m: depth_m,
        velocity_x_m_s: 0.0,
        velocity_y_m_s: 0.0,
    }
}

#[test]
fn below_threshold_produces_no_episodes_and_tracks_max_depth() -> Result<(), String> {
    let policy = EpisodePolicy::new(0.15, 0.0, 60.0).map_err(|e| e.to_string())?;
    let mut tracker = EpisodeTracker::new("loc-a", policy);
    tracker
        .observe(0.0, CellSample::Dry)
        .map_err(|e| e.to_string())?;
    tracker
        .observe(10.0, wet(0.05))
        .map_err(|e| e.to_string())?;
    tracker
        .observe(20.0, wet(0.10))
        .map_err(|e| e.to_string())?;
    tracker
        .observe(30.0, CellSample::Dry)
        .map_err(|e| e.to_string())?;

    let evo = tracker.finish();
    assert_eq!(evo.support, LocationSupportStatus::Supported);
    assert!(evo.episodes.is_empty());
    assert_eq!(evo.max_depth_m, Some(0.10));
    assert_eq!(evo.total_exceedance_duration_s, 0.0);
    Ok(())
}

#[test]
fn clean_episode_preserves_onset_peak_and_recession_confirmation() -> Result<(), String> {
    let policy = EpisodePolicy::new(0.10, 0.02, 30.0).map_err(|e| e.to_string())?;
    let mut tracker = EpisodeTracker::new("loc-b", policy);
    tracker
        .observe(0.0, CellSample::Dry)
        .map_err(|e| e.to_string())?;
    tracker
        .observe(10.0, wet(0.12))
        .map_err(|e| e.to_string())?;
    tracker
        .observe(20.0, wet(0.30))
        .map_err(|e| e.to_string())?;
    tracker
        .observe(30.0, wet(0.09))
        .map_err(|e| e.to_string())?;
    // Below recession level (0.08 m) at t=40
    tracker
        .observe(40.0, wet(0.05))
        .map_err(|e| e.to_string())?;
    tracker
        .observe(55.0, wet(0.03))
        .map_err(|e| e.to_string())?;
    tracker
        .observe(70.0, CellSample::Dry)
        .map_err(|e| e.to_string())?;

    let evo = tracker.finish();
    assert_eq!(evo.episodes.len(), 1);
    let ep = &evo.episodes[0];
    assert_eq!(ep.onset, OnsetStatus::Observed { time_s: 10.0 });
    assert!(!ep.initial_inundation);
    assert_eq!(ep.peak_depth_m, 0.30);
    assert_eq!(ep.peak_time_s, 20.0);
    assert!(!ep.peak_at_horizon_end);
    assert_eq!(
        ep.recession,
        RecessionStatus::Confirmed {
            crossing_time_s: 40.0,
            confirmation_time_s: 70.0,
        }
    );
    assert_eq!(ep.rebound_count, 0);
    Ok(())
}

#[test]
fn initial_inundation_and_right_censored_horizon_coexist() -> Result<(), String> {
    let policy = EpisodePolicy::new(0.10, 0.0, 60.0).map_err(|e| e.to_string())?;
    let mut tracker = EpisodeTracker::new("loc-c", policy);
    tracker.observe(0.0, wet(0.20)).map_err(|e| e.to_string())?;
    tracker
        .observe(30.0, wet(0.45))
        .map_err(|e| e.to_string())?;

    let evo = tracker.finish();
    assert_eq!(evo.episodes.len(), 1);
    let ep = &evo.episodes[0];
    assert!(ep.initial_inundation);
    assert_eq!(
        ep.onset,
        OnsetStatus::LeftCensored {
            first_observed_s: 0.0
        }
    );
    assert!(ep.peak_at_horizon_end);
    assert_eq!(
        ep.recession,
        RecessionStatus::RightCensored {
            horizon_end_s: 30.0
        }
    );
    assert_eq!(ep.exceedance_duration_s, 30.0);
    Ok(())
}

#[test]
fn incomplete_recession_persistence_is_pending_not_confirmed() -> Result<(), String> {
    let policy = EpisodePolicy::new(0.10, 0.0, 60.0).map_err(|e| e.to_string())?;
    let mut tracker = EpisodeTracker::new("loc-d", policy);
    tracker
        .observe(0.0, CellSample::Dry)
        .map_err(|e| e.to_string())?;
    tracker
        .observe(10.0, wet(0.20))
        .map_err(|e| e.to_string())?;
    tracker
        .observe(20.0, wet(0.05))
        .map_err(|e| e.to_string())?;
    tracker
        .observe(50.0, CellSample::Dry)
        .map_err(|e| e.to_string())?;

    let evo = tracker.finish();
    assert_eq!(evo.episodes.len(), 1);
    assert_eq!(
        evo.episodes[0].recession,
        RecessionStatus::PendingAtHorizon {
            crossing_time_s: 20.0,
            horizon_end_s: 50.0,
        }
    );
    Ok(())
}

#[test]
fn rebound_cancels_pending_recession_and_later_reexceedance_starts_new_episode()
-> Result<(), String> {
    let policy = EpisodePolicy::new(0.10, 0.0, 30.0).map_err(|e| e.to_string())?;
    let mut tracker = EpisodeTracker::new("loc-e", policy);
    tracker
        .observe(0.0, CellSample::Dry)
        .map_err(|e| e.to_string())?;
    tracker
        .observe(10.0, wet(0.20))
        .map_err(|e| e.to_string())?;
    // Starts pending recession at t=20
    tracker
        .observe(20.0, wet(0.05))
        .map_err(|e| e.to_string())?;
    // Rebounds before t=50
    tracker
        .observe(35.0, wet(0.25))
        .map_err(|e| e.to_string())?;
    // Sustained recession from t=50 to t=80
    tracker
        .observe(50.0, wet(0.04))
        .map_err(|e| e.to_string())?;
    tracker
        .observe(80.0, CellSample::Dry)
        .map_err(|e| e.to_string())?;
    // Second separate episode at t=100
    tracker
        .observe(100.0, wet(0.15))
        .map_err(|e| e.to_string())?;

    let evo = tracker.finish();
    assert_eq!(evo.episodes.len(), 2);
    assert_eq!(evo.episodes[0].rebound_count, 1);
    assert_eq!(evo.episodes[0].peak_depth_m, 0.25);
    assert_eq!(
        evo.episodes[0].recession,
        RecessionStatus::Confirmed {
            crossing_time_s: 50.0,
            confirmation_time_s: 80.0,
        }
    );
    assert_eq!(
        evo.episodes[1].onset,
        OnsetStatus::Observed { time_s: 100.0 }
    );
    assert_eq!(
        evo.episodes[1].recession,
        RecessionStatus::RightCensored {
            horizon_end_s: 100.0
        }
    );
    Ok(())
}

#[test]
fn unsupported_location_never_becomes_dry_zero() -> Result<(), String> {
    let policy = EpisodePolicy::new(0.10, 0.0, 30.0).map_err(|e| e.to_string())?;
    let mut tracker = EpisodeTracker::new("outside-domain", policy);
    tracker
        .observe(0.0, CellSample::Unsupported)
        .map_err(|e| e.to_string())?;
    tracker
        .observe(10.0, CellSample::Unsupported)
        .map_err(|e| e.to_string())?;

    let evo = tracker.finish();
    assert_eq!(evo.support, LocationSupportStatus::Unsupported);
    assert!(evo.episodes.is_empty());
    assert_eq!(evo.max_depth_m, None);
    Ok(())
}

#[test]
fn non_monotonic_timestamp_is_rejected() -> Result<(), String> {
    let policy = EpisodePolicy::new(0.10, 0.0, 30.0).map_err(|e| e.to_string())?;
    let mut tracker = EpisodeTracker::new("loc-f", policy);
    tracker
        .observe(10.0, CellSample::Dry)
        .map_err(|e| e.to_string())?;
    assert!(tracker.observe(9.0, CellSample::Dry).is_err());
    Ok(())
}

#[test]
fn hysteresis_band_resets_pending_recession_and_durations_sum_exactly() -> Result<(), String> {
    let policy = EpisodePolicy::new(0.10, 0.02, 30.0).map_err(|e| e.to_string())?;
    let mut tracker = EpisodeTracker::new("loc-g", policy);
    tracker
        .observe(0.0, CellSample::Dry)
        .map_err(|e| e.to_string())?;
    tracker
        .observe(10.0, wet(0.20))
        .map_err(|e| e.to_string())?;
    tracker
        .observe(20.0, wet(0.05))
        .map_err(|e| e.to_string())?;
    tracker
        .observe(35.0, wet(0.09))
        .map_err(|e| e.to_string())?;
    tracker
        .observe(40.0, wet(0.05))
        .map_err(|e| e.to_string())?;
    tracker
        .observe(55.0, CellSample::Dry)
        .map_err(|e| e.to_string())?;

    let evo = tracker.finish();
    assert_eq!(evo.episodes.len(), 1);
    let ep = &evo.episodes[0];
    assert_eq!(
        ep.recession,
        RecessionStatus::PendingAtHorizon {
            crossing_time_s: 40.0,
            horizon_end_s: 55.0,
        }
    );
    assert!((ep.exceedance_duration_s - evo.total_exceedance_duration_s).abs() < 1e-12);
    Ok(())
}
