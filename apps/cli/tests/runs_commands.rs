use std::process::Command;

use arus_assets::Archive;
use arus_domain::{Provenance, RunClassification};
use arus_storage::{RunRecord, RunStore};
use tempfile::TempDir;

#[test]
fn cli_runs_list_and_show_inspect_archived_runs() -> Result<(), String> {
    let temp = TempDir::new().map_err(|e| e.to_string())?;
    let archive = Archive::open(temp.path()).map_err(|e| e.to_string())?;
    let store = RunStore::new(&archive);
    store
        .record_run(&RunRecord {
            run_id: "cli-run-1".to_owned(),
            provenance: Provenance {
                backend: "hydr".to_owned(),
                contract: "hydr-swe-numr-cpu-2".to_owned(),
                classification: RunClassification::NumericalDemonstration,
                note: "cli inspect test".to_owned(),
            },
            accepted_time_s: 60.0,
            conservation_passed: true,
            checkpoint_blob: None,
            outlook_blob: None,
        })
        .map_err(|e| e.to_string())?;

    let archive_arg = temp
        .path()
        .to_str()
        .ok_or_else(|| "non-utf8 temp path".to_owned())?;

    let list_out = Command::new(env!("CARGO_BIN_EXE_arus-cli"))
        .args(["runs", "--archive", archive_arg, "list"])
        .output()
        .map_err(|e| e.to_string())?;
    assert!(list_out.status.success());
    let list_json: serde_json::Value =
        serde_json::from_slice(&list_out.stdout).map_err(|e| e.to_string())?;
    assert_eq!(list_json["latest_run_id"], "cli-run-1");

    let show_out = Command::new(env!("CARGO_BIN_EXE_arus-cli"))
        .args(["runs", "--archive", archive_arg, "show", "cli-run-1"])
        .output()
        .map_err(|e| e.to_string())?;
    assert!(show_out.status.success());
    let show_json: serde_json::Value =
        serde_json::from_slice(&show_out.stdout).map_err(|e| e.to_string())?;
    assert_eq!(show_json["run_id"], "cli-run-1");
    assert_eq!(show_json["conservation_passed"], true);
    Ok(())
}
