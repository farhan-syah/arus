use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::Archive;
use crate::archive::{parent, sync_parent};
use crate::error::{Error, Result, io};

pub const MAX_BLOB_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobRef {
    pub sha256: String,
    pub bytes: u64,
}

impl Archive {
    pub fn put_blob(&self, bytes: &[u8]) -> Result<BlobRef> {
        let reference = BlobRef {
            sha256: format!("{:x}", Sha256::digest(bytes)),
            bytes: bytes.len() as u64,
        };
        let path = self.blob_path(&reference.sha256, true)?;
        if path.try_exists().map_err(|error| io(&path, error))? {
            check_blob(&path, &reference.sha256, Some(reference.bytes))?;
            return Ok(reference);
        }
        let directory = parent(&path)?;
        let mut temporary =
            NamedTempFile::new_in(directory).map_err(|error| io(directory, error))?;
        temporary
            .write_all(bytes)
            .map_err(|error| io(&path, error))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| io(&path, error))?;
        match temporary.persist_noclobber(&path) {
            Ok(_) => sync_parent(&path)?,
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                check_blob(&path, &reference.sha256, Some(reference.bytes))?;
            }
            Err(error) => return Err(io(path, error.error)),
        }
        Ok(reference)
    }

    /// Reads and checks a blob of at most 64 MiB.
    pub fn read_blob(&self, sha256: &str) -> Result<Vec<u8>> {
        let path = self.blob_path(sha256, false)?;
        let file = File::open(&path).map_err(|error| io(&path, error))?;
        if file.metadata().map_err(|error| io(&path, error))?.len() > MAX_BLOB_BYTES {
            return Err(Error::BlobLimit {
                path,
                limit: MAX_BLOB_BYTES,
            });
        }
        let mut bytes = Vec::new();
        file.take(MAX_BLOB_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| io(&path, error))?;
        if bytes.len() as u64 > MAX_BLOB_BYTES {
            return Err(Error::BlobLimit {
                path,
                limit: MAX_BLOB_BYTES,
            });
        }
        if format!("{:x}", Sha256::digest(&bytes)) != sha256 {
            return Err(Error::CorruptBlob { path });
        }
        Ok(bytes)
    }

    fn blob_path(&self, sha256: &str, create: bool) -> Result<PathBuf> {
        if sha256.len() != 64
            || !sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::Path {
                path: sha256.into(),
                reason: "expected a lowercase SHA256 digest",
            });
        }
        self.resolve(&Path::new("blobs").join(&sha256[..2]).join(sha256), create)
    }
}

fn check_blob(path: &Path, expected: &str, expected_bytes: Option<u64>) -> Result<()> {
    let mut file = File::open(path).map_err(|error| io(path, error))?;
    if let Some(expected_bytes) = expected_bytes
        && file.metadata().map_err(|error| io(path, error))?.len() != expected_bytes
    {
        return Err(Error::CorruptBlob { path: path.into() });
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| io(path, error))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    if format!("{:x}", hasher.finalize()) != expected {
        return Err(Error::CorruptBlob { path: path.into() });
    }
    Ok(())
}
