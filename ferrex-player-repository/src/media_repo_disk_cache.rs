use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::SystemTime,
};

use base64::Engine;
use directories::ProjectDirs;
use ferrex_core::{
    infra::cache::{MediaRepoBlobStore, MediaRepoCacheRoot},
    player_prelude::{LibraryId, MovieBatchId, SeriesID},
};
use sha2::Digest;
use tokio::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Derive the player's stable 64-bit content hash from a `cacache` integrity value.
///
/// This matches the server-side `stable_hash_u64` implementation:
/// `u64::from_be_bytes(sha256(payload_bytes)[..8])`.
pub fn content_hash_u64_from_integrity(
    integrity: &cacache::Integrity,
) -> Option<u64> {
    let raw = integrity.to_string();
    let (alg, b64) = raw.split_once('-')?;
    if alg != "sha256" {
        return None;
    }
    let digest = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    if digest.len() != 32 {
        return None;
    }
    let first: [u8; 8] = digest.get(0..8)?.try_into().ok()?;
    Some(u64::from_be_bytes(first))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MediaRepoCacheNamespace(String);

impl MediaRepoCacheNamespace {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MovieBatchCacheKey {
    pub library_id: LibraryId,
    pub batch_id: MovieBatchId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SeriesBundleCacheKey {
    pub library_id: LibraryId,
    pub series_id: SeriesID,
}

#[derive(Debug, Clone)]
pub struct CachedMediaRepoBlob {
    pub version: u64,
    pub integrity: cacache::Integrity,
    pub byte_len: u32,
}

#[derive(Debug, Clone)]
pub struct CachedMovieBatchEntry {
    pub batch_id: MovieBatchId,
    pub version: u64,
    pub integrity: cacache::Integrity,
    pub byte_len: u32,
}

#[derive(Debug, Clone)]
pub struct CachedSeriesBundleEntry {
    pub series_id: SeriesID,
    pub version: u64,
    pub integrity: cacache::Integrity,
    pub byte_len: u32,
}

#[derive(Debug, Clone)]
pub struct CachedRepoSnapshotEntry {
    pub integrity: cacache::Integrity,
    pub byte_len: u32,
}

#[derive(Debug, Default)]
struct MediaRepoCacheIndex {
    movie_batches: HashMap<MovieBatchCacheKey, CachedMediaRepoBlob>,
    series_bundles: HashMap<SeriesBundleCacheKey, CachedMediaRepoBlob>,
    repo_snapshot: Option<CachedRepoSnapshotEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct MediaRepoCacheIndexFile {
    schema_version: u32,
    #[serde(default)]
    movie_batches: Vec<MovieBatchCacheEntryFile>,
    #[serde(default)]
    series_bundles: Vec<SeriesBundleCacheEntryFile>,
    #[serde(default)]
    repo_snapshot: Option<RepoSnapshotCacheEntryFile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct MovieBatchCacheEntryFile {
    library_id: LibraryId,
    batch_id: MovieBatchId,
    version: u64,
    integrity: String,
    byte_len: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct SeriesBundleCacheEntryFile {
    library_id: LibraryId,
    series_id: SeriesID,
    version: u64,
    integrity: String,
    byte_len: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct RepoSnapshotCacheEntryFile {
    integrity: String,
    byte_len: u32,
}

#[derive(Debug)]
pub struct PlayerDiskMediaRepoCache {
    blob_store: MediaRepoBlobStore,
    index_path: PathBuf,
    index: Mutex<MediaRepoCacheIndex>,
}

impl PlayerDiskMediaRepoCache {
    pub fn try_new_for_server(server_url: &str) -> anyhow::Result<Self> {
        let namespace = namespace_for_server_url(server_url);
        let root = media_repo_cache_root_for_namespace(&namespace)?;
        let root_path = root.as_path();
        std::fs::create_dir_all(root_path)?;

        let index_path = root_path.join("media-repo-index-v1.json");
        let index = load_index_file(&index_path).unwrap_or_default();

        Ok(Self {
            blob_store: MediaRepoBlobStore::new(root),
            index_path,
            index: Mutex::new(index),
        })
    }

    pub fn root(&self) -> &MediaRepoCacheRoot {
        self.blob_store.root()
    }

    pub async fn list_movie_batches_for_library(
        &self,
        library_id: LibraryId,
    ) -> Vec<CachedMovieBatchEntry> {
        let guard = self.index.lock().await;
        let mut out = Vec::new();
        for (key, value) in guard.movie_batches.iter() {
            if key.library_id == library_id {
                out.push(CachedMovieBatchEntry {
                    batch_id: key.batch_id,
                    version: value.version,
                    integrity: value.integrity.clone(),
                    byte_len: value.byte_len,
                });
            }
        }
        out.sort_by_key(|e| e.batch_id.as_u32());
        out
    }

    pub async fn list_series_bundles_for_library(
        &self,
        library_id: LibraryId,
    ) -> Vec<CachedSeriesBundleEntry> {
        let guard = self.index.lock().await;
        let mut out = Vec::new();
        for (key, value) in guard.series_bundles.iter() {
            if key.library_id == library_id {
                out.push(CachedSeriesBundleEntry {
                    series_id: key.series_id,
                    version: value.version,
                    integrity: value.integrity.clone(),
                    byte_len: value.byte_len,
                });
            }
        }
        out.sort_by_key(|e| e.series_id.to_uuid());
        out
    }

    pub async fn read_hash(
        &self,
        integrity: &cacache::Integrity,
    ) -> ferrex_core::error::Result<Vec<u8>> {
        self.blob_store.read_hash(integrity).await
    }

    pub async fn read_repo_snapshot(&self) -> Option<Vec<u8>> {
        let integrity = {
            let guard = self.index.lock().await;
            guard
                .repo_snapshot
                .as_ref()
                .map(|entry| entry.integrity.clone())
        }?;

        match self.blob_store.read_hash(&integrity).await {
            Ok(bytes) => Some(bytes),
            Err(err) => {
                log::warn!(
                    "[Library] media repo snapshot cache read failed; err={}",
                    err
                );
                {
                    let mut guard = self.index.lock().await;
                    guard.repo_snapshot = None;
                }
                let _ = self.persist_index().await;
                let _ = self.blob_store.remove_hash(&integrity).await;
                None
            }
        }
    }

    pub async fn put_repo_snapshot(&self, bytes: &[u8]) -> anyhow::Result<()> {
        let old_integrity = {
            let guard = self.index.lock().await;
            guard
                .repo_snapshot
                .as_ref()
                .map(|entry| entry.integrity.clone())
        };

        let stored = self.blob_store.write_hash(bytes).await?;
        {
            let mut guard = self.index.lock().await;
            guard.repo_snapshot = Some(CachedRepoSnapshotEntry {
                integrity: stored.integrity.clone(),
                byte_len: stored.byte_len as u32,
            });
        }
        if let Some(old_integrity) = old_integrity
            && old_integrity != stored.integrity
        {
            let _ = self.blob_store.remove_hash(&old_integrity).await;
        }
        self.persist_index().await?;
        Ok(())
    }

    pub async fn invalidate_repo_snapshot(&self) {
        let old_integrity = {
            let mut guard = self.index.lock().await;
            guard.repo_snapshot.take().map(|entry| entry.integrity)
        };

        if let Err(err) = self.persist_index().await {
            log::warn!(
                "[Library] media repo snapshot index persist failed; err={}",
                err
            );
        }

        if let Some(old_integrity) = old_integrity {
            let _ = self.blob_store.remove_hash(&old_integrity).await;
        }
    }

    pub async fn remove_movie_batches(
        &self,
        library_id: LibraryId,
        batch_ids: &[MovieBatchId],
    ) {
        let mut guard = self.index.lock().await;
        for batch_id in batch_ids {
            guard.movie_batches.remove(&MovieBatchCacheKey {
                library_id,
                batch_id: *batch_id,
            });
        }
    }

    /// Update the stored version for a movie batch without touching the blob bytes.
    ///
    /// This is used when the server reports a version bump but also provides a
    /// content hash that matches the client's cached bytes.
    pub async fn set_movie_batch_version(
        &self,
        library_id: LibraryId,
        batch_id: MovieBatchId,
        version: u64,
    ) -> bool {
        let mut guard = self.index.lock().await;
        let key = MovieBatchCacheKey {
            library_id,
            batch_id,
        };
        let Some(entry) = guard.movie_batches.get_mut(&key) else {
            return false;
        };
        if entry.version == version {
            return false;
        }
        entry.version = version;
        true
    }

    pub async fn remove_series_bundles(
        &self,
        library_id: LibraryId,
        series_ids: &[SeriesID],
    ) {
        let mut guard = self.index.lock().await;
        for series_id in series_ids {
            guard.series_bundles.remove(&SeriesBundleCacheKey {
                library_id,
                series_id: *series_id,
            });
        }
    }

    pub async fn put_movie_batch(
        &self,
        library_id: LibraryId,
        batch_id: MovieBatchId,
        version: u64,
        bytes: &[u8],
    ) -> anyhow::Result<cacache::Integrity> {
        let stored = self.blob_store.write_hash(bytes).await?;
        let mut guard = self.index.lock().await;
        guard.movie_batches.insert(
            MovieBatchCacheKey {
                library_id,
                batch_id,
            },
            CachedMediaRepoBlob {
                version,
                integrity: stored.integrity.clone(),
                byte_len: stored.byte_len as u32,
            },
        );
        Ok(stored.integrity)
    }

    pub async fn put_series_bundle(
        &self,
        library_id: LibraryId,
        series_id: SeriesID,
        version: u64,
        bytes: &[u8],
    ) -> anyhow::Result<cacache::Integrity> {
        let stored = self.blob_store.write_hash(bytes).await?;
        let mut guard = self.index.lock().await;
        guard.series_bundles.insert(
            SeriesBundleCacheKey {
                library_id,
                series_id,
            },
            CachedMediaRepoBlob {
                version,
                integrity: stored.integrity.clone(),
                byte_len: stored.byte_len as u32,
            },
        );
        Ok(stored.integrity)
    }

    pub async fn persist_index(&self) -> anyhow::Result<()> {
        let snapshot = {
            let guard = self.index.lock().await;
            snapshot_index(&guard)
        };

        let bytes = serde_json::to_vec(&snapshot)?;
        write_atomic(&self.index_path, &bytes)?;
        Ok(())
    }
}

fn snapshot_index(index: &MediaRepoCacheIndex) -> MediaRepoCacheIndexFile {
    let mut movie_batches = Vec::with_capacity(index.movie_batches.len());
    for (key, value) in index.movie_batches.iter() {
        movie_batches.push(MovieBatchCacheEntryFile {
            library_id: key.library_id,
            batch_id: key.batch_id,
            version: value.version,
            integrity: value.integrity.to_string(),
            byte_len: value.byte_len,
        });
    }
    movie_batches
        .sort_by_key(|e| (e.library_id.to_uuid(), e.batch_id.as_u32()));

    let mut series_bundles = Vec::with_capacity(index.series_bundles.len());
    for (key, value) in index.series_bundles.iter() {
        series_bundles.push(SeriesBundleCacheEntryFile {
            library_id: key.library_id,
            series_id: key.series_id,
            version: value.version,
            integrity: value.integrity.to_string(),
            byte_len: value.byte_len,
        });
    }
    series_bundles
        .sort_by_key(|e| (e.library_id.to_uuid(), e.series_id.to_uuid()));

    let repo_snapshot =
        index
            .repo_snapshot
            .as_ref()
            .map(|entry| RepoSnapshotCacheEntryFile {
                integrity: entry.integrity.to_string(),
                byte_len: entry.byte_len,
            });

    MediaRepoCacheIndexFile {
        schema_version: 1,
        movie_batches,
        series_bundles,
        repo_snapshot,
    }
}

fn load_index_file(path: &Path) -> Option<MediaRepoCacheIndex> {
    let bytes = std::fs::read(path).ok()?;

    let parsed: Option<MediaRepoCacheIndexFile> =
        serde_json::from_slice(&bytes).ok();

    Some(index_from_file(parsed?))
}

fn index_from_file(parsed: MediaRepoCacheIndexFile) -> MediaRepoCacheIndex {
    let mut index = MediaRepoCacheIndex::default();

    for entry in parsed.movie_batches {
        let Ok(integrity) = entry.integrity.parse::<cacache::Integrity>()
        else {
            continue;
        };
        index.movie_batches.insert(
            MovieBatchCacheKey {
                library_id: entry.library_id,
                batch_id: entry.batch_id,
            },
            CachedMediaRepoBlob {
                version: entry.version,
                integrity,
                byte_len: entry.byte_len,
            },
        );
    }

    for entry in parsed.series_bundles {
        let Ok(integrity) = entry.integrity.parse::<cacache::Integrity>()
        else {
            continue;
        };
        index.series_bundles.insert(
            SeriesBundleCacheKey {
                library_id: entry.library_id,
                series_id: entry.series_id,
            },
            CachedMediaRepoBlob {
                version: entry.version,
                integrity,
                byte_len: entry.byte_len,
            },
        );
    }

    if let Some(entry) = parsed.repo_snapshot
        && let Ok(integrity) = entry.integrity.parse::<cacache::Integrity>()
    {
        index.repo_snapshot = Some(CachedRepoSnapshotEntry {
            integrity,
            byte_len: entry.byte_len,
        });
    }

    index
}

fn media_repo_cache_root_for_namespace(
    namespace: &MediaRepoCacheNamespace,
) -> anyhow::Result<MediaRepoCacheRoot> {
    let proj_dirs = ProjectDirs::from("", "ferrex", "ferrex-player")
        .ok_or_else(|| anyhow::anyhow!("Failed to resolve ProjectDirs"))?;
    let root: PathBuf = proj_dirs
        .cache_dir()
        .join("media-repo")
        .join(namespace.as_str());
    Ok(MediaRepoCacheRoot::new(root))
}

fn namespace_for_server_url(server_url: &str) -> MediaRepoCacheNamespace {
    let normalized = normalize_server_url(server_url);
    let digest = sha2::Sha256::digest(normalized.as_bytes());
    MediaRepoCacheNamespace(hex_encode(&digest[..16]))
}

fn normalize_server_url(server_url: &str) -> String {
    server_url.trim().trim_end_matches('/').to_ascii_lowercase()
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp_path = parent.join(format!(
        ".{}.tmp.{}",
        path.file_name().and_then(|v| v.to_str()).unwrap_or("index"),
        nanos
    ));
    std::fs::write(&tmp_path, bytes)?;
    std::fs::rename(tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use sha2::Digest;

    #[test]
    fn content_hash_u64_from_integrity_matches_sha256_prefix() {
        let bytes = b"ferrex-media-repo-hash-test";
        let digest = sha2::Sha256::digest(bytes);
        let b64 = base64::engine::general_purpose::STANDARD.encode(digest);
        let integrity: cacache::Integrity =
            format!("sha256-{}", b64).parse().expect("integrity parse");

        let expected: [u8; 8] = digest[..8]
            .try_into()
            .expect("sha256 digest must be at least 8 bytes");
        let expected = u64::from_be_bytes(expected);

        assert_eq!(content_hash_u64_from_integrity(&integrity), Some(expected));
    }

    #[test]
    fn normalize_server_url_is_stable() {
        assert_eq!(
            normalize_server_url("HTTPS://localhost:3000/"),
            "https://localhost:3000"
        );
    }
}
