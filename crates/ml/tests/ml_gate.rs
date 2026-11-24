use arus_ml::{BaselineComparison, MlEligibility, MlGateConfig, evaluate_ml_eligibility};

#[test]
fn ml_gate_requires_locked_event_improvement_over_baseline() -> Result<(), String> {
    let config = MlGateConfig {
        min_rmse_improvement_m: 0.02,
        min_locked_events: 3,
    };

    let rejected = evaluate_ml_eligibility(
        BaselineComparison {
            locked_event_count: 1,
            baseline_rmse_m: 0.15,
            ml_rmse_m: 0.14,
        },
        config,
    )
    .map_err(|e| e.to_string())?;
    assert!(matches!(rejected, MlEligibility::Rejected { reasons } if reasons.len() == 2));

    let eligible = evaluate_ml_eligibility(
        BaselineComparison {
            locked_event_count: 4,
            baseline_rmse_m: 0.15,
            ml_rmse_m: 0.11,
        },
        config,
    )
    .map_err(|e| e.to_string())?;
    assert_eq!(eligible, MlEligibility::Eligible);
    Ok(())
}
