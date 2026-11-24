use arus_evaluation::{
    CensoredTime, TimingScore, assess_water_budget, score_crossing_time, score_depth_series,
};
use arus_model_api::WaterBudget;

#[test]
fn exact_and_interval_censoring_score_deterministically() -> Result<(), String> {
    let exact = score_crossing_time(
        CensoredTime::Exact { time_s: 120.0 },
        CensoredTime::Exact { time_s: 100.0 },
    )
    .map_err(|e| e.to_string())?;
    assert_eq!(
        exact,
        TimingScore::ExactError {
            signed_error_s: 20.0
        }
    );

    let inside = score_crossing_time(
        CensoredTime::Exact { time_s: 115.0 },
        CensoredTime::Interval {
            start_s: 100.0,
            end_s: 130.0,
        },
    )
    .map_err(|e| e.to_string())?;
    assert_eq!(
        inside,
        TimingScore::IntervalDistance {
            signed_distance_s: 0.0,
            interval_width_s: 30.0,
        }
    );

    let early = score_crossing_time(
        CensoredTime::Exact { time_s: 85.0 },
        CensoredTime::Interval {
            start_s: 100.0,
            end_s: 130.0,
        },
    )
    .map_err(|e| e.to_string())?;
    assert_eq!(
        early,
        TimingScore::IntervalDistance {
            signed_distance_s: -15.0,
            interval_width_s: 30.0,
        }
    );
    Ok(())
}

#[test]
fn left_and_right_censoring_never_invent_exact_errors() -> Result<(), String> {
    let ok_left = score_crossing_time(
        CensoredTime::Exact { time_s: 50.0 },
        CensoredTime::LeftCensored {
            upper_bound_s: 60.0,
        },
    )
    .map_err(|e| e.to_string())?;
    assert_eq!(ok_left, TimingScore::ConsistentWithCensoring);

    let late_left = score_crossing_time(
        CensoredTime::Exact { time_s: 75.0 },
        CensoredTime::LeftCensored {
            upper_bound_s: 60.0,
        },
    )
    .map_err(|e| e.to_string())?;
    assert_eq!(
        late_left,
        TimingScore::ViolatesCensoring {
            signed_violation_s: 15.0
        }
    );

    let missing = score_crossing_time(CensoredTime::Exact { time_s: 75.0 }, CensoredTime::Missing)
        .map_err(|e| e.to_string())?;
    assert_eq!(
        missing,
        TimingScore::Unscorable {
            reason: "observation-missing"
        }
    );
    Ok(())
}

#[test]
fn depth_series_metrics_and_budget_assessment_work() -> Result<(), String> {
    let metrics = score_depth_series(&[(0.20, 0.10), (0.40, 0.50)]).map_err(|e| e.to_string())?;
    assert_eq!(metrics.count, 2);
    assert!(metrics.bias_m.abs() < 1e-12);
    assert!((metrics.mae_m - 0.10).abs() < 1e-12);
    assert!((metrics.rmse_m - 0.10).abs() < 1e-12);
    assert!((metrics.peak_depth_error_m - (-0.10)).abs() < 1e-12);
    assert!(score_depth_series(&[]).is_err());

    let passing = assess_water_budget(WaterBudget {
        storage_change_m3: 1.0,
        inputs_m3: 1.0,
        outputs_m3: 0.0,
        adjustment_m3: 0.0,
        residual_m3: 1e-12,
        residual_bound_m3: 1e-9,
        adjustment_used_m3: 0.0,
        adjustment_limit_m3: 1e-9,
    });
    assert!(passing.passed);

    let failing = assess_water_budget(WaterBudget {
        storage_change_m3: 1.0,
        inputs_m3: 1.0,
        outputs_m3: 0.0,
        adjustment_m3: 0.0,
        residual_m3: 1e-4,
        residual_bound_m3: 1e-9,
        adjustment_used_m3: 2e-9,
        adjustment_limit_m3: 1e-9,
    });
    assert!(!failing.passed);
    assert_eq!(failing.reasons.len(), 2);
    Ok(())
}
