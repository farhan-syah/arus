use arus_optimisation::{MitigationCandidate, MitigationConstraints, screen_candidates};
use arus_scenarios::{LocationDelta, PairedComparisonReport};

fn report(
    id: &str,
    conservation_valid: bool,
    reduction_m: f64,
    worsening_m: f64,
) -> PairedComparisonReport {
    let improved_locations = if reduction_m > 0.0 {
        vec![LocationDelta {
            location_id: "site-a".to_owned(),
            baseline_peak_m: 0.50,
            intervention_peak_m: 0.50 - reduction_m,
            delta_m: -reduction_m,
        }]
    } else {
        Vec::new()
    };
    let worsened_locations = if worsening_m > 0.0 {
        vec![LocationDelta {
            location_id: "site-b".to_owned(),
            baseline_peak_m: 0.20,
            intervention_peak_m: 0.20 + worsening_m,
            delta_m: worsening_m,
        }]
    } else {
        Vec::new()
    };
    PairedComparisonReport {
        baseline_id: "base".to_owned(),
        intervention_id: id.to_owned(),
        is_screening: true,
        conservation_valid,
        improved_locations,
        unchanged_locations: Vec::new(),
        worsened_locations,
    }
}

#[test]
fn screening_rejects_unconserved_and_extracts_pareto_front() -> Result<(), String> {
    let candidates = vec![
        MitigationCandidate {
            candidate_id: "bad-mass".to_owned(),
            cost_myr: 1.0,
            comparison: report("bad-mass", false, 0.30, 0.0),
        },
        MitigationCandidate {
            candidate_id: "bad-transfer".to_owned(),
            cost_myr: 1.0,
            comparison: report("bad-transfer", true, 0.30, 0.08),
        },
        MitigationCandidate {
            candidate_id: "cheap-ok".to_owned(),
            cost_myr: 2.0,
            comparison: report("cheap-ok", true, 0.15, 0.0),
        },
        MitigationCandidate {
            candidate_id: "dominated".to_owned(),
            cost_myr: 3.0,
            comparison: report("dominated", true, 0.10, 0.0),
        },
        MitigationCandidate {
            candidate_id: "strong-ok".to_owned(),
            cost_myr: 4.0,
            comparison: report("strong-ok", true, 0.35, 0.01),
        },
    ];

    let result = screen_candidates(
        &candidates,
        MitigationConstraints {
            max_cost_myr: 5.0,
            max_allowed_worsening_m: 0.02,
        },
    )
    .map_err(|e| e.to_string())?;

    assert_eq!(result.rejected.len(), 2);
    assert_eq!(result.feasible.len(), 3);
    assert_eq!(result.pareto_front.len(), 2);
    assert!(
        result
            .pareto_front
            .iter()
            .any(|c| c.candidate_id == "cheap-ok")
    );
    assert!(
        result
            .pareto_front
            .iter()
            .any(|c| c.candidate_id == "strong-ok")
    );
    assert!(
        result
            .pareto_front
            .iter()
            .all(|c| c.requires_full_model_check)
    );
    Ok(())
}
