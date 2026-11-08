use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};
use tempfile::TempDir;

type TestResult = Result<(), Box<dyn Error>>;

struct Fixture {
    root: TempDir,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        fs::write(
            root.path().join("sources.json"),
            serde_json::to_vec(&json!({
                "schema_version": 1,
                "sources": [{
                    "id": "sample-source",
                    "title": "Sample source",
                    "provider": "sample-provider",
                    "description": "Catalog metadata for command checks.",
                    "data_class": "observation",
                    "access": "unknown",
                    "connector": "catalog_only",
                    "documentation_urls": ["https://example.org/docs"],
                    "license": {
                        "name": "Unconfirmed",
                        "url": "https://example.org/terms",
                        "noncommercial_only": false,
                        "redistribution": "Unconfirmed"
                    },
                    "coverage": "Unconfirmed",
                    "limitations": ["Numerical acquisition remains unavailable."],
                    "reviewed_on": "2026-11-08",
                    "refresh_seconds": null,
                    "provider_min_interval_seconds": 1,
                    "max_response_bytes": 4096
                }]
            }))?,
        )?;
        fs::write(
            root.path().join("collections.json"),
            serde_json::to_vec(&json!({
                "schema_version": 1,
                "collections": [{
                    "id": "sample-collection",
                    "source_id": "sample-source",
                    "description": "Manual collection metadata.",
                    "enabled": false,
                    "accept_noncommercial": false,
                    "parameters": {}
                }]
            }))?,
        )?;
        Ok(Self { root })
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    fn run(&self, args: &[&str]) -> Result<Output, Box<dyn Error>> {
        Ok(Command::new(env!("CARGO_BIN_EXE_arus-cli"))
            .current_dir(self.path())
            .arg("--catalog")
            .arg(self.path().join("sources.json"))
            .arg("--collections")
            .arg(self.path().join("collections.json"))
            .arg("--data-dir")
            .arg(self.path().join("archive"))
            .args(args)
            .output()?)
    }
}

#[test]
fn help_requires_no_catalog_or_archive() -> TestResult {
    let root = tempfile::tempdir()?;
    let output = Command::new(env!("CARGO_BIN_EXE_arus-cli"))
        .current_dir(root.path())
        .arg("--help")
        .output()?;
    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout)?.contains("catalog"));
    assert!(!root.path().join("data").exists());
    Ok(())
}

#[test]
fn source_list_does_not_read_collections_or_create_archive() -> TestResult {
    let fixture = Fixture::new()?;
    fs::remove_file(fixture.path().join("collections.json"))?;
    let output = fixture.run(&["catalog", "list"])?;
    assert!(output.status.success(), "{:?}", output.stderr);
    let sources: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(sources[0]["id"], "sample-source");
    assert!(!fixture.path().join("archive").exists());
    Ok(())
}

#[test]
fn catalog_check_reports_structure_without_claiming_availability() -> TestResult {
    let fixture = Fixture::new()?;
    let output = fixture.run(&["catalog", "check"])?;
    assert!(output.status.success(), "{:?}", output.stderr);
    let report: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(report["valid"], true);
    assert_eq!(report["availability_checked"], false);
    assert!(!fixture.path().join("archive").exists());
    Ok(())
}

#[test]
fn unknown_source_reports_an_actionable_error() -> TestResult {
    let fixture = Fixture::new()?;
    let output = fixture.run(&["catalog", "show", "absent-source"])?;
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("absent-source"));
    assert!(stderr.contains("catalog list"));
    assert!(!fixture.path().join("archive").exists());
    Ok(())
}

#[test]
fn unknown_collection_does_not_create_archive() -> TestResult {
    let fixture = Fixture::new()?;
    let output = fixture.run(&["data", "sync", "--collection", "absent-collection"])?;
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)?.contains("absent-collection"));
    assert!(!fixture.path().join("archive").exists());
    Ok(())
}

#[test]
fn malformed_catalog_reports_the_input_path() -> TestResult {
    let fixture = Fixture::new()?;
    fs::write(fixture.path().join("sources.json"), b"{invalid")?;
    let output = fixture.run(&["catalog", "list"])?;
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)?.contains("sources.json"));
    assert!(!fixture.path().join("archive").exists());
    Ok(())
}
