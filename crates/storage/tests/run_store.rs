use arus_assets::Archive;
use arus_domain::{Provenance, RunClassification};
use arus_storage::{RunRecord, RunStore};
use tempfile::TempDir;

fn sample_record(run_id: &str) -> RunRecord {
    RunRecord {
        run_id: run_id.to_owned(),
        provenance: Provenance {
            backend: "hydr".to_owned(),
            contract: "hydr-swe-numr-cpu-2".to_owned(),
            classification: RunClassification::NumericalDemonstration,
            note: "synthetic archive test".to_owned(),
        },
        accepted_time_s: 60.0,
        conservation_passed: true,
        checkpoint_blob: None,
        outlook_blob: None,
    }
}

#[test]
fn run_store_persists_immutably_and_rejects_duplicate_ids() -> Result<(), String> {
    let temp = TempDir::new().map_err(|e| e.to_string())?;
    let archive = Archive::open(temp.path()).map_err(|e| e.to_string())?;
    let store = RunStore::new(&archive);

    let first = sample_record("run-001");
    store.record_run(&first).map_err(|e| e.to_string())?;
    assert!(store.record_run(&first).is_err());

    let second = sample_record("run-002");
    store.record_run(&second).map_err(|e| e.to_string())?;

    assert_eq!(
        store.get_run("run-001").map_err(|e| e.to_string())?,
        Some(first)
    );
    assert_eq!(store.get_run("missing").map_err(|e| e.to_string())?, None);

    let index = store.load_index().map_err(|e| e.to_string())?;
    assert_eq!(index.latest_run_id.as_deref(), Some("run-002"));
    assert_eq!(index.run_ids, vec!["run-001", "run-002"]);
    Ok(())
}

#[test]
fn published_pointer_enforces_monotonic_sequence() -> Result<(), String> {
    use arus_storage::PublishedPointer;

    let temp = TempDir::new().map_err(|e| e.to_string())?;
    let archive = Archive::open(temp.path()).map_err(|e| e.to_string())?;
    let store = RunStore::new(&archive);
    assert_eq!(store.load_published().map_err(|e| e.to_string())?, None);

    let p1 = PublishedPointer {
        cycle_id: "cycle-1".to_owned(),
        cycle_sequence: 1,
        run_id: "run-1".to_owned(),
        valid_from_unix_s: 100,
        expires_at_unix_s: 500,
        supersedes_cycle_id: None,
    };
    store.set_published(&p1).map_err(|e| e.to_string())?;
    assert!(store.set_published(&p1).is_err());

    let p2 = PublishedPointer {
        cycle_id: "cycle-2".to_owned(),
        cycle_sequence: 2,
        run_id: "run-2".to_owned(),
        valid_from_unix_s: 200,
        expires_at_unix_s: 600,
        supersedes_cycle_id: Some("cycle-1".to_owned()),
    };
    store.set_published(&p2).map_err(|e| e.to_string())?;
    assert_eq!(store.load_published().map_err(|e| e.to_string())?, Some(p2));
    Ok(())
}
