//! Embedded NodeDB Lite backend for ARUS run manifests, indices, and published pointers.
//!
//! Uses `nodedb-lite` with manual deterministic flushing (`auto_flush_ms = 0`) so committed
//! records survive process restart and reopen without exposing database internals upstream.

use std::path::Path;
use std::sync::Arc;

use nodedb_client::{Document, NodeDb, Value};
use nodedb_lite::{Encryption, LiteConfig, NodeDbLite, PagedbStorageDefault, PagedbStorageMem};

use crate::store::{PublishedPointer, Result, RunIndex, RunRecord, StorageError};

const RUNS_COLLECTION: &str = "arus_runs";
const META_COLLECTION: &str = "arus_meta";
const INDEX_DOC_ID: &str = "runs-index";
const PUBLISHED_DOC_ID: &str = "published-current";
const PAYLOAD_FIELD: &str = "json";

enum Backend {
    Disk(Arc<NodeDbLite<PagedbStorageDefault>>),
    Memory(Arc<NodeDbLite<PagedbStorageMem>>),
}

impl Backend {
    async fn document_put(
        &self,
        collection: &str,
        doc: Document,
    ) -> std::result::Result<(), String> {
        match self {
            Self::Disk(db) => db
                .document_put(collection, doc)
                .await
                .map_err(|e| e.to_string()),
            Self::Memory(db) => db
                .document_put(collection, doc)
                .await
                .map_err(|e| e.to_string()),
        }
    }

    async fn document_get(
        &self,
        collection: &str,
        id: &str,
    ) -> std::result::Result<Option<Document>, String> {
        match self {
            Self::Disk(db) => db
                .document_get(collection, id)
                .await
                .map_err(|e| e.to_string()),
            Self::Memory(db) => db
                .document_get(collection, id)
                .await
                .map_err(|e| e.to_string()),
        }
    }

    async fn flush(&self) -> std::result::Result<(), String> {
        match self {
            Self::Disk(db) => db.flush().await.map_err(|e| e.to_string()),
            Self::Memory(db) => db.flush().await.map_err(|e| e.to_string()),
        }
    }
}

/// Embedded NodeDB Lite store for ARUS run metadata and monotonic publication pointers.
pub struct NodeDbRunStore {
    db: Backend,
}

impl NodeDbRunStore {
    /// Opens or creates a persistent on-disk NodeDB Lite store at `path`.
    pub async fn open(path: &Path) -> Result<Self> {
        let storage = PagedbStorageDefault::open(path, Encryption::Plaintext)
            .await
            .map_err(|error| StorageError::InvalidRecord(format!("nodedb open failed: {error}")))?;
        let config = LiteConfig {
            auto_flush_ms: 0,
            ..LiteConfig::default()
        };
        let db = NodeDbLite::open_with_config(storage, config)
            .await
            .map_err(|error| StorageError::InvalidRecord(format!("nodedb init failed: {error}")))?;
        Ok(Self {
            db: Backend::Disk(db),
        })
    }

    /// Opens an in-memory NodeDB Lite store for ephemeral testing.
    pub async fn open_in_memory() -> Result<Self> {
        let storage = PagedbStorageMem::open_in_memory()
            .await
            .map_err(|error| StorageError::InvalidRecord(format!("nodedb mem open: {error}")))?;
        let config = LiteConfig {
            auto_flush_ms: 0,
            ..LiteConfig::default()
        };
        let db = NodeDbLite::open_with_config(storage, config)
            .await
            .map_err(|error| StorageError::InvalidRecord(format!("nodedb init failed: {error}")))?;
        Ok(Self {
            db: Backend::Memory(db),
        })
    }

    /// Appends an immutable `RunRecord`, updates `RunIndex`, and flushes to storage.
    ///
    /// Rejects duplicate `run_id` values without overwriting prior records.
    pub async fn record_run(&self, record: &RunRecord) -> Result<()> {
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
        if self.get_run(&record.run_id).await?.is_some() {
            return Err(StorageError::InvalidRecord(format!(
                "run_id {:?} already exists in NodeDB store",
                record.run_id
            )));
        }
        let doc = encode_json_doc(&record.run_id, record)?;
        self.db
            .document_put(RUNS_COLLECTION, doc)
            .await
            .map_err(|error| StorageError::InvalidRecord(format!("nodedb put run: {error}")))?;

        let mut index = self.load_index().await?;
        index.latest_run_id = Some(record.run_id.clone());
        index.run_ids.push(record.run_id.clone());
        let index_doc = encode_json_doc(INDEX_DOC_ID, &index)?;
        self.db
            .document_put(META_COLLECTION, index_doc)
            .await
            .map_err(|error| StorageError::InvalidRecord(format!("nodedb put index: {error}")))?;

        self.db
            .flush()
            .await
            .map_err(|error| StorageError::InvalidRecord(format!("nodedb flush: {error}")))?;
        Ok(())
    }

    /// Reads a recorded `RunRecord` by `run_id`, returning `None` if absent.
    pub async fn get_run(&self, run_id: &str) -> Result<Option<RunRecord>> {
        let doc = self
            .db
            .document_get(RUNS_COLLECTION, run_id)
            .await
            .map_err(|error| StorageError::InvalidRecord(format!("nodedb get run: {error}")))?;
        doc.map(|d| decode_json_doc(&d)).transpose()
    }

    /// Reads the current `RunIndex`, returning an empty index when none exists yet.
    pub async fn load_index(&self) -> Result<RunIndex> {
        let doc = self
            .db
            .document_get(META_COLLECTION, INDEX_DOC_ID)
            .await
            .map_err(|error| StorageError::InvalidRecord(format!("nodedb get index: {error}")))?;
        match doc {
            Some(d) => decode_json_doc(&d),
            None => Ok(RunIndex::default()),
        }
    }

    /// Reads the currently published operational forecast pointer, if any.
    pub async fn load_published(&self) -> Result<Option<PublishedPointer>> {
        let doc = self
            .db
            .document_get(META_COLLECTION, PUBLISHED_DOC_ID)
            .await
            .map_err(|error| {
                StorageError::InvalidRecord(format!("nodedb get published: {error}"))
            })?;
        doc.map(|d| decode_json_doc(&d)).transpose()
    }

    /// Atomically advances the published pointer, refusing non-monotonic cycle sequences.
    pub async fn set_published(&self, pointer: &PublishedPointer) -> Result<()> {
        if let Some(current) = self.load_published().await?
            && pointer.cycle_sequence <= current.cycle_sequence
        {
            return Err(StorageError::InvalidRecord(format!(
                "cycle_sequence {} does not advance beyond published {}",
                pointer.cycle_sequence, current.cycle_sequence
            )));
        }
        let doc = encode_json_doc(PUBLISHED_DOC_ID, pointer)?;
        self.db
            .document_put(META_COLLECTION, doc)
            .await
            .map_err(|error| {
                StorageError::InvalidRecord(format!("nodedb put published: {error}"))
            })?;
        self.db
            .flush()
            .await
            .map_err(|error| StorageError::InvalidRecord(format!("nodedb flush: {error}")))?;
        Ok(())
    }
}

fn encode_json_doc<T: serde::Serialize>(id: &str, value: &T) -> Result<Document> {
    let json = serde_json::to_string(value)
        .map_err(|error| StorageError::InvalidRecord(format!("serialize json: {error}")))?;
    let mut doc = Document::new(id);
    doc.set(PAYLOAD_FIELD, Value::String(json));
    Ok(doc)
}

fn decode_json_doc<T: serde::de::DeserializeOwned>(doc: &Document) -> Result<T> {
    match doc.fields.get(PAYLOAD_FIELD) {
        Some(Value::String(json)) => serde_json::from_str(json)
            .map_err(|error| StorageError::InvalidRecord(format!("deserialize json: {error}"))),
        other => Err(StorageError::InvalidRecord(format!(
            "unexpected document payload field: {other:?}"
        ))),
    }
}
