use std::process::Command;

use arus_assets::Archive;
use arus_storage::RunStore;
use tempfile::TempDir;

#[test]
fn worker_outlook_emits_valid_json_and_persists_run_record() -> Result<(), String> {
    let temp = TempDir::new().map_err(|e| e.to_string())?;
    let output = Command::new(env!("CARGO_BIN_EXE_arus-worker"))
        .args([
            "outlook",
            "--run-id",
            "demo-run-42",
            "--threshold-m",
            "0.003",
            "--archive",
            temp.path()
                .to_str()
                .ok_or_else(|| "non-utf8 temp path".to_owned())?,
        ])
        .output()
        .map_err(|e| e.to_string())?;
    assert!(
        output.status.success(),
        "worker failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    assert_eq!(value["run_id"], "demo-run-42");
    assert_eq!(value["classification"], "numerical-demonstration");
    assert_eq!(value["budget_assessment"]["passed"], true);
    assert!(value["checkpoint"]["blob"]["sha256"].is_string());
    assert_eq!(value["outlooks"][0]["support"], "supported");
    assert_eq!(value["outlooks"][1]["support"], "unsupported");

    let archive = Archive::open(temp.path()).map_err(|e| e.to_string())?;
    let store = RunStore::new(&archive);
    let record = store
        .get_run("demo-run-42")
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "expected persisted RunRecord".to_owned())?;
    assert!(record.conservation_passed);
    assert!(record.checkpoint_blob.is_some());
    let outlook_blob = record
        .outlook_blob
        .ok_or_else(|| "expected persisted outlook_blob".to_owned())?;
    let raw_outlooks = archive
        .read_blob(&outlook_blob.sha256)
        .map_err(|e| e.to_string())?;
    let decoded: Vec<arus_api_types::ApiLocationOutlook> =
        serde_json::from_slice(&raw_outlooks).map_err(|e| e.to_string())?;
    assert_eq!(decoded.len(), 2);
    Ok(())
}

#[test]
fn worker_run_executes_json_spec_with_pervious_soil_and_nodata() -> Result<(), String> {
    use arus_domain::{GridSpec, RunClassification, SpatialReference};
    use arus_geo::TerrainGrid;
    use arus_hydraulics::{LandSpec, ProbeSpec, RainIntervalSpec, SurfaceRunSpec};

    let temp = TempDir::new().map_err(|e| e.to_string())?;
    let archive_dir = temp.path().join("archive");
    let spec_path = temp.path().join("run-spec.json");

    let terrain = TerrainGrid::new(
        GridSpec {
            columns: 2,
            rows: 1,
            dx_m: 10.0,
            dy_m: 10.0,
        },
        SpatialReference {
            projected_crs: "EPSG:3375".to_owned(),
            vertical_datum: "NGVD-MY".to_owned(),
            origin_m: [0.0, 0.0],
        },
        "synthetic manifest terrain",
        vec![Some(1.0), None],
    )
    .map_err(|e| e.to_string())?;

    let spec = SurfaceRunSpec {
        run_id: "manifest-run-1".to_owned(),
        classification: RunClassification::NumericalDemonstration,
        terrain,
        uniform_manning_n: 0.03,
        land: LandSpec::UniformModifiedGreenAmpt {
            suction_m: 0.11,
            conductivity_m_per_s: 3.0e-6,
            maximum_deficit: 0.25,
        },
        initial_depth_m: 0.0,
        forcing: vec![RainIntervalSpec {
            start_s: 0.0,
            end_s: 30.0,
            uniform_rain_m_per_s: 5e-5,
        }],
        target_time_s: 30.0,
        max_steps: 10_000,
        probes: vec![
            ProbeSpec {
                id: "active-cell".to_owned(),
                x_m: 5.0,
                y_m: 5.0,
            },
            ProbeSpec {
                id: "nodata-cell".to_owned(),
                x_m: 15.0,
                y_m: 5.0,
            },
        ],
        threshold_m: 0.0005,
        hysteresis_m: 0.0,
        recession_persistence_s: 5.0,
    };
    let spec_bytes = serde_json::to_vec_pretty(&spec).map_err(|e| e.to_string())?;
    std::fs::write(&spec_path, spec_bytes).map_err(|e| e.to_string())?;

    let output = Command::new(env!("CARGO_BIN_EXE_arus-worker"))
        .args([
            "run",
            "--spec",
            spec_path
                .to_str()
                .ok_or_else(|| "non-utf8 spec path".to_owned())?,
            "--archive",
            archive_dir
                .to_str()
                .ok_or_else(|| "non-utf8 archive path".to_owned())?,
        ])
        .output()
        .map_err(|e| e.to_string())?;
    assert!(
        output.status.success(),
        "worker run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    assert_eq!(value["run_id"], "manifest-run-1");
    assert_eq!(value["budget_assessment"]["passed"], true);
    assert_eq!(value["outlooks"][0]["support"], "supported");
    assert_eq!(value["outlooks"][1]["support"], "unsupported");
    Ok(())
}
