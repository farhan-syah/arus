//! Durable archive-backed run-record persistence built over `arus-assets::Archive`.
//!
//! Run receipts are append-only and immutable. Latest-run pointers update atomically.

pub mod store;

pub use store::{Result, RunIndex, RunRecord, RunStore, StorageError};
