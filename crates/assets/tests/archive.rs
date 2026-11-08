use std::error::Error;
use std::fs;
use std::path::Path;

use arus_assets::{Archive, Error as ArchiveError, MAX_BLOB_BYTES};
use serde_json::json;

#[test]
fn repeated_payloads_share_one_checked_blob() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let archive = Archive::open(directory.path())?;
    let first = archive.put_blob(b"rainfall")?;
    let second = archive.put_blob(b"rainfall")?;
    assert_eq!(first, second);
    assert_eq!(archive.read_blob(&first.sha256)?, b"rainfall");
    let blob = archive
        .root()
        .join("blobs")
        .join(&first.sha256[..2])
        .join(&first.sha256);
    fs::write(blob, b"tampered")?;
    assert!(matches!(
        archive.put_blob(b"rainfall"),
        Err(ArchiveError::CorruptBlob { .. })
    ));
    assert!(matches!(
        archive.read_blob(&first.sha256),
        Err(ArchiveError::CorruptBlob { .. })
    ));
    Ok(())
}

#[test]
fn oversized_blob_reads_stop_before_allocation() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let archive = Archive::open(directory.path())?;
    let reference = archive.put_blob(b"sparse")?;
    let path = archive
        .root()
        .join("blobs")
        .join(&reference.sha256[..2])
        .join(&reference.sha256);
    fs::OpenOptions::new()
        .write(true)
        .open(path)?
        .set_len(MAX_BLOB_BYTES + 1)?;
    assert!(matches!(
        archive.read_blob(&reference.sha256),
        Err(ArchiveError::BlobLimit { .. })
    ));
    Ok(())
}

#[test]
fn metadata_replacement_preserves_complete_json() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let archive = Archive::open(directory.path())?;
    let relative = Path::new("state/source.json");
    assert_eq!(archive.read_json::<serde_json::Value>(relative)?, None);
    archive.write_json_atomic(relative, &json!({"cycle": 1}))?;
    archive.write_json_atomic(relative, &json!({"cycle": 2, "complete": true}))?;
    assert_eq!(
        archive.read_json::<serde_json::Value>(relative)?,
        Some(json!({"cycle": 2, "complete": true}))
    );
    struct Rejected;
    impl serde::Serialize for Rejected {
        fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("rejected metadata"))
        }
    }
    assert!(archive.write_json_atomic(relative, &Rejected).is_err());
    assert_eq!(
        archive.read_json::<serde_json::Value>(relative)?,
        Some(json!({"cycle": 2, "complete": true}))
    );
    Ok(())
}

#[test]
fn archive_paths_reject_traversal_and_absolute_names() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let archive = Archive::open(directory.path())?;
    for path in [
        "../outside.json",
        "/outside.json",
        "state/../../outside.json",
        "",
    ] {
        assert!(matches!(
            archive.write_json_atomic(Path::new(path), &json!({})),
            Err(ArchiveError::Path { .. })
        ));
    }
    assert!(archive.read_blob("../outside").is_err());
    assert!(
        archive
            .append_receipt("../outside", "receipt", &json!({}))
            .is_err()
    );
    Ok(())
}

#[test]
fn active_archive_lock_rejects_another_collector() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let archive = Archive::open(directory.path())?;
    let guard = archive.lock()?;
    assert!(matches!(archive.lock(), Err(ArchiveError::Locked { .. })));
    drop(guard);
    let _guard = archive.lock()?;
    Ok(())
}

#[test]
fn receipts_cannot_replace_prior_records() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let archive = Archive::open(directory.path())?;
    archive.append_receipt("rain", "receipt_1", &json!({"status": "success"}))?;
    assert!(matches!(
        archive.append_receipt("rain", "receipt_1", &json!({"status": "error"})),
        Err(ArchiveError::ReceiptExists { .. })
    ));
    assert_eq!(
        archive.read_json::<serde_json::Value>(Path::new("receipts/rain/receipt_1.json"))?,
        Some(json!({"status": "success"}))
    );
    for path in [
        "receipts/rain/receipt_1.json",
        "blobs/payload",
        "archive.lock",
    ] {
        assert!(matches!(
            archive.write_json_atomic(Path::new(path), &json!({})),
            Err(ArchiveError::Path { .. })
        ));
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn relative_symlinks_cannot_escape_archive() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let archive = Archive::open(directory.path())?;
    symlink(outside.path(), archive.root().join("state"))?;
    assert!(matches!(
        archive.write_json_atomic(Path::new("state/value.json"), &json!({})),
        Err(ArchiveError::Path { .. })
    ));
    assert!(!outside.path().join("value.json").exists());
    Ok(())
}
