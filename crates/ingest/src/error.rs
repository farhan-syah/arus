use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Cannot read {path}: {source}. Check the path and permissions.")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Cannot parse {path}: {source}. Correct the JSON document.")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("Catalog check rejected {0}. Correct the catalog or collection config.")]
    Config(String),
    #[error("Acquisition rejected {0}.")]
    Acquisition(String),
    #[error(transparent)]
    Archive(#[from] arus_assets::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
