use std::{
    fmt,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::error::{MediaError, Result};
use cacache::Integrity;
use ferrex_model::image::ImageSize;
use uuid::Uuid;

/// Root directory for the image blob store.
///
/// This is a dedicated directory that `cacache` will manage internally
/// (index + content-addressed blobs).
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ImageCacheRoot(PathBuf);

impl ImageCacheRoot {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl fmt::Debug for ImageCacheRoot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ImageCacheRoot").field(&self.0).finish()
    }
}

/// Stable key for locating an image blob within the image cache.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ImageCacheKey(String);

impl ImageCacheKey {
    pub fn new(key: String) -> Self {
        Self(key)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ImageCacheKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ImageCacheKey").field(&self.0).finish()
    }
}

impl fmt::Display for ImageCacheKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Build the stable image cache key for a given image id and size.
///
/// This must remain stable across server + player and is intentionally:
/// - human-readable
/// - versioned (prefix)
/// - specific to logical image size to avoid collisions
pub fn image_cache_key_for(iid: Uuid, imz: ImageSize) -> ImageCacheKey {
    let iid = iid.simple();
    let w = imz.width_name();
    let imz = imz.image_variant().to_string();
    let mut string = String::with_capacity(16 + 32 + 16 + 8);
    string.push_str("images/v1/");
    string.push_str(&iid.to_string());
    string.push_str(&w);
    string.push_str(&imz);
    ImageCacheKey(string)
    // ImageCacheKey::new(format!(
    //     "images/v1/iid/{}/{}/{}",
    //     iid.as_hyphenated(),
    //     imz.image_variant(),
    //     imz.width_name()
    // ))
}

/// Minimal metadata returned from a successful store write.
#[derive(Debug, Clone)]
pub struct StoredImageBlob {
    pub integrity: Integrity,
    pub byte_len: usize,
}

/// Minimal metadata returned from a cache index lookup.
#[derive(Debug, Clone)]
pub struct CachedImageBlobMeta {
    pub integrity: Integrity,
    pub byte_len: usize,
    pub written_at: SystemTime,
}

/// A thin typed wrapper over `cacache` for image blobs.
#[derive(Clone, Debug)]
pub struct ImageBlobStore {
    root: ImageCacheRoot,
}

impl ImageBlobStore {
    pub fn new(root: ImageCacheRoot) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &ImageCacheRoot {
        &self.root
    }

    pub async fn metadata(
        &self,
        key: &ImageCacheKey,
    ) -> Result<Option<CachedImageBlobMeta>> {
        let meta = cacache::metadata(self.root.as_path(), key.as_str())
            .await
            .map_err(|e| {
            MediaError::Internal(format!("cacache metadata failed: {e}"))
        })?;

        Ok(meta.map(|m| {
            // `cacache` uses unix millis in `time`.
            let millis = u64::try_from(m.time).unwrap_or(u64::MAX);
            let written_at = UNIX_EPOCH + Duration::from_millis(millis);
            CachedImageBlobMeta {
                integrity: m.integrity,
                byte_len: m.size,
                written_at,
            }
        }))
    }

    pub async fn read(&self, key: &ImageCacheKey) -> Result<Vec<u8>> {
        cacache::read(self.root.as_path(), key.as_str())
            .await
            .map_err(|e| match e {
                cacache::Error::EntryNotFound(_, _) => MediaError::NotFound(
                    format!("cache entry not found: {}", key),
                ),
                cacache::Error::IntegrityError(err) => MediaError::InvalidMedia(
                    format!(
                        "cache entry failed integrity check: {} ({err})",
                        key
                    ),
                ),
                cacache::Error::SizeMismatch(wanted, actual) => {
                    MediaError::InvalidMedia(format!(
                        "cache entry size mismatch: key={}, wanted={wanted}, actual={actual}",
                        key
                    ))
                }
                cacache::Error::IoError(_, msg) => {
                    MediaError::Internal(format!("cacache read I/O error: {msg}"))
                }
                cacache::Error::SerdeError(_, msg) => {
                    MediaError::Internal(format!("cacache read serde error: {msg}"))
                }
            })
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
                        "cache entry size mismatch: key={}, wanted={wanted}, actual={actual}",
                        hash
                    ))
                }
                cacache::Error::IoError(_, msg) => {
                    MediaError::Internal(format!("cacache read I/O error: {msg}"))
                }
                cacache::Error::SerdeError(_, msg) => {
                    MediaError::Internal(format!("cacache read serde error: {msg}"))
                }
            })
    }

    pub async fn remove(&self, key: &ImageCacheKey) -> Result<()> {
        let r_opts = cacache::index::RemoveOpts::new().remove_fully(true);
        r_opts
            .remove(self.root.as_path(), key.as_str())
            .await
            .map_err(|e| {
                MediaError::Internal(format!("cacache remove failed: {e}"))
            })
    }

    pub async fn write(
        &self,
        key: &ImageCacheKey,
        bytes: &[u8],
    ) -> Result<StoredImageBlob> {
        let integrity =
            cacache::write(self.root.as_path(), key.as_str(), bytes)
                .await
                .map_err(|e| {
                    MediaError::Internal(format!("cacache write failed: {e}"))
                })?;

        Ok(StoredImageBlob {
            integrity,
            byte_len: bytes.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::image_cache_key_for;
    use ferrex_model::{PosterSize, image::ImageSize};
    use uuid::Uuid;

    #[test]
    fn image_cache_key_is_stable_and_versioned() {
        let iid =
            Uuid::parse_str("01234567-89ab-cdef-0123-456789abcdef").unwrap();
        let imz = ImageSize::Poster(PosterSize::W185);

        let key = image_cache_key_for(iid, imz);
        assert_eq!(
            key.as_str(),
            "images/v2/iid/01234567-89ab-cdef-0123-456789abcdef/poster/w185"
        );
    }
}
