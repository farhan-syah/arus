use std::process::Command;

use tempfile::TempDir;

#[test]
fn worker_outlook_emits_valid_json_and_persists_checkpoint() -> Result<(), String> {
    let temp = TempDir::new().map_err(|e| e.to_string())?;
    let output = Command::new(env!("CARGO_BIN_EXE_arus-worker"))
        .args([
            "outlook",
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
    assert_eq!(value["classification"], "numerical-demonstration");
    assert_eq!(value["budget_assessment"]["passed"], true);
    assert!(value["checkpoint"]["blob"]["sha256"].is_string());
    assert_eq!(value["outlooks"][0]["support"], "supported");
    assert_eq!(value["outlooks"][1]["support"], "unsupported");
    Ok(())
}
