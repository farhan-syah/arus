//! Durable run-record and publication-pointer persistence over `arus-assets::Archive`
//! and embedded `nodedb-lite`.
//!
//! Run receipts are append-only and immutable. Published cycle pointers advance monotonically.

pub mod nodedb;
pub mod store;

pub use nodedb::NodeDbRunStore;
pub use store::{PublishedPointer, Result, RunIndex, RunRecord, RunStore, StorageError};
