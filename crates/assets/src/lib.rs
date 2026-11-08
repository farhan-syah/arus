//! Content-addressed raw assets and atomic acquisition metadata.

mod archive;
mod blob;
mod error;
mod paths;

pub use archive::{Archive, ArchiveGuard};
pub use blob::{BlobRef, MAX_BLOB_BYTES};
pub use error::{Error, Result};
