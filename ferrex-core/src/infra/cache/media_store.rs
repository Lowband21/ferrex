use std::{
    fmt,
    path::{Path, PathBuf},
};

use cacache::Integrity;

use crate::error::{MediaError, Result};

/// Root directory for the media repo blob store.
///
/// This cache is intended to be used in "hash-only" mode (`cacache::write_hash`
/// / `cacache::read_hash`), avoiding index lookups by persisting the returned
/// `Integrity` alongside higher-level metadata (like bundle/batch versions).
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct MediaRepoCacheRoot(PathBuf);

impl MediaRepoCacheRoot {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl fmt::Debug for MediaRepoCacheRoot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("MediaRepoCacheRoot").field(&self.0).finish()
    }
}

/// Minimal metadata returned from a successful store write.
#[derive(Debug, Clone)]
pub struct StoredMediaRepoBlob {
    pub integrity: Integrity,
    pub byte_len: usize,
}

/// A thin typed wrapper over `cacache` for media repo blobs.
#[derive(Clone, Debug)]
pub struct MediaRepoBlobStore {
    root: MediaRepoCacheRoot,
}

impl MediaRepoBlobStore {
    pub fn new(root: MediaRepoCacheRoot) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &MediaRepoCacheRoot {
        &self.root
    }

    pub async fn read_hash(&self, hash: &Integrity) -> Result<Vec<u8>> {
        cacache::read_hash(self.root.as_path(), hash)
            .await
            .map_err(|e| match e {
                cacache::Error::EntryNotFound(_, _) => MediaError::NotFound(
                    format!("cache entry not found: {}", hash),
                ),
                cacache::Error::IntegrityError(err) => MediaError::InvalidMedia(
                    format!(
                        "cache entry failed integrity check: {} ({err})",
                        hash
                    ),
                ),
                cacache::Error::SizeMismatch(wanted, actual) => {
                    MediaError::InvalidMedia(format!(
                        "cache entry size mismatch: hash={}, wanted={wanted}, actual={actual}",
                        hash
                    ))
                }
                cacache::Error::IoError(_, msg) => {
                    MediaError::Internal(format!(
                        "cacache read_hash I/O error: {msg}"
                    ))
                }
                cacache::Error::SerdeError(_, msg) => {
                    MediaError::Internal(format!(
                        "cacache read_hash serde error: {msg}"
                    ))
                }
            })
    }

    pub async fn write_hash(
        &self,
        bytes: &[u8],
    ) -> Result<StoredMediaRepoBlob> {
        let integrity = cacache::write_hash(self.root.as_path(), bytes)
            .await
            .map_err(|e| {
            MediaError::Internal(format!("cacache write_hash failed: {e}"))
        })?;

        Ok(StoredMediaRepoBlob {
            integrity,
            byte_len: bytes.len(),
        })
    }

    pub async fn remove_hash(&self, hash: &Integrity) -> Result<()> {
        cacache::remove_hash(self.root.as_path(), hash)
            .await
            .map_err(|e| {
                MediaError::Internal(format!("cacache remove_hash failed: {e}"))
            })
    }
}
