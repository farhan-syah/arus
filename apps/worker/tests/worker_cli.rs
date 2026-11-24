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
        open_faces: Vec::new(),
        forcing: vec![RainIntervalSpec {
            start_s: 0.0,
            end_s: 30.0,
            uniform_rain_m_per_s: 5e-5,
            boundaries: Vec::new(),
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
        coastal: None,
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

#[test]
fn worker_compare_runs_paired_baseline_vs_pervious_intervention() -> Result<(), String> {
    use arus_domain::{GridSpec, RunClassification, SpatialReference};
    use arus_geo::TerrainGrid;
    use arus_hydraulics::{LandSpec, ProbeSpec, RainIntervalSpec, SurfaceRunSpec};

    let temp = TempDir::new().map_err(|e| e.to_string())?;
    let terrain = TerrainGrid::new(
        GridSpec {
            columns: 2,
            rows: 2,
            dx_m: 10.0,
            dy_m: 10.0,
        },
        SpatialReference {
            projected_crs: "EPSG:3375".to_owned(),
            vertical_datum: "NGVD-MY".to_owned(),
            origin_m: [0.0, 0.0],
        },
        "paired comparison test terrain",
        vec![Some(0.0); 4],
    )
    .map_err(|e| e.to_string())?;

    let base_spec = SurfaceRunSpec {
        run_id: "baseline-impervious".to_owned(),
        classification: RunClassification::NumericalDemonstration,
        terrain: terrain.clone(),
        uniform_manning_n: 0.03,
        land: LandSpec::UniformImpervious,
        initial_depth_m: 0.0,
        open_faces: Vec::new(),
        forcing: vec![RainIntervalSpec {
            start_s: 0.0,
            end_s: 30.0,
            uniform_rain_m_per_s: 5e-5,
            boundaries: Vec::new(),
        }],
        target_time_s: 30.0,
        max_steps: 10_000,
        probes: vec![ProbeSpec {
            id: "site-center".to_owned(),
            x_m: 5.0,
            y_m: 5.0,
        }],
        threshold_m: 0.0005,
        hysteresis_m: 0.0,
        recession_persistence_s: 5.0,
        coastal: None,
    };

    let mut int_spec = base_spec.clone();
    int_spec.run_id = "intervention-pervious".to_owned();
    int_spec.land = LandSpec::UniformModifiedGreenAmpt {
        suction_m: 0.11,
        conductivity_m_per_s: 3.0e-6,
        maximum_deficit: 0.25,
    };

    let base_path = temp.path().join("baseline.json");
    let int_path = temp.path().join("intervention.json");
    std::fs::write(
        &base_path,
        serde_json::to_vec_pretty(&base_spec).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        &int_path,
        serde_json::to_vec_pretty(&int_spec).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    let output = Command::new(env!("CARGO_BIN_EXE_arus-worker"))
        .args([
            "compare",
            "--baseline",
            base_path
                .to_str()
                .ok_or_else(|| "non-utf8 path".to_owned())?,
            "--intervention",
            int_path
                .to_str()
                .ok_or_else(|| "non-utf8 path".to_owned())?,
        ])
        .output()
        .map_err(|e| e.to_string())?;
    assert!(
        output.status.success(),
        "worker compare failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    assert_eq!(report["conservation_valid"], true);
    assert_eq!(
        report["improved_locations"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        report["worsened_locations"].as_array().map(Vec::len),
        Some(0)
    );
    Ok(())
}

#[test]
fn worker_resume_verifies_archived_checkpoint_and_publish_gates_demonstrations()
-> Result<(), String> {
    let temp = TempDir::new().map_err(|e| e.to_string())?;
    let archive_arg = temp
        .path()
        .to_str()
        .ok_or_else(|| "non-utf8 temp path".to_owned())?;

    let outlook_out = Command::new(env!("CARGO_BIN_EXE_arus-worker"))
        .args([
            "outlook",
            "--run-id",
            "resume-run-1",
            "--archive",
            archive_arg,
        ])
        .output()
        .map_err(|e| e.to_string())?;
    assert!(outlook_out.status.success());

    let resume_out = Command::new(env!("CARGO_BIN_EXE_arus-worker"))
        .args([
            "resume",
            "--archive",
            archive_arg,
            "--from-run",
            "resume-run-1",
        ])
        .output()
        .map_err(|e| e.to_string())?;
    assert!(
        resume_out.status.success(),
        "resume failed: {}",
        String::from_utf8_lossy(&resume_out.stderr)
    );
    let resume_json: serde_json::Value =
        serde_json::from_slice(&resume_out.stdout).map_err(|e| e.to_string())?;
    assert_eq!(resume_json["bitwise_match"], true);
    assert_eq!(resume_json["accepted_time_s"], 60.0);

    // Numerical demonstrations MUST be refused by the publication gate even with approval flags.
    let pub_out = Command::new(env!("CARGO_BIN_EXE_arus-worker"))
        .args([
            "publish",
            "--archive",
            archive_arg,
            "--run-id",
            "resume-run-1",
            "--cycle-id",
            "cycle-1",
            "--cycle-sequence",
            "1",
            "--valid-from-unix-s",
            "1000",
            "--expires-at-unix-s",
            "5000",
            "--now-unix-s",
            "2000",
            "--scientific-approved",
            "--owner-approved",
        ])
        .output()
        .map_err(|e| e.to_string())?;
    assert!(!pub_out.status.success());
    let pub_json: serde_json::Value =
        serde_json::from_slice(&pub_out.stdout).map_err(|e| e.to_string())?;
    assert_eq!(pub_json["status"], "refused");
    Ok(())
}

#[test]
fn worker_runoff_and_coupled_subcommands_execute_benchmark_manifests() -> Result<(), String> {
    let manifests =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../benchmarks/manifests");
    let runoff_path = manifests.join("subcatchment_urban_runoff.json");
    let runoff_arg = runoff_path
        .to_str()
        .ok_or_else(|| "non-utf8 runoff path".to_owned())?;
    let runoff_out = Command::new(env!("CARGO_BIN_EXE_arus-worker"))
        .args(["runoff", "--spec", runoff_arg])
        .output()
        .map_err(|e| e.to_string())?;
    assert!(
        runoff_out.status.success(),
        "runoff failed: {}",
        String::from_utf8_lossy(&runoff_out.stderr)
    );
    let runoff_json: serde_json::Value =
        serde_json::from_slice(&runoff_out.stdout).map_err(|e| e.to_string())?;
    assert_eq!(runoff_json["run_id"], "bench-subcatchment-urban-runoff");
    assert_eq!(runoff_json["final_time_s"], 120.0);
    assert!(
        runoff_json["total_outlet_m3"]
            .as_f64()
            .is_some_and(|v| v > 0.0)
    );

    let coupled_path = manifests.join("coupled_surface_channel_bank_weir.json");
    let coupled_arg = coupled_path
        .to_str()
        .ok_or_else(|| "non-utf8 coupled path".to_owned())?;
    let coupled_out = Command::new(env!("CARGO_BIN_EXE_arus-worker"))
        .args(["coupled", "--spec", coupled_arg])
        .output()
        .map_err(|e| e.to_string())?;
    assert!(
        coupled_out.status.success(),
        "coupled failed: {}",
        String::from_utf8_lossy(&coupled_out.stderr)
    );
    let coupled_json: serde_json::Value =
        serde_json::from_slice(&coupled_out.stdout).map_err(|e| e.to_string())?;
    assert_eq!(coupled_json["budget_assessment"]["passed"], true);
    assert!(
        coupled_json["report"]["bank_exchanges"][0]["surface_to_channel_m3"]
            .as_f64()
            .is_some_and(|v| v > 0.0)
    );
    Ok(())
}
