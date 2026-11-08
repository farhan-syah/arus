use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result, io};

pub(crate) fn resolve(root: &Path, relative: &Path, create: bool) -> Result<PathBuf> {
    let parts: Vec<_> = relative.components().collect();
    if parts.is_empty()
        || parts
            .iter()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(Error::Path {
            path: relative.into(),
            reason: "expected nonempty normal components",
        });
    }
    let mut path = root.to_path_buf();
    for (index, part) in parts.iter().enumerate() {
        path.push(part.as_os_str());
        let parent = index + 1 < parts.len();
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(Error::Path {
                        path,
                        reason: "symlink component",
                    });
                }
                if parent && !metadata.is_dir() {
                    return Err(Error::Path {
                        path,
                        reason: "parent is not a directory",
                    });
                }
                if !parent && !metadata.is_file() {
                    return Err(Error::Path {
                        path,
                        reason: "destination is not a file",
                    });
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if parent && create {
                    match fs::create_dir(&path) {
                        Ok(()) => {
                            if let Some(parent) = path.parent() {
                                fs::File::open(parent)
                                    .and_then(|file| file.sync_all())
                                    .map_err(|error| io(parent, error))?;
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                            let metadata =
                                fs::symlink_metadata(&path).map_err(|error| io(&path, error))?;
                            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                                return Err(Error::Path {
                                    path,
                                    reason: "parent changed during creation",
                                });
                            }
                        }
                        Err(error) => return Err(io(&path, error)),
                    }
                }
            }
            Err(error) => return Err(io(&path, error)),
        }
    }
    Ok(path)
}

pub(crate) fn segment(value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(Error::Path {
            path: value.into(),
            reason: "identifier contains unsafe characters",
        });
    }
    Ok(())
}
