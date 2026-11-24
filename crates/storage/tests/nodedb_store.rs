use arus_domain::{Provenance, RunClassification};
use arus_storage::{NodeDbRunStore, PublishedPointer, RunRecord};
use tempfile::TempDir;

fn sample_record(run_id: &str) -> RunRecord {
    RunRecord {
        run_id: run_id.to_owned(),
        provenance: Provenance {
            backend: "hydr".to_owned(),
            contract: "hydr-swe-numr-cpu-2".to_owned(),
            classification: RunClassification::NumericalDemonstration,
            note: "nodedb-lite durability test".to_owned(),
        },
        accepted_time_s: 60.0,
        conservation_passed: true,
        checkpoint_blob: None,
        outlook_blob: None,
    }
}

#[tokio::test]
async fn nodedb_in_memory_enforces_immutability_and_monotonic_publish() -> Result<(), String> {
    let store = NodeDbRunStore::open_in_memory()
        .await
        .map_err(|e| e.to_string())?;
    let r1 = sample_record("ndb-run-1");
    store.record_run(&r1).await.map_err(|e| e.to_string())?;
    assert!(store.record_run(&r1).await.is_err());

    assert_eq!(
        store
            .get_run("ndb-run-1")
            .await
            .map_err(|e| e.to_string())?,
        Some(r1)
    );
    let idx = store.load_index().await.map_err(|e| e.to_string())?;
    assert_eq!(idx.latest_run_id.as_deref(), Some("ndb-run-1"));

    let p1 = PublishedPointer {
        cycle_id: "c1".to_owned(),
        cycle_sequence: 1,
        run_id: "ndb-run-1".to_owned(),
        valid_from_unix_s: 100,
        expires_at_unix_s: 500,
        supersedes_cycle_id: None,
    };
    store.set_published(&p1).await.map_err(|e| e.to_string())?;
    assert!(store.set_published(&p1).await.is_err());
    Ok(())
}

#[tokio::test]
async fn nodedb_on_disk_persists_across_reopen() -> Result<(), String> {
    let temp = TempDir::new().map_err(|e| e.to_string())?;
    let db_path = temp.path().join("arus.nodedb");
    let r1 = sample_record("disk-run-1");
    let p1 = PublishedPointer {
        cycle_id: "c1".to_owned(),
        cycle_sequence: 1,
        run_id: "disk-run-1".to_owned(),
        valid_from_unix_s: 100,
        expires_at_unix_s: 500,
        supersedes_cycle_id: None,
    };

    {
        let store = NodeDbRunStore::open(&db_path)
            .await
            .map_err(|e| e.to_string())?;
        store.record_run(&r1).await.map_err(|e| e.to_string())?;
        store.set_published(&p1).await.map_err(|e| e.to_string())?;
    }

    let reopened = NodeDbRunStore::open(&db_path)
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(
        reopened
            .get_run("disk-run-1")
            .await
            .map_err(|e| e.to_string())?,
        Some(r1)
    );
    assert_eq!(
        reopened.load_published().await.map_err(|e| e.to_string())?,
        Some(p1)
    );
    Ok(())
}
