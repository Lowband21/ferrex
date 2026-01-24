use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::error::{MediaError, Result};

/// File-backed, immutable image blobs keyed by a stable, URL-safe token.
///
/// This exists alongside the integrity-checked `cacache` store to support:
/// - cheap, streamable serving paths (OS page cache friendly)
/// - immutable, cacheable URLs (token-addressed)
#[derive(Clone, Debug)]
pub struct ImageFileStore {
    root: PathBuf,
}

impl ImageFileStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Produce a stable, URL-safe token from a `cacache` integrity string.
    pub fn token_from_integrity(integrity: &str) -> String {
        let digest = Sha256::digest(integrity.as_bytes());
        hex::encode(digest)
    }

    pub fn is_valid_token(token: &str) -> bool {
        token.len() == 64
            && token
                .as_bytes()
                .iter()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    }

    pub fn path_for_token(&self, token: &str) -> Result<PathBuf> {
        if !Self::is_valid_token(token) {
            return Err(MediaError::InvalidMedia(format!(
                "invalid image blob token: {token}"
            )));
        }
        Ok(self.root.join(token))
    }

    pub async fn ensure_root(&self) -> Result<()> {
        tokio::fs::create_dir_all(&self.root).await.map_err(|err| {
            MediaError::Internal(format!(
                "failed to create image blob dir {:?}: {err}",
                self.root
            ))
        })
    }

    pub async fn exists(&self, token: &str) -> Result<bool> {
        let path = self.path_for_token(token)?;
        Ok(tokio::fs::try_exists(path).await.unwrap_or(false))
    }

    /// Best-effort atomic write (tmp + rename). If the blob already exists, this is a no-op.
    pub async fn write_if_missing(
        &self,
        token: &str,
        bytes: &[u8],
    ) -> Result<()> {
        self.ensure_root().await?;
        let path = self.path_for_token(token)?;

        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(());
        }

        let tmp = self
            .root
            .join(format!("{token}.tmp-{}", Uuid::new_v4().simple()));

        let mut file = tokio::fs::File::create(&tmp).await.map_err(|err| {
            MediaError::Internal(format!(
                "failed to create temp image blob {:?}: {err}",
                tmp
            ))
        })?;
        file.write_all(bytes).await.map_err(|err| {
            MediaError::Internal(format!(
                "failed to write temp image blob {:?}: {err}",
                tmp
            ))
        })?;
        file.flush().await.map_err(|err| {
            MediaError::Internal(format!(
                "failed to flush temp image blob {:?}: {err}",
                tmp
            ))
        })?;
        drop(file);

        // If another writer won the race, discard our temp.
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Ok(());
        }

        tokio::fs::rename(&tmp, &path).await.map_err(|err| {
            MediaError::Internal(format!(
                "failed to move image blob {:?} -> {:?}: {err}",
                tmp, path
            ))
        })?;

        Ok(())
    }
}
