use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::{Serialize, de::DeserializeOwned};
use tempfile::NamedTempFile;

use crate::error::{Error, Result, io};
use crate::paths;

const MAX_METADATA_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug)]
pub struct Archive {
    root: PathBuf,
}

#[derive(Debug)]
pub struct ArchiveGuard {
    _file: File,
}

impl Archive {
    pub fn open(root: &Path) -> Result<Self> {
        match fs::symlink_metadata(root) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(Error::Path {
                    path: root.into(),
                    reason: "archive root is a symlink",
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io(root, error)),
        }
        fs::create_dir_all(root).map_err(|error| io(root, error))?;
        let root = fs::canonicalize(root).map_err(|error| io(root, error))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn lock(&self) -> Result<ArchiveGuard> {
        let path = self.resolve(Path::new("archive.lock"), true)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| io(&path, error))?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(ArchiveGuard { _file: file }),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                Err(Error::Locked { path })
            }
            Err(error) => Err(io(path, error)),
        }
    }

    pub fn read_json<T: DeserializeOwned>(&self, relative: &Path) -> Result<Option<T>> {
        let path = self.resolve(relative, false)?;
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io(path, error)),
        };
        let mut bytes = Vec::new();
        file.take(MAX_METADATA_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| io(&path, error))?;
        if bytes.len() as u64 > MAX_METADATA_BYTES {
            return Err(Error::MetadataLimit {
                path,
                limit: MAX_METADATA_BYTES,
            });
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|source| Error::Json { path, source })
    }

    pub fn write_json_atomic<T: Serialize>(&self, relative: &Path, value: &T) -> Result<()> {
        if relative.components().next().is_some_and(|part| {
            ["blobs", "receipts", "archive.lock"]
                .iter()
                .any(|name| part.as_os_str() == *name)
        }) {
            return Err(Error::Path {
                path: relative.into(),
                reason: "reserved immutable archive path",
            });
        }
        let path = self.resolve(relative, true)?;
        let temporary = self.json_temporary(&path, value)?;
        temporary
            .persist(&path)
            .map_err(|error| io(&path, error.error))?;
        sync_parent(&path)
    }

    pub fn append_receipt<T: Serialize>(
        &self,
        collection_id: &str,
        receipt_id: &str,
        value: &T,
    ) -> Result<()> {
        paths::segment(collection_id)?;
        paths::segment(receipt_id)?;
        let relative = Path::new("receipts")
            .join(collection_id)
            .join(format!("{receipt_id}.json"));
        let path = self.resolve(&relative, true)?;
        let temporary = self.json_temporary(&path, value)?;
        temporary.persist_noclobber(&path).map_err(|error| {
            if error.error.kind() == std::io::ErrorKind::AlreadyExists {
                Error::ReceiptExists { path: path.clone() }
            } else {
                io(&path, error.error)
            }
        })?;
        sync_parent(&path)
    }

    pub(crate) fn resolve(&self, relative: &Path, create: bool) -> Result<PathBuf> {
        paths::resolve(&self.root, relative, create)
    }

    fn json_temporary<T: Serialize>(&self, path: &Path, value: &T) -> Result<NamedTempFile> {
        let mut bytes = serde_json::to_vec_pretty(value).map_err(|source| Error::Json {
            path: path.into(),
            source,
        })?;
        bytes.push(b'\n');
        if bytes.len() as u64 > MAX_METADATA_BYTES {
            return Err(Error::MetadataLimit {
                path: path.into(),
                limit: MAX_METADATA_BYTES,
            });
        }
        let parent = parent(path)?;
        let mut temporary = NamedTempFile::new_in(parent).map_err(|error| io(parent, error))?;
        temporary
            .write_all(&bytes)
            .map_err(|error| io(path, error))?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| io(path, error))?;
        Ok(temporary)
    }
}

pub(crate) fn parent(path: &Path) -> Result<&Path> {
    path.parent().ok_or_else(|| Error::Path {
        path: path.into(),
        reason: "missing parent directory",
    })
}

pub(crate) fn sync_parent(path: &Path) -> Result<()> {
    let parent = parent(path)?;
    File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(|error| io(parent, error))
}
