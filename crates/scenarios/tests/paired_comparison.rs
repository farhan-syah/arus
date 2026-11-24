use std::collections::BTreeMap;

use arus_model_api::WaterBudget;
use arus_scenarios::{ScenarioManifest, ScenarioRunSummary, compare_paired_runs};

fn valid_budget() -> WaterBudget {
    WaterBudget {
        storage_change_m3: 10.0,
        inputs_m3: 10.0,
        outputs_m3: 0.0,
        adjustment_m3: 0.0,
        residual_m3: 1e-12,
        residual_bound_m3: 1e-9,
        adjustment_used_m3: 0.0,
        adjustment_limit_m3: 1e-9,
    }
}

fn manifest(id: &str, forcing: &str) -> ScenarioManifest {
    ScenarioManifest {
        scenario_id: id.to_owned(),
        baseline_id: "base-0".to_owned(),
        forcing_digest: forcing.to_owned(),
        initial_state_digest: "init-sha256".to_owned(),
        is_screening: true,
        assumptions: vec!["constant design storm".to_owned()],
        edits: vec!["add detention bund".to_owned()],
    }
}

#[test]
fn paired_comparison_flags_transferred_downstream_impacts() -> Result<(), String> {
    let baseline = ScenarioRunSummary {
        manifest: manifest("base-0", "forcing-1"),
        budget: valid_budget(),
        location_peak_depths_m: BTreeMap::from([
            ("upstream".to_owned(), 0.50),
            ("downstream".to_owned(), 0.20),
        ]),
    };
    let intervention = ScenarioRunSummary {
        manifest: manifest("bund-1", "forcing-1"),
        budget: valid_budget(),
        location_peak_depths_m: BTreeMap::from([
            ("upstream".to_owned(), 0.30),
            ("downstream".to_owned(), 0.35),
        ]),
    };

    let report = compare_paired_runs(&baseline, &intervention).map_err(|e| e.to_string())?;
    assert!(report.conservation_valid);
    assert!(report.is_screening);
    assert_eq!(report.improved_locations.len(), 1);
    assert_eq!(report.improved_locations[0].location_id, "upstream");
    assert_eq!(report.worsened_locations.len(), 1);
    assert_eq!(report.worsened_locations[0].location_id, "downstream");
    assert!((report.worsened_locations[0].delta_m - 0.15).abs() < 1e-12);
    Ok(())
}

#[test]
fn unmatched_forcing_digest_is_rejected() {
    let baseline = ScenarioRunSummary {
        manifest: manifest("base-0", "forcing-1"),
        budget: valid_budget(),
        location_peak_depths_m: BTreeMap::from([("site".to_owned(), 0.50)]),
    };
    let intervention = ScenarioRunSummary {
        manifest: manifest("bund-1", "forcing-2"),
        budget: valid_budget(),
        location_peak_depths_m: BTreeMap::from([("site".to_owned(), 0.30)]),
    };
    assert!(compare_paired_runs(&baseline, &intervention).is_err());
}
