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
