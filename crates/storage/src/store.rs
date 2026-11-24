//! Append-only run-manifest persistence over `arus_assets::Archive`.

use std::path::Path;

use arus_assets::{Archive, BlobRef};
use arus_domain::Provenance;
use serde::{Deserialize, Serialize};

const RUN_COLLECTION: &str = "runs";
const INDEX_RELATIVE_PATH: &str = "state/runs-index.json";
const PUBLISHED_RELATIVE_PATH: &str = "state/published-current.json";

/// Typed errors returned by run storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("invalid run record: {0}")]
    InvalidRecord(String),
    #[error("archive error: {0}")]
    Archive(#[from] arus_assets::Error),
}

/// Result alias for storage operations.
pub type Result<T> = std::result::Result<T, StorageError>;

/// Immutable run manifest recorded in the archive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: String,
    pub provenance: Provenance,
    pub accepted_time_s: f64,
    pub conservation_passed: bool,
    pub checkpoint_blob: Option<BlobRef>,
    pub outlook_blob: Option<BlobRef>,
}

/// Atomic index tracking recorded runs and the latest run ID.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunIndex {
    pub latest_run_id: Option<String>,
    pub run_ids: Vec<String>,
}

/// Atomic pointer to the currently published operational forecast cycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishedPointer {
    pub cycle_id: String,
    pub cycle_sequence: u64,
    pub run_id: String,
    pub valid_from_unix_s: i64,
    pub expires_at_unix_s: i64,
    pub supersedes_cycle_id: Option<String>,
}

/// Durable run store backed by an immutable `arus_assets::Archive`.
#[derive(Debug)]
pub struct RunStore<'a> {
    archive: &'a Archive,
}

impl<'a> RunStore<'a> {
    /// Opens a run store over `archive`.
    pub fn new(archive: &'a Archive) -> Self {
        Self { archive }
    }

    /// Appends an immutable `RunRecord` receipt and updates the atomic run index.
    ///
    /// Rejects duplicate `run_id` values without overwriting prior receipts.
    pub fn record_run(&self, record: &RunRecord) -> Result<()> {
        if record.run_id.trim().is_empty() {
            return Err(StorageError::InvalidRecord(
                "run_id must be non-empty".to_owned(),
            ));
        }
        if !record.accepted_time_s.is_finite() {
            return Err(StorageError::InvalidRecord(format!(
                "accepted_time_s={} must be finite",
                record.accepted_time_s
            )));
        }
        let _guard = self.archive.lock()?;
        self.archive
            .append_receipt(RUN_COLLECTION, &record.run_id, record)?;
        let mut index = self.load_index()?;
        index.latest_run_id = Some(record.run_id.clone());
        index.run_ids.push(record.run_id.clone());
        self.archive
            .write_json_atomic(Path::new(INDEX_RELATIVE_PATH), &index)?;
        Ok(())
    }

    /// Reads a recorded `RunRecord` by `run_id`, returning `None` if absent.
    pub fn get_run(&self, run_id: &str) -> Result<Option<RunRecord>> {
        let relative = Path::new("receipts")
            .join(RUN_COLLECTION)
            .join(format!("{run_id}.json"));
        Ok(self.archive.read_json(&relative)?)
    }

    /// Reads the current run index, returning an empty index when none exists yet.
    pub fn load_index(&self) -> Result<RunIndex> {
        Ok(self
            .archive
            .read_json(Path::new(INDEX_RELATIVE_PATH))?
            .unwrap_or_default())
    }

    /// Reads the currently published operational forecast pointer, if any.
    pub fn load_published(&self) -> Result<Option<PublishedPointer>> {
        Ok(self.archive.read_json(Path::new(PUBLISHED_RELATIVE_PATH))?)
    }

    /// Atomically advances the published pointer, refusing non-monotonic cycle sequences.
    pub fn set_published(&self, pointer: &PublishedPointer) -> Result<()> {
        let _guard = self.archive.lock()?;
        if let Some(current) = self.load_published()?
            && pointer.cycle_sequence <= current.cycle_sequence
        {
            return Err(StorageError::InvalidRecord(format!(
                "cycle_sequence {} does not advance beyond published {}",
                pointer.cycle_sequence, current.cycle_sequence
            )));
        }
        self.archive
            .write_json_atomic(Path::new(PUBLISHED_RELATIVE_PATH), pointer)?;
        Ok(())
    }
}
