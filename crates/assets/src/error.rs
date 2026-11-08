use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Archive IO failed at {path}: {source}. Check the path and filesystem permissions.")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Archive JSON failed at {path}: {source}. Restore valid metadata before retrying.")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error(
        "Archive path rejected at {path}: {reason}. Use a relative path without symlinks or traversal."
    )]
    Path { path: PathBuf, reason: &'static str },
    #[error("Archive lock is busy at {path}. Wait for the active collector to finish.")]
    Locked { path: PathBuf },
    #[error(
        "Archive blob checksum differs at {path}. Quarantine the corrupted file before retrying."
    )]
    CorruptBlob { path: PathBuf },
    #[error("Archive receipt already exists at {path}. Use a new receipt identifier.")]
    ReceiptExists { path: PathBuf },
    #[error(
        "Archive metadata exceeds {limit} bytes at {path}. Split the metadata before retrying."
    )]
    MetadataLimit { path: PathBuf, limit: u64 },
    #[error("Archive blob exceeds {limit} bytes at {path}. Split the asset before reading it.")]
    BlobLimit { path: PathBuf, limit: u64 },
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Error {
    Error::Io {
        path: path.into(),
        source,
    }
}
